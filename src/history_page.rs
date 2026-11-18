//! Geçmiş sayfası (`julie://history`).

use serde_json::json;

use crate::places::Entry;
use crate::start_page::embed_state;

const TEMPLATE: &str = include_str!("history_page.html");

/// Sayfada gösterilen en fazla kayıt sayısı.
pub const LIMIT: usize = 1000;

/// Sayfanın HTML'ini geçmiş kayıtlarıyla birlikte üretir.
pub fn render(query: &str, entries: &[Entry]) -> String {
    let entries: Vec<_> = entries
        .iter()
        .map(|e| json!({ "url": e.url, "title": e.title, "time": e.time }))
        .collect();
    embed_state(TEMPLATE, &json!({ "query": query, "entries": entries }))
}

/// Adresteki `?q=` arama sorgusunu çözer.
pub fn query_from(query_string: Option<&str>) -> String {
    let Some(qs) = query_string else {
        return String::new();
    };
    qs.split('&')
        .find_map(|pair| pair.strip_prefix("q="))
        .map(percent_decode)
        .unwrap_or_default()
}

pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            ) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_search_queries() {
        assert_eq!(query_from(None), "");
        assert_eq!(query_from(Some("q=rust+dili")), "rust dili");
        assert_eq!(query_from(Some("x=1&q=%C3%A7ay%20saati")), "çay saati");
        assert_eq!(query_from(Some("q=100%")), "100%");
        assert_eq!(query_from(Some("q=%zz")), "%zz");
        assert_eq!(query_from(Some("q=%ç")), "%ç");
    }

    #[test]
    fn renders_entries_safely() {
        let entries = [Entry {
            url: "https://a.example/".into(),
            title: "</script><img src=x onerror=alert(1)>".into(),
            time: 1_700_000_000,
        }];
        let html = render("ara</script>", &entries);
        assert!(!html.contains("/*STATE*/"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains(r#""time":1700000000"#));
    }
}
