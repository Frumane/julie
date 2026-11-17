//! Adres çubuğuna yazılan metni gidilecek bir URL'ye çevirir.

/// Tarayıcıya gömülü başlangıç sayfasının adresi. WebView2'de özel
/// protokoller `http://<protokol>.<yol>` biçiminde görünür; `julie://start`
/// burada `http://julie.start/` olur ve istek ağa çıkmadan bizde karşılanır.
pub const START_URL: &str = "http://julie.start/";

/// Geçmiş sayfası (`julie://history`).
pub const HISTORY_URL: &str = "http://julie.history/";

/// Ayarlar sayfası (`julie://settings`).
pub const SETTINGS_URL: &str = "http://julie.settings/";

/// Tarayıcının kendi sayfaları (`julie://<ad>`).
const INTERNAL_PAGES: [&str; 3] = ["start", "history", "settings"];

/// Tarayıcının kendi sayfasının adı (`start`, `history`); değilse `None`.
pub fn internal_page(url: &str) -> Option<&'static str> {
    let rest = url.strip_prefix("http://julie.")?;
    let name = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    INTERNAL_PAGES
        .into_iter()
        .find(|p| p.eq_ignore_ascii_case(name))
}

/// Adres tarayıcının kendi sayfalarından birine mi ait?
pub fn is_internal_page(url: &str) -> bool {
    internal_page(url).is_some()
}

/// Adres başlangıç sayfasına mı ait?
pub fn is_start_page(url: &str) -> bool {
    url.strip_prefix(START_URL.trim_end_matches('/'))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
}

/// Adresin alan adını küçük harfle ve başındaki `www.` olmadan verir.
/// `https://www.Example.com:8080/a` → `example.com`
pub fn host_of(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    (!host.is_empty()).then(|| host.to_string())
}

/// Adresin tam alan adı (küçük harf, `www.` dahil, port olmadan). Yalnızca
/// http(s) adresleri ve yalnızca alan adı karakterlerinden oluşan adlar için döner.
pub fn exact_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host
        .rsplit_once(':')
        .map_or(host, |(h, _)| h)
        .to_ascii_lowercase();
    let valid = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(host)
}

/// Kullanıcının yazdığını bir URL'ye çevirir:
/// - şeması olan adresler (`https://`, `about:` ...) olduğu gibi kalır,
/// - alan adına benzeyenlerin başına `https://` eklenir,
/// - geri kalan her şey `search_prefix` ile aranır.
pub fn resolve(input: &str, search_prefix: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return START_URL.to_string();
    }
    // `julie://history` → `http://julie.history/`
    if let Some(name) = text
        .get(..8)
        .filter(|p| p.eq_ignore_ascii_case("julie://"))
        .and_then(|_| {
            INTERNAL_PAGES
                .into_iter()
                .find(|p| text[8..].trim_end_matches('/').eq_ignore_ascii_case(p))
        })
    {
        return format!("http://julie.{name}/");
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
        assert_eq!(resolve("julie://history"), HISTORY_URL);
        assert_eq!(resolve("JULIE://History/"), HISTORY_URL);
        assert_eq!(resolve("julie://settings"), SETTINGS_URL);
    }

    #[test]
    fn uses_the_given_engine() {
        assert_eq!(
            super::resolve("rust", "https://www.google.com/search?q="),
            "https://www.google.com/search?q=rust"
        );
    }

    #[test]
    fn exact_hosts() {
        assert_eq!(
            exact_host("https://www.YouTube.com/watch?v=1").as_deref(),
            Some("www.youtube.com")
        );
        assert_eq!(
            exact_host("http://localhost:8080/").as_deref(),
            Some("localhost")
        );
        assert_eq!(exact_host("about:blank"), None);
        assert_eq!(exact_host("file:///C:/a.html"), None);
        assert_eq!(exact_host("https://ex\"ample.com/"), None);
    }

    #[test]
    fn extracts_hosts() {
        assert_eq!(
            host_of("https://www.Example.com:8080/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            host_of("http://user@site.org/").as_deref(),
            Some("site.org")
        );
        assert_eq!(
            host_of("https://sub.site.org").as_deref(),
            Some("sub.site.org")
        );
        assert_eq!(host_of("about:blank"), None);
        assert_eq!(host_of("https:///x"), None);
    }

    #[test]
    fn detects_internal_pages() {
        assert_eq!(internal_page("http://julie.start/"), Some("start"));
        assert_eq!(
            internal_page("http://julie.history/?q=rust"),
            Some("history")
        );
        assert_eq!(internal_page("http://julie.historyx/"), None);
        assert_eq!(internal_page("https://julie.history/"), None);
        assert_eq!(internal_page("http://julie.example.com/"), None);
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
