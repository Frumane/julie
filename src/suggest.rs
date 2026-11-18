//! Adres çubuğu önerileri: yazılan metinden ve geçmişten satırlar üretir.
//!
//! İlk satır her zaman Enter'a basınca olacak şeydir (siteye git ya da ara).
//! Altında geçmişten ve yer imlerinden eşleşen siteler, sonra arama
//! motorunun önerileri gelir.

use crate::address;
use crate::places::Suggestion;
use crate::settings::Engine;

/// Gösterilecek en fazla geçmiş/yer imi satırı (ilk satır hariç).
pub const MAX_MATCHES: usize = 6;
/// Arama motoru önerileri de varken geçmişten gösterilecek satır sayısı.
const MATCHES_WITH_REMOTE: usize = 3;
/// Listedeki en fazla satır.
const MAX_ROWS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Yazılan metni arama motorunda ara.
    Search,
    /// Yazılan adrese git.
    Visit,
    /// Geçmişteki bir sayfa.
    History,
    /// Yer imlerindeki bir sayfa.
    Bookmark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    /// Satırın ana yazısı (sayfa başlığı ya da yazılan metin).
    pub title: String,
    /// Yanındaki soluk yazı (adres ya da "Google ile ara").
    pub detail: String,
    /// Seçilince adres çubuğuna verilecek metin.
    pub target: String,
    /// Site ikonunun anahtarı (alan adı).
    pub icon: Option<String>,
}

/// Yazılan metin, geçmişteki eşleşmeler ve arama motorunun önerilerinden
/// satırları kurar. `completed`: adres kutusunun otomatik tamamladığı metin
/// (Enter'a basınca ona gidilir).
pub fn build(
    text: &str,
    completed: Option<&str>,
    engine: Engine,
    matches: Vec<Suggestion>,
    remote: &[String],
) -> Vec<Item> {
    let typed = text.trim();
    if typed.is_empty() {
        return Vec::new();
    }
    let text = completed.map_or(typed, str::trim);
    let resolved = address::resolve(text, engine.search_prefix());
    let searching = resolved.starts_with(engine.search_prefix());
    let first = if searching {
        Item {
            kind: Kind::Search,
            title: text.to_string(),
            detail: format!("{} ile ara", engine.name()),
            target: text.to_string(),
            icon: None,
        }
    } else {
        Item {
            kind: Kind::Visit,
            title: display_url(&resolved),
            detail: "Siteye git".to_string(),
            target: text.to_string(),
            icon: address::host_of(&resolved),
        }
    };
    let mut items = vec![first];
    let max_matches = if remote.is_empty() {
        MAX_MATCHES
    } else {
        MATCHES_WITH_REMOTE
    };
    for m in matches {
        if items.len() > max_matches {
            break;
        }
        if !searching && same_page(&m.url, &resolved) {
            continue;
        }
        let shown = display_url(&m.url);
        items.push(Item {
            kind: if m.bookmarked {
                Kind::Bookmark
            } else {
                Kind::History
            },
            title: if m.title.trim().is_empty() {
                shown.clone()
            } else {
                m.title.trim().to_string()
            },
            detail: shown,
            icon: address::host_of(&m.url),
            target: m.url,
        });
    }
    for phrase in remote {
        if items.len() >= MAX_ROWS {
            break;
        }
        let phrase = phrase.trim();
        let repeated = phrase.is_empty()
            || phrase.eq_ignore_ascii_case(typed)
            || items
                .iter()
                .any(|i| i.kind == Kind::Search && i.title.eq_ignore_ascii_case(phrase));
        if repeated {
            continue;
        }
        items.push(Item {
            kind: Kind::Search,
            title: phrase.to_string(),
            detail: String::new(),
            target: phrase.to_string(),
            icon: None,
        });
    }
    items
}

/// Arama motoruna öneri için gönderilebilir mi? Adres gibi görünen metinler
/// (yazılan site, yerel ağ adresleri) dışarı gönderilmez.
pub fn may_ask_engine(text: &str, engine: Engine) -> bool {
    let text = text.trim();
    !text.is_empty()
        && text.chars().count() <= 80
        && address::resolve(text, engine.search_prefix()).starts_with(engine.search_prefix())
}

