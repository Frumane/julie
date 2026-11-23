//! Tarayıcıya gömülü başlangıç sayfası.
//!
//! Sayfa programın içinde durur ve ağdan hiçbir şey indirmez; yeni sekme
//! bu yüzden anında açılır. Sayfa tarayıcıyla `window.ipc.postMessage`
//! üzerinden konuşur.

use serde_json::json;

use crate::places::Entry;
use crate::settings::Engine;

const TEMPLATE: &str = include_str!("start_page.html");

/// Sayfaya verilen güvenlik politikası: dış kaynak yüklenemez, yalnızca
/// sayfanın kendi içindeki stil ve betik çalışır.
pub const CONTENT_SECURITY_POLICY: &str =
    "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:";

/// Tarayıcının kendi sayfalarından (başlangıç, geçmiş) gelen mesajlar.
/// Yalnızca bu sayfalardan kabul edilir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Arama kutusuna yazılan metin.
    Go(String),
    /// Seçilen arama motoru.
    SetEngine(Engine),
    /// SmartScreen zararlı site koruması açıldı/kapandı.
    SetSmartScreen(bool),
    /// Ayarlar sayfası: siteye girişte şifre kaydetme önerilsin mi?
    SetSavePasswords(bool),
    /// Ayarlar sayfası: kayıtlı şifreleri sil.
    ClearPasswords,
    /// Ayarlar sayfası: formlar otomatik doldurulsun mu?
    SetAutofill(bool),
    /// Ayarlar sayfası: kayıtlı form bilgilerini sil.
    ClearAutofill,
    /// Ayarlar: son sekme kapanınca tarayıcı da kapansın mı?
    SetCloseWithLastTab(bool),
    /// Ayarlar: yazarken arama motorundan öneri alınsın mı?
    SetSearchSuggestions(bool),
    /// Ayarlar: adres kutusu geçmişten otomatik tamamlasın mı?
    SetAutocomplete(bool),
    /// Ayarlar: donanım hızlandırma (yeniden başlatınca geçerli).
    SetHardwareAcceleration(bool),
    /// Başlangıç sayfasının arama kutusuna yazılıyor: öneri iste.
    Suggest(String),
    /// Yeni gizli pencere aç.
    OpenPrivateWindow,
    /// Yer imini sil.
    RemoveBookmark(String),
    /// Adresi geçmişten sil.
    DeleteHistory(String),
    /// Bütün geçmişi sil.
    ClearHistory,
    /// Görünüm ayarı değişti (`layout`, `rail`).
    Appearance(String, String),
    /// Ayarlar: arka plandaki sekmeler ne zaman uyutulsun.
    SetTabSleep(crate::sleep::SleepAfter),
    /// Gizlilik ayarı değişti (`https`, `tracking` ...).
    Privacy(String, String),
    /// Uyarı sayfası: şifresiz siteye yine de git.
    AllowHttp(String),
    /// Uyarı ya da hata sayfası: geri dön.
    PageBack,
    /// Hata sayfası: adresi yeniden dene.
    Retry(String),
    /// Hata sayfası: geçersiz sertifikaya rağmen siteye git.
    AllowCert(String),
    /// Hata sayfası: sertifikayı Windows'un penceresinde göster.
    ViewCert,
    /// İndirmeler sayfası: bir indirmeyle ilgili istek.
    Download(crate::downloads::Action, u64),
    /// İndirmeler sayfası: bitenleri listeden temizle.
    ClearDownloads,
    /// İndirmeler sayfası: türü "hep aç" listesinden çıkar.
    ForgetAlwaysOpen(String),
}

