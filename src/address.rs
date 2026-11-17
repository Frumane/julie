//! Adres çubuğuna yazılan metni gidilecek bir URL'ye çevirir.

/// Tarayıcıya gömülü başlangıç sayfasının adresi. WebView2'de özel
/// protokoller `http://<protokol>.<yol>` biçiminde görünür; `julie://start`
/// burada `http://julie.start/` olur ve istek ağa çıkmadan bizde karşılanır.
pub const START_URL: &str = "http://julie.start/";

/// Adres başlangıç sayfasına mı ait?
pub fn is_start_page(url: &str) -> bool {
    url.strip_prefix(START_URL.trim_end_matches('/'))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
}

/// Kullanıcının yazdığını bir URL'ye çevirir:
/// - şeması olan adresler (`https://`, `about:` ...) olduğu gibi kalır,
/// - alan adına benzeyenlerin başına `https://` eklenir,
/// - geri kalan her şey `search_prefix` ile aranır.
pub fn resolve(input: &str, search_prefix: &str) -> String {
    let text = input.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("julie://start") {
        return START_URL.to_string();
    }
    if has_scheme(text) {
        return text.to_string();
    }
    if !text.contains(char::is_whitespace) {
        if is_localhost(text) {
            return format!("http://{text}");
        }
        if looks_like_host(text) {
            return format!("https://{text}");
        }
    }
    format!("{search_prefix}{}", encode_query(text))
}

fn has_scheme(text: &str) -> bool {
    if let Some((scheme, rest)) = text.split_once("://") {
        return !scheme.is_empty()
            && !rest.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    }
    let lower = text.to_ascii_lowercase();
    ["about:", "data:", "javascript:", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

fn host_part(text: &str) -> &str {
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    &text[..end]
}

fn is_localhost(text: &str) -> bool {
    let host = host_part(text);
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1"
}

fn looks_like_host(text: &str) -> bool {
    let host = host_part(text);
    // Sondaki port kısmını at (ör. "example.com:8080").
    let name = match host.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
        && labels
            .last()
            .is_some_and(|tld| !tld.chars().all(|c| c.is_ascii_digit()) || labels.len() == 4)
}

fn encode_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = "https://duckduckgo.com/?q=";

    fn resolve(input: &str) -> String {
        super::resolve(input, SEARCH)
    }

    #[test]
    fn keeps_full_urls() {
        assert_eq!(
            resolve("https://example.com/a?b=c"),
            "https://example.com/a?b=c"
        );
        assert_eq!(resolve("about:blank"), "about:blank");
        assert_eq!(resolve("file:///C:/test.html"), "file:///C:/test.html");
    }

    #[test]
    fn adds_https_to_hosts() {
        assert_eq!(resolve("example.com"), "https://example.com");
        assert_eq!(
            resolve("  github.com/rust-lang  "),
            "https://github.com/rust-lang"
        );
        assert_eq!(resolve("türkiye.gov.tr"), "https://türkiye.gov.tr");
        assert_eq!(resolve("example.com:8080/x"), "https://example.com:8080/x");
        assert_eq!(resolve("192.168.1.1"), "https://192.168.1.1");
    }

    #[test]
    fn localhost_uses_http() {
        assert_eq!(resolve("localhost:3000"), "http://localhost:3000");
        assert_eq!(resolve("127.0.0.1:8080/api"), "http://127.0.0.1:8080/api");
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(resolve("rust dili"), "https://duckduckgo.com/?q=rust+dili");
        assert_eq!(resolve("merhaba"), "https://duckduckgo.com/?q=merhaba");
        assert_eq!(resolve("1.5"), "https://duckduckgo.com/?q=1.5");
        assert_eq!(resolve("ç?"), "https://duckduckgo.com/?q=%C3%A7%3F");
    }

    #[test]
    fn empty_goes_to_start_page() {
        assert_eq!(resolve("   "), START_URL);
        assert_eq!(resolve("julie://start"), START_URL);
    }

    #[test]
    fn uses_the_given_engine() {
        assert_eq!(
            super::resolve("rust", "https://www.google.com/search?q="),
            "https://www.google.com/search?q=rust"
        );
    }

    #[test]
    fn detects_start_page() {
        assert!(is_start_page("http://julie.start/"));
        assert!(is_start_page("http://julie.start"));
        assert!(is_start_page("http://julie.start/?x=1"));
        assert!(!is_start_page("http://julie.starter.com/"));
        assert!(!is_start_page("https://duckduckgo.com/"));
    }
}
