//! Okuma modu: sayfanın asıl yazısını reklamsız, sade bir sayfada gösterir.
//!
//! Yazıyı Mozilla'nın Readability kütüphanesi (Firefox'un okuma modu) bulur;
//! sayfada çalışır ve sonucu tarayıcıya verir. Tarayıcı yazıyı bellekte
//! tutar ve `julie://reader` sayfasında gösterir. Sitenin yazısı
//! güvenilmez sayılır: sayfa yalnızca izin verilen öğeleri kopyalar,
//! betik çalıştırmaz (CSP yalnızca kendi betiğine izin verir) ve
//! tarayıcıya komut gönderemez.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

/// Okuma modu sayfasının adresi (`julie://reader`).
pub const READER_URL: &str = "http://julie.reader/";

const READABILITY: &str = include_str!("vendor/readability/Readability.js");
const READERABLE: &str = include_str!("vendor/readability/Readability-readerable.js");
const PAGE: &str = include_str!("reader_page.html");

/// Bellekte tutulan en fazla yazı (en eskisi atılır).
const MAX_ARTICLES: usize = 16;
/// Bir yazının en fazla boyutu (bayt).
const MAX_CONTENT: usize = 4 * 1024 * 1024;

/// Sayfa okuma moduna uygun mu? Sonuç `true`/`false`.
pub fn detect_script() -> String {
    format!(
        "(() => {{ try {{ {READERABLE}\n return isProbablyReaderable(document); }} catch (e) {{ return false; }} }})()"
    )
}

/// Yazıyı çıkaran betik. Sonuç bir nesne ya da `null`.
pub fn extract_script() -> String {
    format!(
        "(() => {{ try {{ {READABILITY}\n\
         const a = new Readability(document.cloneNode(true)).parse();\n\
         if (!a || !a.content) return null;\n\
         return {{ title: a.title, byline: a.byline, siteName: a.siteName, content: a.content,\n\
           lang: a.lang, dir: a.dir, length: a.length, url: location.href }};\n\
         }} catch (e) {{ return null; }} }})()"
    )
}

/// Okuma moduna alınan yazı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    pub url: String,
    pub title: String,
    pub byline: String,
    pub site_name: String,
    pub content: String,
    pub lang: String,
    pub dir: String,
    pub length: u64,
}

impl Article {
    /// Betiğin sonucunu (JSON) okur. Adres sayfanın kendi adresi olmalı
    /// (`expected_url`); yalnızca http(s) yazılar kabul edilir.
    pub fn parse(json: &str, expected_url: &str) -> Option<Article> {
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let text = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let url = text("url");
        let content = text("content");
        if content.is_empty() || content.len() > MAX_CONTENT || !is_web(&url) {
            return None;
        }
        // Sayfa yazıyı çıkarırken başka adrese geçtiyse kabul etme.
        if strip_fragment(&url) != strip_fragment(expected_url) {
            return None;
        }
        let dir = text("dir");
        Some(Article {
            url,
            title: text("title"),
            byline: text("byline"),
            site_name: text("siteName"),
            content,
            lang: text("lang"),
            dir: if dir == "rtl" { dir } else { String::new() },
            length: value.get("length").and_then(|v| v.as_u64()).unwrap_or(0),
        })
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "url": self.url,
            "title": self.title,
            "byline": self.byline,
            "siteName": self.site_name,
            "content": self.content,
            "lang": self.lang,
            "dir": self.dir,
            "length": self.length,
        })
    }
}

fn is_web(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn strip_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

/// Bellekteki yazılar.
#[derive(Debug, Default)]
pub struct Store {
    next: u64,
    articles: HashMap<u64, Article>,
}

impl Store {
    /// Yazıyı saklar; sayfanın adresini döndürür.
    pub fn add(&mut self, article: Article) -> String {
        self.next += 1;
        let key = self.next;
        let url = page_url(key, &article.url);
        self.articles.insert(key, article);
        while self.articles.len() > MAX_ARTICLES {
            let oldest = *self.articles.keys().min().expect("boş değil");
            self.articles.remove(&oldest);
        }
        url
    }

    pub fn get(&self, key: u64) -> Option<&Article> {
        self.articles.get(&key)
    }
}

/// `julie://reader/?id=3&url=…`: yazı bellekte yoksa (Julie yeniden
/// başladıysa) `url` ile sayfaya dönülebilir.
pub fn page_url(key: u64, original: &str) -> String {
    format!("{READER_URL}?id={key}&url={}", encode(original))
}

/// Okuma modu adresindeki asıl sayfanın adresi.
pub fn original_url(reader_url: &str) -> Option<String> {
    let query = reader_url.strip_prefix(READER_URL)?.strip_prefix('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("url="))
        .map(decode)
        .filter(|u| is_web(u))
}

fn key_of(query: Option<&str>) -> Option<u64> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))?
        .parse()
        .ok()
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Sayfanın HTML'i ve CSP'si. `query`: adresin `?` sonrası.
pub fn render(query: Option<&str>, store: &Store) -> (String, String) {
    let data = match key_of(query).and_then(|k| store.get(k)) {
        Some(article) => article.json(),
        None => {
            // Yazı artık yok: yalnızca sayfaya dönüş bağlantısı.
            let url = query
                .map(|q| format!("{READER_URL}?{q}"))
                .and_then(|u| original_url(&u));
            serde_json::json!({ "url": url })
        }
    };
    // `<` kaçırılır: yazının içindeki "</script>" betiği kapatamaz.
    let data = data.to_string().replace('<', "\\u003c");
    // Windows'ta git satır sonlarını CRLF yapabilir; tarayıcı betiğin özetini
    // LF'ye çevirip hesaplar. Aynı olsun diye sayfa LF'ye çevrilir.
    let html = PAGE
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replacen("/*ARTICLE*/null", &data, 1);
    let csp = content_security_policy(&html);
    (html, csp)
}