impl Message {
    pub fn parse(raw: &str) -> Option<Message> {
        if let Some(text) = raw.strip_prefix("go:") {
            let text = text.trim();
            return (!text.is_empty()).then(|| Message::Go(text.to_string()));
        }
        if let Some(text) = raw.strip_prefix("suggest:") {
            return Some(Message::Suggest(text.to_string()));
        }
        if let Some(id) = raw.strip_prefix("engine:") {
            return Engine::from_id(id).map(Message::SetEngine);
        }
        if let Some(url) = raw.strip_prefix("bookmark-remove:") {
            return Some(Message::RemoveBookmark(url.to_string()));
        }
        if let Some(url) = raw.strip_prefix("history-delete:") {
            return Some(Message::DeleteHistory(url.to_string()));
        }
        if let Some(change) = raw.strip_prefix("appearance:") {
            let (key, value) = change.split_once('=')?;
            return Some(Message::Appearance(key.to_string(), value.to_string()));
        }
        if let Some(id) = raw.strip_prefix("tab-sleep:") {
            return crate::sleep::SleepAfter::from_id(id).map(Message::SetTabSleep);
        }
        if let Some(change) = raw.strip_prefix("privacy:") {
            let (key, value) = change.split_once('=')?;
            return Some(Message::Privacy(key.to_string(), value.to_string()));
        }
        if let Some(url) = raw.strip_prefix("error-retry:") {
            return (url.starts_with("http://") || url.starts_with("https://"))
                .then(|| Message::Retry(url.to_string()));
        }
        if let Some(url) = raw.strip_prefix("cert-allow:") {
            return url
                .starts_with("https://")
                .then(|| Message::AllowCert(url.to_string()));
        }
        if let Some(url) = raw.strip_prefix("http-allow:") {
            return url
                .starts_with("http://")
                .then(|| Message::AllowHttp(url.to_string()));
        }
        if let Some(ext) = raw.strip_prefix("downloads-forget:") {
            return Some(Message::ForgetAlwaysOpen(ext.to_string()));
        }
        if let Some(rest) = raw.strip_prefix("download-") {
            let (action, id) = rest.split_once(':')?;
            return Some(Message::Download(
                crate::downloads::Action::parse(action)?,
                id.parse().ok()?,
            ));
        }
        match raw {
            "cert-view" => Some(Message::ViewCert),
            "downloads-clear" => Some(Message::ClearDownloads),
            "passwords:on" => Some(Message::SetSavePasswords(true)),
            "passwords:off" => Some(Message::SetSavePasswords(false)),
            "passwords-clear" => Some(Message::ClearPasswords),
            "autofill:on" => Some(Message::SetAutofill(true)),
            "autofill:off" => Some(Message::SetAutofill(false)),
            "autofill-clear" => Some(Message::ClearAutofill),
            "smartscreen:on" => Some(Message::SetSmartScreen(true)),
            "smartscreen:off" => Some(Message::SetSmartScreen(false)),
            "last-tab-closes:on" => Some(Message::SetCloseWithLastTab(true)),
            "last-tab-closes:off" => Some(Message::SetCloseWithLastTab(false)),
            "search-suggestions:on" => Some(Message::SetSearchSuggestions(true)),
            "search-suggestions:off" => Some(Message::SetSearchSuggestions(false)),
            "autocomplete:on" => Some(Message::SetAutocomplete(true)),
            "autocomplete:off" => Some(Message::SetAutocomplete(false)),
            "gpu:on" => Some(Message::SetHardwareAcceleration(true)),
            "gpu:off" => Some(Message::SetHardwareAcceleration(false)),
            "history-clear" => Some(Message::ClearHistory),
            "private-window" => Some(Message::OpenPrivateWindow),
            "insecure-back" | "error-back" => Some(Message::PageBack),
            _ => None,
        }
    }
}

/// Sayfayı üretmek için gereken ayarlar.
#[derive(Debug, Clone, Copy)]
pub struct State {
    pub engine: Option<Engine>,
    pub smartscreen: bool,
    /// Ayarlar sayfası için görünüm ayarları ve Windows vurgu rengi.
    pub appearance: crate::ui::theme::Appearance,
    pub system_accent: crate::ui::theme::Color,
    /// Ayarlar sayfası için: son sekme kapanınca tarayıcı da kapansın mı?
    pub close_with_last_tab: bool,
    /// Ayarlar sayfası için: arama önerileri ve otomatik tamamlama açık mı?
    pub search_suggestions: bool,
    pub autocomplete: bool,
    /// Gizli pencere mi (başlangıç sayfası bunu belirtir).
    pub private: bool,
    /// Ayarlar sayfası için gizlilik seçenekleri.
    pub privacy: crate::settings::Privacy,
    /// Ayarlar sayfası için sekme uyutma süresi.
    pub tab_sleep: crate::sleep::SleepAfter,
    /// Ayarlar sayfası için: kayıtlı tercih ve şu an çalışan durum (ikisi
    /// farklıysa "yeniden başlat" notu gösterilir).
    pub gpu: bool,
    pub gpu_running: bool,
    /// Ayarlar sayfası için: şifre kaydetme önerilsin mi?
    pub save_passwords: bool,
    /// Ayarlar sayfası için: formlar (ad, adres…) otomatik doldurulsun mu?
    pub autofill: bool,
}

/// Sayfanın HTML'ini o anki ayarlar ve yer imleriyle birlikte üretir.
/// `icon`, bir adres için kayıtlı site ikonunu (PNG) verir.
pub fn render(state: State, bookmarks: &[Entry], icon: impl Fn(&str) -> Option<Vec<u8>>) -> String {
    use base64::Engine as _;
    let engines: Vec<_> = Engine::ALL
        .iter()
        .map(|e| json!({ "id": e.id(), "name": e.name(), "note": e.note() }))
        .collect();
    let bookmarks: Vec<_> = bookmarks
        .iter()
        .map(|b| {
            let icon = icon(&b.url).map(|png| {
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                )
            });
            json!({ "url": b.url, "title": b.title, "icon": icon })
        })
        .collect();
    let data = json!({
        "engines": engines,
        "selected": state.engine.map(Engine::id),
        "smartscreen": state.smartscreen,
        "private": state.private,
        "bookmarks": bookmarks,
    });
    embed_state(TEMPLATE, &data)
}

