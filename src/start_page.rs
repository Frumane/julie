//! Tarayıcıya gömülü başlangıç sayfası.
//!
//! Sayfa programın içinde durur ve ağdan hiçbir şey indirmez; yeni sekme
//! bu yüzden anında açılır. Sayfa tarayıcıyla `window.ipc.postMessage`
//! üzerinden konuşur.

use crate::settings::Engine;

const TEMPLATE: &str = include_str!("start_page.html");

/// Sayfaya verilen güvenlik politikası: dış kaynak yüklenemez, yalnızca
/// sayfanın kendi içindeki stil ve betik çalışır.
pub const CONTENT_SECURITY_POLICY: &str =
    "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'";

/// Başlangıç sayfasından gelen mesajlar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Arama kutusuna yazılan metin.
    Go(String),
    /// Seçilen arama motoru.
    SetEngine(Engine),
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
        None
    }
}

/// Sayfanın HTML'ini o anki seçili motorla birlikte üretir.
pub fn render(selected: Option<Engine>) -> String {
    let engines: Vec<String> = Engine::ALL
        .iter()
        .map(|e| {
            format!(
                r#"{{"id":"{}","name":"{}","note":"{}"}}"#,
                e.id(),
                e.name(),
                e.note()
            )
        })
        .collect();
    let selected = match selected {
        Some(e) => format!(r#""{}""#, e.id()),
        None => "null".to_string(),
    };
    let state = format!(
        r#"{{"engines":[{}],"selected":{}}}"#,
        engines.join(","),
        selected
    );
    TEMPLATE.replace("/*STATE*/null", &state)
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
        assert_eq!(Message::parse("başka bir şey"), None);
    }

    #[test]
    fn injects_state() {
        let html = render(Some(Engine::Brave));
        assert!(!html.contains("/*STATE*/"));
        assert!(html.contains(r#""selected":"brave""#));
        assert!(html.contains(r#"{"id":"google","name":"Google""#));
        assert!(render(None).contains(r#""selected":null"#));
    }
}
