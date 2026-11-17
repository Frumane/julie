//! Kozmetik filtrelemenin sayfa tarafı: sayfaya enjekte edilen betik ve
//! sayfayla tarayıcı arasındaki mesajlar.

/// Her sayfanın başında çalışan betik.
pub const SCRIPT: &str = include_str!("cosmetic.js");

/// Sayfadaki betikten gelen mesaj.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Sayfa yüklenmeye başladı; adrese özgü kuralları iste.
    Start,
    /// Sayfada yeni görülen sınıf ve kimlik adları.
    Seen {
        classes: Vec<String>,
        ids: Vec<String>,
    },
}

/// Bir mesajda kabul edilen en fazla ad sayısı (kötü niyetli bir sayfa
/// tarayıcıyı meşgul edemesin diye).
const MAX_NAMES: usize = 5000;

impl Message {
    pub fn parse(raw: &str) -> Option<Message> {
        if raw == "cosmetic-start" {
            return Some(Message::Start);
        }
        let body = raw.strip_prefix("cosmetic-seen:")?;
        let (classes, ids) = body.split_once('\n').unwrap_or((body, ""));
        let words = |s: &str| -> Vec<String> {
            s.split_whitespace()
                .take(MAX_NAMES)
                .map(str::to_string)
                .collect()
        };
        Some(Message::Seen {
            classes: words(classes),
            ids: words(ids),
        })
    }
}

/// Seçicileri gizleyen CSS. Her seçici ayrı kural olarak yazılır: tek bir
/// geçersiz seçici bütün kuralı bozmasın.
pub fn hide_css<S: AsRef<str>>(selectors: &[S]) -> String {
    let mut css = String::new();
    for selector in selectors {
        let selector = selector.as_ref();
        // Seçici kuralın dışına taşamasın.
        if selector.contains(['{', '}']) || selector.contains("</") {
            continue;
        }
        css.push_str(selector);
        css.push_str("{display:none!important}\n");
    }
    css
}

/// CSS'i sayfaya uygulayan betik çağrısı.
pub fn apply_script(css: &str) -> String {
    format!(
        "window.__julieCosmetic&&window.__julieCosmetic.hide({})",
        js_string(css)
    )
}

/// Metni JavaScript dizesi olarak güvenle yazar.
fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages() {
        assert_eq!(Message::parse("cosmetic-start"), Some(Message::Start));
        assert_eq!(
            Message::parse("cosmetic-seen:ad banner\nsponsor"),
            Some(Message::Seen {
                classes: vec!["ad".into(), "banner".into()],
                ids: vec!["sponsor".into()],
            })
        );
        assert_eq!(
            Message::parse("cosmetic-seen:only-classes"),
            Some(Message::Seen {
                classes: vec!["only-classes".into()],
                ids: vec![],
            })
        );
        assert_eq!(Message::parse("go:başka"), None);
    }

    #[test]
    fn builds_css_rule_per_selector() {
        assert_eq!(
            hide_css(&[".ad", "#x > div"]),
            ".ad{display:none!important}\n#x > div{display:none!important}\n"
        );
        assert_eq!(hide_css(&["a{} body"]), "");
    }

    #[test]
    fn escapes_script_strings() {
        assert_eq!(js_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(js_string("x\n</style>"), r#""x\n\u003c/style>""#);
        assert_eq!(js_string("\u{2028}"), r#""\u2028""#);
    }
}