/// Veriyi sayfanın `/*STATE*/null` yer tutucusuna JSON olarak yerleştirir.
/// Sayfa başlıkları gibi dışarıdan gelen metinler `</script>` içerebilir;
/// `<` kaçışlanarak betik bloğunun dışına taşmaları engellenir.
pub fn embed_state(template: &str, data: &serde_json::Value) -> String {
    let json = data
        .to_string()
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    template.replace("/*STATE*/null", &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages() {
        assert_eq!(
            Message::parse("download-pause:7"),
            Some(Message::Download(crate::downloads::Action::Pause, 7))
        );
        assert_eq!(Message::parse("download-pause:x"), None);
        assert_eq!(Message::parse("download-sil:7"), None);
        assert_eq!(
            Message::parse("downloads-clear"),
            Some(Message::ClearDownloads)
        );
        assert_eq!(
            Message::parse("go: rust "),
            Some(Message::Go("rust".into()))
        );
        assert_eq!(Message::parse("go:   "), None);
        assert_eq!(
            Message::parse("engine:google"),
            Some(Message::SetEngine(Engine::Google))
        );
        assert_eq!(Message::parse("engine:yok"), None);
        assert_eq!(
            Message::parse("smartscreen:off"),
            Some(Message::SetSmartScreen(false))
        );
        assert_eq!(
            Message::parse("passwords:off"),
            Some(Message::SetSavePasswords(false))
        );
        assert_eq!(
            Message::parse("passwords-clear"),
            Some(Message::ClearPasswords)
        );
        assert_eq!(
            Message::parse("autofill:off"),
            Some(Message::SetAutofill(false))
        );
        assert_eq!(
            Message::parse("autofill-clear"),
            Some(Message::ClearAutofill)
        );
        assert_eq!(
            Message::parse("last-tab-closes:on"),
            Some(Message::SetCloseWithLastTab(true))
        );
        assert_eq!(
            Message::parse("search-suggestions:off"),
            Some(Message::SetSearchSuggestions(false))
        );
        assert_eq!(
            Message::parse("autocomplete:off"),
            Some(Message::SetAutocomplete(false))
        );
        assert_eq!(
            Message::parse("suggest:hava "),
            Some(Message::Suggest("hava ".into()))
        );
        assert_eq!(Message::parse("başka bir şey"), None);
    }

    #[test]
    fn parses_page_messages() {
        assert_eq!(
            Message::parse("bookmark-remove:https://a.example/"),
            Some(Message::RemoveBookmark("https://a.example/".into()))
        );
        assert_eq!(
            Message::parse("history-delete:https://a.example/?x=1"),
            Some(Message::DeleteHistory("https://a.example/?x=1".into()))
        );
        assert_eq!(Message::parse("history-clear"), Some(Message::ClearHistory));
        assert_eq!(
            Message::parse("private-window"),
            Some(Message::OpenPrivateWindow)
        );
        assert_eq!(
            Message::parse("appearance:layout=rail"),
            Some(Message::Appearance("layout".into(), "rail".into()))
        );
        assert_eq!(Message::parse("appearance:bozuk"), None);
    }

    fn state(engine: Option<Engine>, smartscreen: bool) -> State {
        State {
            engine,
            smartscreen,
            appearance: Default::default(),
            system_accent: crate::ui::theme::Color::hex(0x0078d4),
            close_with_last_tab: false,
            search_suggestions: true,
            autocomplete: true,
            private: false,
            privacy: Default::default(),
            tab_sleep: Default::default(),
            gpu: true,
            gpu_running: true,
            save_passwords: true,
            autofill: true,
        }
    }

    #[test]
    fn injects_state() {
        let html = render(state(Some(Engine::Brave), true), &[], |_| None);
        assert!(!html.contains("/*STATE*/"));
        assert!(html.contains(r#""selected":"brave""#));
        assert!(html.contains(r#""smartscreen":true"#));
        assert!(html.contains(r#"{"id":"google","name":"Google""#));
        assert!(render(state(None, false), &[], |_| None).contains(r#""selected":null"#));
    }

    #[test]
    fn bookmarks_cannot_break_out_of_the_script() {
        let evil = Entry {
            url: "https://a.example/".into(),
            title: "</script><script>alert(1)</script>".into(),
            time: 0,
        };
        let html = render(state(None, true), &[evil], |_| None);
        assert!(!html.contains("<script>alert"));
        assert!(html.contains(r"\u003c/script>"));
    }

    #[test]
    fn embeds_bookmark_icons() {
        let site = Entry {
            url: "https://youtube.com/".into(),
            title: "YouTube".into(),
            time: 0,
        };
        let html = render(state(None, true), &[site], |url| {
            url.contains("youtube").then(|| vec![1, 2, 3])
        });
        assert!(html.contains(r#""icon":"data:image/png;base64,AQID""#));
    }
}
