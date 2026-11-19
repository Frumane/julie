//! Kullanıcı ayarları: seçilen arama motoru ve diske kaydedilmesi.
//!
//! Dosya biçimi bilinçli olarak basit tutuldu (`anahtar=değer` satırları),
//! böylece ek bir bağımlılık gerekmiyor.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::privacy::TrackingLevel;
use crate::sleep::SleepAfter;
use crate::ui::theme::{Accent, Appearance, ColorMode, Density, FindCorner, LayoutKind, Tone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Engine {
    DuckDuckGo,
    Google,
    Bing,
    Brave,
    Startpage,
    Ecosia,
    Yandex,
}

impl Engine {
    pub const ALL: [Engine; 7] = [
        Engine::Google,
        Engine::DuckDuckGo,
        Engine::Bing,
        Engine::Brave,
        Engine::Startpage,
        Engine::Ecosia,
        Engine::Yandex,
    ];

    /// Henüz seçim yapılmamışken kullanılan motor.
    pub const FALLBACK: Engine = Engine::DuckDuckGo;

    /// Ayar dosyasında ve başlangıç sayfasında kullanılan kimlik.
    pub fn id(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "duckduckgo",
            Engine::Google => "google",
            Engine::Bing => "bing",
            Engine::Brave => "brave",
            Engine::Startpage => "startpage",
            Engine::Ecosia => "ecosia",
            Engine::Yandex => "yandex",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "DuckDuckGo",
            Engine::Google => "Google",
            Engine::Bing => "Bing",
            Engine::Brave => "Brave Search",
            Engine::Startpage => "Startpage",
            Engine::Ecosia => "Ecosia",
            Engine::Yandex => "Yandex",
        }
    }

    /// Başlangıç sayfasında motorun altında görünen kısa açıklama.
    pub fn note(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "Takip etmez",
            Engine::Google => "En kapsamlı sonuçlar",
            Engine::Bing => "Microsoft",
            Engine::Brave => "Kendi dizini, takip etmez",
            Engine::Startpage => "Google sonuçları, takipsiz",
            Engine::Ecosia => "Gelirle ağaç diker",
            Engine::Yandex => "Türkçe sonuçlarda güçlü",
        }
    }

    /// Arama sorgusunun sonuna ekleneceği adres.
    pub fn search_prefix(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "https://duckduckgo.com/?q=",
            Engine::Google => "https://www.google.com/search?q=",
            Engine::Bing => "https://www.bing.com/search?q=",
            Engine::Brave => "https://search.brave.com/search?q=",
            Engine::Startpage => "https://www.startpage.com/do/search?q=",
            Engine::Ecosia => "https://www.ecosia.org/search?q=",
            Engine::Yandex => "https://yandex.com.tr/search/?text=",
        }
    }

    /// Yazarken arama önerilerinin alınacağı adres (Firefox'un da kullandığı
    /// OpenSearch öneri servisleri; cevap `["sorgu", ["öneri", ...]]`).
    pub fn suggest_url(self, query: &str) -> String {
        let prefix = match self {
            Engine::DuckDuckGo => "https://duckduckgo.com/ac/?type=list&kl=tr-tr&q=",
            Engine::Google => {
                "https://suggestqueries.google.com/complete/search?client=firefox&hl=tr&ie=utf-8&oe=utf-8&q="
            }
            Engine::Bing => "https://www.bing.com/osjson.aspx?form=OSDJAS&setlang=tr&query=",
            Engine::Brave => "https://search.brave.com/api/suggest?q=",
            Engine::Startpage => "https://www.startpage.com/osuggestions?q=",
            Engine::Ecosia => "https://ac.ecosia.org/autocomplete?type=list&q=",
            Engine::Yandex => "https://suggest.yandex.com.tr/suggest-ff.cgi?uil=tr&part=",
        };
        format!("{prefix}{}", crate::address::encode_query(query.trim()))
    }

    pub fn from_id(id: &str) -> Option<Engine> {
        Engine::ALL.into_iter().find(|e| e.id() == id.trim())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `None`: kullanıcı henüz arama motoru seçmedi (ilk açılış).
    pub engine: Option<Engine>,
    /// Reklam engelleyicinin kapalı olduğu siteler (`www.` olmadan).
    pub blocking_disabled: BTreeSet<String>,
    /// Microsoft SmartScreen zararlı site koruması kapatıldı mı?
    /// (Varsayılan: açık.)
    pub smartscreen_off: bool,
    /// Arayüzün düzeni ve renkleri.
    pub appearance: Appearance,
    /// Son sekme kapanınca tarayıcı da kapansın mı? (Varsayılan: hayır;
    /// yerine yeni bir sekme açılır.)
    pub close_with_last_tab: bool,
    /// Yazarken arama motorundan öneri alınmasın. (Varsayılan: alınır.)
    pub search_suggestions_off: bool,
    /// Adres kutusu geçmişten otomatik tamamlamasın. (Varsayılan: tamamlar.)
    pub autocomplete_off: bool,
    /// Gizlilik ve güvenlik seçenekleri.
    pub privacy: Privacy,
    /// Arka plandaki sekmeler ne zaman uyutulsun.
    pub tab_sleep: SleepAfter,
}

