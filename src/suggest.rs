//! Adres çubuğu önerileri: yazılan metinden ve geçmişten satırlar üretir.
//!
//! İlk satır her zaman Enter'a basınca olacak şeydir (siteye git ya da ara).
//! Altında geçmişten ve yer imlerinden eşleşen siteler gelir.

use crate::address;
use crate::places::Suggestion;
use crate::settings::Engine;

/// Gösterilecek en fazla geçmiş/yer imi satırı (ilk satır hariç).
pub const MAX_MATCHES: usize = 6;

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

/// Yazılan metin ve geçmişteki eşleşmelerden öneri satırlarını kurar.
pub fn build(text: &str, engine: Engine, matches: Vec<Suggestion>) -> Vec<Item> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
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
    for m in matches {
        if items.len() > MAX_MATCHES {
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
    items
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
            Engine::Google,
            vec![found("https://www.rust-lang.org/", "Rust", true)],
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
            Engine::DuckDuckGo,
            vec![
                found("https://www.youtube.com/", "YouTube", false),
                found("https://www.youtube.com/feed", "", false),
            ],
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
        assert!(build("  ", Engine::Google, vec![]).is_empty());
        let many = (0..20)
            .map(|i| found(&format!("https://s{i}.example/"), "", false))
            .collect();
        assert_eq!(build("s", Engine::Google, many).len(), 1 + MAX_MATCHES);
    }

    #[test]
    fn display_url_is_short() {
        assert_eq!(display_url("https://www.example.com/"), "example.com");
        assert_eq!(display_url("http://a.b/c?d=1"), "a.b/c?d=1");
    }
}
