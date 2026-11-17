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
    "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'";

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
    /// Yer imini sil.
    RemoveBookmark(String),
    /// Adresi geçmişten sil.
    DeleteHistory(String),
    /// Bütün geçmişi sil.
    ClearHistory,
    /// Görünüm ayarı değişti (`layout`, `rail`).
    Appearance(String, String),
}

impl Message {
    pub fn parse(raw: &str) -> Option<Message> {
        if let Some(text) = raw.strip_prefix("go:") {
            let text = text.trim();
            return (!text.is_empty()).then(|| Message::Go(text.to_string()));
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
        match raw {
            "smartscreen:on" => Some(Message::SetSmartScreen(true)),
            "smartscreen:off" => Some(Message::SetSmartScreen(false)),
            "history-clear" => Some(Message::ClearHistory),
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
}

/// Sayfanın HTML'ini o anki ayarlar ve yer imleriyle birlikte üretir.
pub fn render(state: State, bookmarks: &[Entry]) -> String {
    let engines: Vec<_> = Engine::ALL
        .iter()
        .map(|e| json!({ "id": e.id(), "name": e.name(), "note": e.note() }))
        .collect();
    let bookmarks: Vec<_> = bookmarks
        .iter()
        .map(|b| json!({ "url": b.url, "title": b.title }))
        .collect();
    let data = json!({
        "engines": engines,
        "selected": state.engine.map(Engine::id),
        "smartscreen": state.smartscreen,
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
        }
    }

    #[test]
    fn injects_state() {
        let html = render(state(Some(Engine::Brave), true), &[]);
        assert!(!html.contains("/*STATE*/"));
        assert!(html.contains(r#""selected":"brave""#));
        assert!(html.contains(r#""smartscreen":true"#));
        assert!(html.contains(r#"{"id":"google","name":"Google""#));
        assert!(render(state(None, false), &[]).contains(r#""selected":null"#));
    }

    #[test]
    fn bookmarks_cannot_break_out_of_the_script() {
        let evil = Entry {
            url: "https://a.example/".into(),
            title: "</script><script>alert(1)</script>".into(),
            time: 0,
        };
        let html = render(state(None, true), &[evil]);
        assert!(!html.contains("<script>alert"));
        assert!(html.contains(r"\u003c/script>"));
    }
}