/// Cevabın metni. Motorlar bazen Türkiye'den gelen isteğe Türkçe kod
/// sayfasıyla (ISO-8859-9 / Windows-1254) ya da Latin-1 ile cevap verir;
/// `content_type` başlığındaki karakter kümesine göre çözülür.
pub fn decode_body(bytes: &[u8], content_type: &str) -> String {
    let charset = content_type
        .split(';')
        .find_map(|p| p.trim().strip_prefix("charset="))
        .map(|c| c.trim_matches('"').to_ascii_lowercase())
        .unwrap_or_default();
    let turkish = matches!(
        charset.as_str(),
        "iso-8859-9" | "windows-1254" | "latin5" | "cp1254"
    );
    let latin1 = matches!(charset.as_str(), "iso-8859-1" | "latin1" | "windows-1252");
    if !turkish && !latin1 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    bytes
        .iter()
        .map(|&b| match (turkish, b) {
            (true, 0xD0) => 'Ğ',
            (true, 0xDD) => 'İ',
            (true, 0xDE) => 'Ş',
            (true, 0xF0) => 'ğ',
            (true, 0xFD) => 'ı',
            (true, 0xFE) => 'ş',
            _ => char::from(b),
        })
        .collect()
}

/// Arama motorunun öneri cevabı: `["sorgu", ["öneri", ...]]` biçimi;
/// DuckDuckGo'nun eski `[{"phrase": "..."}]` biçimi de okunur.
pub fn parse_remote(body: &str) -> Vec<String> {
    let Ok(serde_json::Value::Array(top)) = serde_json::from_str(body) else {
        return Vec::new();
    };
    let phrases: Vec<String> = match top.get(1) {
        Some(serde_json::Value::Array(list)) => list
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => top
            .iter()
            .filter_map(|v| v.get("phrase")?.as_str().map(String::from))
            .collect(),
    };
    phrases.into_iter().take(10).collect()
}

/// Adres kutusunun otomatik tamamlaması: yazılan metin geçmişteki bir
/// sitenin başıysa ("you") sitenin adına ("youtube.com") tamamlar.
/// Yazılanın büyük/küçük harfi korunur. `/` yazıldıysa adresin tamamına
/// tamamlar.
pub fn completion(text: &str, matches: &[Suggestion]) -> Option<String> {
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let lower = text.to_lowercase();
    matches.iter().find_map(|m| {
        let bare = display_url(&m.url);
        let whole = if lower.contains('/') {
            bare.as_str()
        } else {
            &bare[..bare.find(['/', '?', '#']).unwrap_or(bare.len())]
        };
        let rest = whole
            .to_lowercase()
            .strip_prefix(&lower)
            .filter(|rest| !rest.is_empty())
            .map(|rest| rest.to_string())?;
        // Küçük harfe çevirmek uzunluğu değiştirmediyse kalan kısım aynen alınır.
        let tail = whole.get(whole.len() - rest.len()..)?;
        Some(format!("{text}{tail}"))
    })
}

/// Adresin okunur hâli: `https://`, `www.` ve sondaki `/` olmadan.
pub fn display_url(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    rest.strip_suffix('/').unwrap_or(rest).to_string()
}