/// Gizlilik ve güvenlik seçenekleri. Hepsi varsayılan olarak koruyucu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Privacy {
    /// `http://` adresleri önce şifreli denenmesin.
    pub https_first_off: bool,
    /// Adreslerdeki takip parametreleri temizlenmesin.
    pub strip_tracking_off: bool,
    /// Edge'in izleme önleme seviyesi.
    pub tracking: TrackingLevel,
}

impl Privacy {
    /// Ayarlar sayfasından gelen değişikliği uygular; anahtar bilinmiyorsa
    /// `false` döner.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let on = value == "on";
        match key {
            "https" => self.https_first_off = !on,
            "strip" => self.strip_tracking_off = !on,
            "tracking" => match TrackingLevel::from_id(value) {
                Some(level) => self.tracking = level,
                None => return false,
            },
            _ => return false,
        }
        true
    }
}

impl Settings {
    /// Seçili motor, seçim yoksa varsayılan.
    pub fn engine_or_fallback(&self) -> Engine {
        self.engine.unwrap_or(Engine::FALLBACK)
    }

    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "search_engine" => settings.engine = Engine::from_id(value),
                "smartscreen" => settings.smartscreen_off = value == "off",
                "close_with_last_tab" => settings.close_with_last_tab = value == "on",
                "search_suggestions" => settings.search_suggestions_off = value == "off",
                "autocomplete" => settings.autocomplete_off = value == "off",
                "https_first" => settings.privacy.https_first_off = value == "off",
                "strip_tracking" => settings.privacy.strip_tracking_off = value == "off",
                "tab_sleep" => {
                    if let Some(v) = SleepAfter::from_id(value) {
                        settings.tab_sleep = v;
                    }
                }
                "tracking_prevention" => {
                    if let Some(level) = TrackingLevel::from_id(value) {
                        settings.privacy.tracking = level;
                    }
                }
                "ui_layout" => {
                    if let Some(v) = LayoutKind::from_id(value) {
                        settings.appearance.layout = v;
                    }
                }
                "ui_mode" => {
                    if let Some(v) = ColorMode::from_id(value) {
                        settings.appearance.mode = v;
                    }
                }
                "ui_accent" => {
                    if let Some(v) = Accent::from_id(value) {
                        settings.appearance.accent = v;
                    }
                }
                "ui_tone" => {
                    if let Some(v) = Tone::from_id(value) {
                        settings.appearance.tone = v;
                    }
                }
                "ui_find" => {
                    if let Some(v) = FindCorner::from_id(value) {
                        settings.appearance.find = v;
                    }
                }
                "ui_density" => {
                    if let Some(v) = Density::from_id(value) {
                        settings.appearance.density = v;
                    }
                }
                "blocking_disabled" if !value.is_empty() => {
                    settings.blocking_disabled.insert(value.to_lowercase());
                }
                _ => {}
            }
        }
        settings
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# Julie ayarları\n");
        if let Some(engine) = self.engine {
            out.push_str(&format!("search_engine={}\n", engine.id()));
        }
        if self.smartscreen_off {
            out.push_str("smartscreen=off\n");
        }
        if self.close_with_last_tab {
            out.push_str("close_with_last_tab=on\n");
        }
        if self.search_suggestions_off {
            out.push_str("search_suggestions=off\n");
        }
        if self.autocomplete_off {
            out.push_str("autocomplete=off\n");
        }
        let p = &self.privacy;
        if p.https_first_off {
            out.push_str("https_first=off\n");
        }
        if p.strip_tracking_off {
            out.push_str("strip_tracking=off\n");
        }
        if self.tab_sleep != SleepAfter::default() {
            out.push_str(&format!("tab_sleep={}\n", self.tab_sleep.id()));
        }
        if p.tracking != TrackingLevel::default() {
            out.push_str(&format!("tracking_prevention={}\n", p.tracking.id()));
        }
        let a = &self.appearance;
        out.push_str(&format!(
            "ui_layout={}\nui_mode={}\nui_accent={}\nui_tone={}\nui_density={}\nui_find={}\n",
            a.layout.id(),
            a.mode.id(),
            a.accent.id(),
            a.tone.id(),
            a.density.id(),
            a.find.id()
        ));
        for host in &self.blocking_disabled {
            out.push_str(&format!("blocking_disabled={host}\n"));
        }
        out
    }

    /// Dosya yoksa ya da okunamazsa varsayılan ayarlar döner.
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .map(|text| Settings::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Önce geçici dosyaya yaz, sonra yerine taşı: yazma sırasında
        // program kapanırsa ayar dosyası bozulmaz.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.serialize())?;
        std::fs::rename(tmp, path)
    }
}

