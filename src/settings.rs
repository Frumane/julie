//! Kullanıcı ayarları: seçilen arama motoru ve diske kaydedilmesi.
//!
//! Dosya biçimi bilinçli olarak basit tutuldu (`anahtar=değer` satırları),
//! böylece ek bir bağımlılık gerekmiyor.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::ui::theme::{Accent, Appearance, ColorMode, Density, LayoutKind, Tone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let a = &self.appearance;
        out.push_str(&format!(
            "ui_layout={}\nui_mode={}\nui_accent={}\nui_tone={}\nui_density={}\n",
            a.layout.id(),
            a.mode.id(),
            a.accent.id(),
            a.tone.id(),
            a.density.id()
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
            },
            close_with_last_tab: true,
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
        assert!(!Settings::default().close_with_last_tab);
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