fn same_page(a: &str, b: &str) -> bool {
    display_url(a).eq_ignore_ascii_case(&display_url(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(url: &str, title: &str, bookmarked: bool) -> Suggestion {
        Suggestion {
            url: url.into(),
            title: title.into(),
            bookmarked,
        }
    }

    #[test]
    fn first_row_searches_plain_text() {
        let items = build(
            "rust dili",
            None,
            Engine::Google,
            vec![found("https://www.rust-lang.org/", "Rust", true)],
            &[],
        );
        assert_eq!(items[0].kind, Kind::Search);
        assert_eq!(items[0].title, "rust dili");
        assert_eq!(items[0].detail, "Google ile ara");
        assert_eq!(items[0].target, "rust dili");
        assert_eq!(items[1].kind, Kind::Bookmark);
        assert_eq!(items[1].title, "Rust");
        assert_eq!(items[1].detail, "rust-lang.org");
        assert_eq!(items[1].target, "https://www.rust-lang.org/");
        assert_eq!(items[1].icon.as_deref(), Some("rust-lang.org"));
    }

    #[test]
    fn first_row_visits_addresses_and_hides_duplicate() {
        let items = build(
            "youtube.com",
            None,
            Engine::DuckDuckGo,
            vec![
                found("https://www.youtube.com/", "YouTube", false),
                found("https://www.youtube.com/feed", "", false),
            ],
            &[],
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, Kind::Visit);
        assert_eq!(items[0].title, "youtube.com");
        assert_eq!(items[0].icon.as_deref(), Some("youtube.com"));
        assert_eq!(items[1].kind, Kind::History);
        assert_eq!(items[1].title, "youtube.com/feed", "başlık yoksa adres");
    }

    #[test]
    fn limits_matches_and_ignores_empty_text() {
        assert!(build("  ", None, Engine::Google, vec![], &[]).is_empty());
        let many = (0..20)
            .map(|i| found(&format!("https://s{i}.example/"), "", false))
            .collect();
        assert_eq!(
            build("s", None, Engine::Google, many, &[]).len(),
            1 + MAX_MATCHES
        );
    }

    #[test]
    fn display_url_is_short() {
        assert_eq!(display_url("https://www.example.com/"), "example.com");
        assert_eq!(display_url("http://a.b/c?d=1"), "a.b/c?d=1");
    }

    #[test]
    fn mixes_history_and_engine_suggestions() {
        let history = (0..5)
            .map(|i| found(&format!("https://hava{i}.example/"), "", false))
            .collect();
        let remote: Vec<String> = [
            "hava",
            "hava durumu",
            "Hava Durumu",
            "havalimanı",
            "havaist",
            "a",
            "b",
        ]
        .map(String::from)
        .to_vec();
        let items = build("hava", None, Engine::Google, history, &remote);
        let kinds: Vec<_> = items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Search,
                Kind::History,
                Kind::History,
                Kind::History,
                Kind::Search,
                Kind::Search,
                Kind::Search,
                Kind::Search
            ]
        );
        let phrases: Vec<_> = items[4..].iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            phrases,
            ["hava durumu", "havalimanı", "havaist", "a"],
            "tekrarlar atlanır"
        );
        assert_eq!(items[4].target, "hava durumu");
    }

    #[test]
    fn completed_text_decides_first_row() {
        let items = build(
            "you",
            Some("youtube.com"),
            Engine::Google,
            vec![found("https://www.youtube.com/", "YouTube", false)],
            &["you".to_string(), "youtube".to_string()],
        );
        assert_eq!(items[0].kind, Kind::Visit);
        assert_eq!(items[0].target, "youtube.com");
        assert!(items.iter().all(|i| i.target != "https://www.youtube.com/"));
        assert_eq!(items.last().unwrap().title, "youtube");
    }

    #[test]
    fn only_search_like_text_goes_to_the_engine() {
        assert!(may_ask_engine("hava durumu", Engine::Google));
        assert!(may_ask_engine("you", Engine::Google));
        assert!(!may_ask_engine("youtube.com", Engine::Google));
        assert!(!may_ask_engine("localhost:3000", Engine::Google));
        assert!(!may_ask_engine("https://a.b/c", Engine::Google));
        assert!(!may_ask_engine("  ", Engine::Google));
        assert!(!may_ask_engine(&"a".repeat(81), Engine::Google));
    }

    #[test]
    fn parses_engine_answers() {
        assert_eq!(
            parse_remote(r#"["hava",["hava durumu","havaist"],[],{"x":1}]"#),
            ["hava durumu", "havaist"]
        );
        assert_eq!(
            parse_remote(r#"[{"phrase":"a"},{"phrase":"b"}]"#),
            ["a", "b"]
        );
        assert!(parse_remote("<!DOCTYPE html>").is_empty());
        assert!(parse_remote("").is_empty());
    }

    #[test]
    fn completes_to_site_names() {
        let m = vec![
            found("https://www.youtube.com/watch?v=1", "", false),
            found("https://github.com/rust-lang/rust", "", false),
        ];
        assert_eq!(completion("you", &m).as_deref(), Some("youtube.com"));
        assert_eq!(completion("YouT", &m).as_deref(), Some("YouTube.com"));
        assert_eq!(
            completion("github.com/r", &m).as_deref(),
            Some("github.com/rust-lang/rust")
        );
        assert_eq!(completion("youtube.com", &m), None, "zaten tam");
        assert_eq!(completion("tube", &m), None, "yalnızca baştan");
        assert_eq!(completion("you tube", &m), None);
        assert_eq!(completion("", &m), None);
    }

    #[test]
    fn decodes_turkish_code_pages() {
        let bytes = b"[\"hava\",[\"havaliman\xfd\",\"\xddstanbul \xfeehir\"]]";
        assert_eq!(
            parse_remote(&decode_body(bytes, "text/javascript; charset=ISO-8859-9")),
            ["havalimanı", "İstanbul şehir"]
        );
        assert_eq!(
            decode_body("ışık".as_bytes(), "text/javascript; charset=UTF-8"),
            "ışık"
        );
        assert_eq!(decode_body("ışık".as_bytes(), ""), "ışık");
        assert_eq!(
            decode_body(b"caf\xe9", "text/plain; charset=iso-8859-1"),
            "café"
        );
    }
}