/// Yalnızca sayfanın kendi betiğine izin veren CSP (betiğin özeti ile).
/// Satır içi olay nitelikleri (`onerror=` …) de böylece çalışmaz.
fn content_security_policy(html: &str) -> String {
    use base64::Engine as _;
    let script = html
        .split_once("<script>")
        .and_then(|(_, rest)| rest.split_once("</script>"))
        .map(|(s, _)| s)
        .unwrap_or_default();
    let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(script.as_bytes()));
    format!(
        "default-src 'none'; script-src 'sha256-{hash}'; style-src 'unsafe-inline'; \
         img-src https: http: data:; media-src https: http:; base-uri 'none'; form-action 'none'; \
         frame-ancestors 'none'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(url: &str) -> Article {
        Article::parse(
            &serde_json::json!({
                "title": " Başlık ", "byline": "Yazar", "siteName": "Site",
                "content": "<p>Merhaba</p>", "lang": "tr", "dir": "ltr",
                "length": 1234, "url": url,
            })
            .to_string(),
            url,
        )
        .unwrap()
    }

    #[test]
    fn parses_article() {
        let a = article("https://example.com/yazi");
        assert_eq!(a.title, "Başlık");
        assert_eq!(a.dir, "");
        assert_eq!(a.length, 1234);
        let json = r#"{"content":"<p>x</p>","url":"https://a.com/"}"#;
        // Adres değiştiyse ya da http(s) değilse reddedilir.
        assert!(Article::parse(json, "https://b.com/").is_none());
        assert!(Article::parse(json, "https://a.com/#bolum").is_some());
        let file = r#"{"content":"<p>x</p>","url":"file:///c:/a.html"}"#;
        assert!(Article::parse(file, "file:///c:/a.html").is_none());
        assert!(Article::parse("null", "https://a.com/").is_none());
        assert!(
            Article::parse(r#"{"content":"","url":"https://a.com/"}"#, "https://a.com/").is_none()
        );
    }

    #[test]
    fn urls() {
        let mut store = Store::default();
        let url = store.add(article("https://example.com/a?b=1&c=ç"));
        assert_eq!(
            url,
            "http://julie.reader/?id=1&url=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1%26c%3D%C3%A7"
        );
        assert_eq!(
            original_url(&url).as_deref(),
            Some("https://example.com/a?b=1&c=ç")
        );
        assert_eq!(
            original_url("http://julie.reader/?id=1&url=javascript%3Aalert(1)"),
            None
        );
        assert_eq!(key_of(Some("id=7&url=x")), Some(7));
        for _ in 0..MAX_ARTICLES + 3 {
            store.add(article("https://example.com/"));
        }
        assert!(store.get(1).is_none());
        assert_eq!(store.articles.len(), MAX_ARTICLES);
    }

    #[test]
    fn page_is_locked_down() {
        let mut store = Store::default();
        let mut a = article("https://example.com/");
        a.content = "<p>a</p></script><script>alert(1)</script>".into();
        a.title = "<img src=x onerror=alert(1)>".into();
        store.add(a);
        let (html, csp) = render(Some("id=1"), &store);
        // Yazının içindeki </script> betiği kapatmaz.
        assert_eq!(html.matches("</script>").count(), 1);
        assert!(!html.contains("<img src=x"));
        assert!(csp.contains("script-src 'sha256-"));
        // Özet sayfadaki betikle aynı.
        let script = html
            .split_once("<script>")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        use base64::Engine as _;
        let hash =
            base64::engine::general_purpose::STANDARD.encode(Sha256::digest(script.as_bytes()));
        assert!(csp.contains(&hash));
        // Yazı yoksa yalnızca dönüş adresi.
        let (html, _) = render(Some("id=99&url=https%3A%2F%2Fa.com%2F"), &store);
        assert!(html.contains(r#"const ARTICLE = {"url":"https://a.com/"};"#));
    }

    #[test]
    fn page_has_unix_line_endings() {
        let (html, _) = render(None, &Store::default());
        assert!(!html.contains('\r'));
    }

    #[test]
    fn scripts_wrap_the_library() {
        assert!(detect_script().contains("isProbablyReaderable(document)"));
        assert!(extract_script().contains("new Readability("));
    }
}