/// `entry` kaydı `host` sitesini kapsıyor mu? `example.com` kaydı
/// `example.com` ve `haber.example.com` adreslerini kapsar.
pub fn covers(entry: &str, host: &str) -> bool {
    host == entry
        || host
            .strip_suffix(entry)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// Kümedeki kayıtlardan biri `host` sitesini kapsıyor mu?
pub fn is_listed(set: &BTreeSet<String>, host: &str) -> bool {
    set.iter().any(|entry| covers(entry, host))
}

/// Uygulama verilerinin klasörü: `%LOCALAPPDATA%\Julie`.
pub fn app_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Julie"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for engine in Engine::ALL {
            assert_eq!(Engine::from_id(engine.id()), Some(engine));
        }
        assert_eq!(Engine::from_id("yok"), None);
    }

    #[test]
    fn parse_and_serialize() {
        let settings = Settings {
            engine: Some(Engine::Google),
            blocking_disabled: ["example.com".to_string(), "site.org".to_string()].into(),
            smartscreen_off: true,
            appearance: Appearance {
                layout: LayoutKind::Classic,
                mode: ColorMode::Dark,
                accent: Accent::Windows,
                tone: Tone::Peach,
                density: Density::Compact,
                find: FindCorner::BottomLeft,
            },
            close_with_last_tab: true,
            search_suggestions_off: true,
            autocomplete_off: true,
            privacy: Privacy {
                https_first_off: true,
                strip_tracking_off: true,
                tracking: TrackingLevel::Strict,
            },
            tab_sleep: SleepAfter::Never,
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
        let mut p = Privacy::default();
        assert!(p.set("https", "off") && p.https_first_off);
        assert!(p.set("tracking", "basic") && p.tracking == TrackingLevel::Basic);
        assert!(!p.set("tracking", "bilinmeyen"));
        assert!(!p.set("yok", "on"));
        assert!(!Settings::default().close_with_last_tab);
        assert!(!Settings::default().search_suggestions_off);
        assert!(!Settings::default().autocomplete_off);
        assert_eq!(
            Engine::Google.suggest_url(" hava durumu "),
            "https://suggestqueries.google.com/complete/search?client=firefox&hl=tr&ie=utf-8&oe=utf-8&q=hava+durumu"
        );
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(
            Settings::parse("search_engine = brave\n").engine,
            Some(Engine::Brave)
        );
        assert_eq!(Settings::parse("search_engine=bilinmeyen").engine, None);
    }

    #[test]
    fn blocking_exceptions_cover_subdomains() {
        let list = Settings::parse("blocking_disabled=example.com").blocking_disabled;
        assert!(is_listed(&list, "example.com"));
        assert!(is_listed(&list, "haber.example.com"));
        assert!(!is_listed(&list, "example.org"));
        assert!(!is_listed(&list, "notexample.com"));
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("julie-test-{}", std::process::id()));
        let path = dir.join("settings.ini");
        let settings = Settings {
            engine: Some(Engine::Yandex),
            ..Default::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
