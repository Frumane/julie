//! Sayfa açılamayınca gösterilen hata sayfası (`julie://error`): site
//! bulunamadı, bağlantı yok, zaman aşımı, geçersiz sertifika.
//!
//! WebView2'nin kendi hata sayfası yerine temaya uyan, Türkçe bir sayfa.
//! Adres `http://julie.error/?kind=<tür>&code=<kod>&url=<asıl adres>`.

use serde_json::json;

use crate::address;

/// Sayfanın türü ve WebView2 hata durumu (`COREWEBVIEW2_WEB_ERROR_STATUS`).
/// Listede olmayan durumlar (kullanıcının durdurduğu yüklemeler, indirmeler,
/// HTTP 404 gibi sitenin kendi hata sayfaları) için `None`: dokunulmaz.
pub fn kind_for_status(status: i32) -> Option<&'static str> {
    Some(match status {
        1 | 2 | 4 | 5 => "cert",
        13 => "dns",
        11 => "offline",
        7 => "timeout",
        6 | 9 | 10 | 12 => "connect",
        8 | 15 => "other",
        _ => return None,
    })
}

/// Hata durumunun kısa adı (sayfanın "Ayrıntılar" bölümünde gösterilir).
pub fn code_for_status(status: i32) -> &'static str {
    match status {
        1 => "CERT_COMMON_NAME_INVALID",
        2 => "CERT_DATE_INVALID",
        4 => "CERT_REVOKED",
        5 => "CERT_INVALID",
        6 => "SERVER_UNREACHABLE",
        7 => "TIMED_OUT",
        8 => "INVALID_RESPONSE",
        9 => "CONNECTION_ABORTED",
        10 => "CONNECTION_RESET",
        11 => "INTERNET_DISCONNECTED",
        12 => "CONNECTION_REFUSED",
        13 => "NAME_NOT_RESOLVED",
        15 => "TOO_MANY_REDIRECTS",
        _ => "UNKNOWN",
    }
}

pub fn error_url(kind: &str, status: i32, url: &str) -> String {
    format!(
        "http://julie.error/?kind={kind}&code={}&url={}",
        code_for_status(status),
        address::encode_query(url)
    )
}

/// Engellenen Edge iç sayfası (`edge://...`) için hata sayfası.
pub fn blocked_url(url: &str) -> String {
    format!(
        "http://julie.error/?kind=blocked&code=BLOCKED_ENGINE_PAGE&url={}",
        address::encode_query(url)
    )
}

fn param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        pair.strip_prefix(name)
            .and_then(|r| r.strip_prefix('='))
            .map(crate::history_page::percent_decode)
    })
}

/// Başlık ve açıklama.
fn texts(kind: &str, code: &str, host: &str) -> (&'static str, String) {
    match kind {
        "cert" => (
            "Bağlantın güvenli değil",
            match code {
                "CERT_DATE_INVALID" => format!(
                    "{host} sitesinin güvenlik sertifikasının süresi dolmuş ya da bilgisayarının saati yanlış. Saldırganlar şifre, mesaj ya da kart bilgilerini çalmaya çalışıyor olabilir."
                ),
                "CERT_COMMON_NAME_INVALID" => format!(
                    "Sunucu kendini {host} olarak tanıtamadı; sertifikası başka bir siteye ait. Biri bu siteyi taklit ediyor olabilir."
                ),
                "CERT_REVOKED" => format!(
                    "{host} sitesinin güvenlik sertifikası iptal edilmiş. Saldırganlar bilgilerini çalmaya çalışıyor olabilir."
                ),
                _ => format!(
                    "{host} sitesinin güvenlik sertifikasına güvenilmiyor. Saldırganlar şifre, mesaj ya da kart bilgilerini çalmaya çalışıyor olabilir."
                ),
            },
        ),
        "blocked" => (
            "Bu sayfa Julie'de açılmaz",
            "edge:// ve chrome:// ile başlayan sayfalar Edge'in kendi ayar sayfalarıdır; Julie'de kapalıdır. Julie'nin ayarları için julie://settings adresini aç. Yalnızca edge://gpu (ekran kartı bilgisi) açıktır.".to_string(),
        ),
        "dns" => (
            "Bu site bulunamadı",
            format!("{host} adresinin sunucusu bulunamadı. Adresi doğru yazdığından emin ol."),
        ),
        "offline" => (
            "İnternet bağlantısı yok",
            "Wi-Fi'ye ya da kabloya bağlı olduğundan emin ol, sonra yeniden dene.".to_string(),
        ),
        "timeout" => (
            "Site yanıt vermedi",
            format!(
                "{host} çok uzun süre yanıt vermedi. Site yoğun ya da geçici olarak kapalı olabilir."
            ),
        ),
        "connect" => (
            "Siteye bağlanılamadı",
            format!(
                "{host} bağlantıyı reddetti ya da kesti. Site kapalı olabilir; bir süre sonra yeniden dene."
            ),
        ),
        _ => (
            "Sayfa açılamadı",
            format!("{host} geçersiz bir yanıt verdi ya da çok fazla yönlendirme yaptı."),
        ),
    }
}

/// Sayfanın HTML'i. `query`, adresin `?` sonrası.
pub fn render(query: Option<&str>) -> String {
    let query = query.unwrap_or_default();
    let url = param(query, "url")
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .unwrap_or_default();
    let kind = param(query, "kind").unwrap_or_default();
    let code = param(query, "code")
        .filter(|c| c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_'))
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let host = address::exact_host(&url).unwrap_or_default();
    let (title, text) = texts(&kind, &code, &host);
    crate::start_page::embed_state(
        include_str!("error_page.html"),
        &json!({
            "url": url,
            "host": host,
            "code": code,
            "title": title,
            "text": text,
            "cert": kind == "cert",
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_kinds() {
        assert_eq!(kind_for_status(13), Some("dns"));
        assert_eq!(kind_for_status(2), Some("cert"));
        assert_eq!(kind_for_status(11), Some("offline"));
        // Durdurulan yükleme, bilinmeyen durum (HTTP hataları, indirmeler)
        // ve kimlik doğrulama isteyen sayfalara dokunulmaz.
        for status in [0, 14, 16, 17, 18, 3] {
            assert_eq!(kind_for_status(status), None, "{status}");
        }
    }

    #[test]
    fn page_round_trip() {
        let url = error_url("dns", 13, "https://yok.example/a?b=c");
        let html = render(url.split_once('?').map(|(_, q)| q));
        assert!(html.contains(r#""url":"https://yok.example/a?b=c""#));
        assert!(html.contains("Bu site bulunamadı"));
        assert!(html.contains("NAME_NOT_RESOLVED"));
        assert!(html.contains(r#""cert":false"#));

        let cert = error_url("cert", 2, "https://eski.example/");
        let html = render(cert.split_once('?').map(|(_, q)| q));
        assert!(html.contains(r#""cert":true"#));
        assert!(html.contains("süresi dolmuş"));
    }

    #[test]
    fn blocked_engine_page() {
        let url = blocked_url("edge://gpu");
        let html = render(url.split_once('?').map(|(_, q)| q));
        assert!(html.contains("Julie'de açılmaz"));
        // "Yeniden dene" düğmesi çıkmasın: adres yine engellenir.
        assert!(html.contains(r#""url":"""#));
    }

    #[test]
    fn rejects_unsafe_values() {
        let html = render(Some("kind=dns&code=<b>&url=javascript:alert(1)"));
        assert!(html.contains(r#""url":"""#));
        assert!(html.contains(r#""code":"UNKNOWN""#));
    }
}
