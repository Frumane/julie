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
        // 3: istemci sertifikası, 16: beklenmeyen hata. Edge'in hata sayfası
        // kapalı olduğundan bunlar da boş kalmasın.
        3 | 8 | 15 | 16 => "other",
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
        3 => "BAD_CLIENT_CERTIFICATE",
        15 => "TOO_MANY_REDIRECTS",
        16 => "UNEXPECTED_ERROR",
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

/// Sertifika hatasında Ayrıntılar'da gösterilen bilgiler.
#[derive(Debug, Clone, PartialEq)]
pub struct CertInfo {
    /// Sertifikanın verildiği ad (ör. `*.ornek.com`).
    pub subject: String,
    /// Sertifikayı veren kuruluş.
    pub issuer: String,
    /// Geçerlilik başlangıcı ve sonu (Unix zamanı, saniye).
    pub valid_from: i64,
    pub valid_to: i64,
}

/// Sertifika hata sayfasının adresi; sertifika bilgisi varsa onu da taşır.
pub fn cert_url(status: i32, url: &str, cert: Option<&CertInfo>) -> String {
    let mut out = error_url("cert", status, url);
    if let Some(cert) = cert {
        out.push_str(&format!(
            "&subject={}&issuer={}&from={}&to={}",
            address::encode_query(&cert.subject),
            address::encode_query(&cert.issuer),
            cert.valid_from,
            cert.valid_to,
        ));
    }
    out
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

/// Başlık ve açıklama. Kısa, düz cümleler; korkutmadan ne olduğunu söyler.
fn texts(kind: &str, code: &str, host: &str) -> (&'static str, String) {
    match kind {
        "cert" => (
            "Bu bağlantı güvenli değil.",
            match code {
                "CERT_DATE_INVALID" => format!(
                    "{host} sitesinin güvenlik sertifikasının süresi dolmuş. Bilgisayarının saati yanlışsa da bu olur."
                ),
                "CERT_COMMON_NAME_INVALID" => format!(
                    "Karşıdaki sunucu kendini {host} olarak kanıtlayamadı; sertifikası başka bir siteye ait."
                ),
                "CERT_REVOKED" => format!("{host} sitesinin güvenlik sertifikası iptal edilmiş."),
                _ => format!("{host} sitesinin güvenlik sertifikasına güvenilmiyor."),
            },
        ),
        "insecure" => (
            "Bu site şifresiz.",
            format!(
                "{host} şifreli (HTTPS) açılamadı. Devam edersen gönderdiklerini ve gördüklerini aynı ağdaki biri okuyabilir ya da değiştirebilir."
            ),
        ),
        "blocked" => (
            "Bu sayfa Julie'de açılmaz.",
            "edge:// sayfaları Edge'in kendi ayarları; Julie'de kapalı. Yalnızca edge://gpu açık. Julie'nin ayarları julie://settings adresinde.".to_string(),
        ),
        "game" => (
            "Sinekkuşu.",
            "Boşluk tuşu, yukarı ok ya da tıklamayla kanat çırp; çiçek saplarına çarpmadan aralarından geç.".to_string(),
        ),
        "dns" => (
            "Bu adreste bir site yok.",
            format!("{host} bulunamadı. Adres yanlış yazılmış olabilir ya da site kapanmış."),
        ),
        "offline" => (
            "İnternet yok.",
            "Bağlantı gelince sayfa kendiliğinden açılır. Beklerken boşluk tuşuyla uçabilirsin.".to_string(),
        ),
        "timeout" => (
            "Site cevap vermiyor.",
            format!("{host} çok uzun süre bekletti. Yoğun ya da geçici olarak kapalı olabilir."),
        ),
        "connect" => (
            "Siteye bağlanılamadı.",
            format!("{host} bağlantıyı kabul etmedi. Biraz sonra yeniden dene."),
        ),
        _ => (
            "Sayfa açılamadı.",
            format!("{host} bozuk bir yanıt verdi ya da sayfayı sürekli başka yere yönlendirdi."),
        ),
    }
}

fn page(kind: &str, code: &str, url: &str, shown: &str, cert: Option<CertInfo>) -> String {
    let host = address::exact_host(url).unwrap_or_default();
    let (title, text) = texts(kind, code, &host);
    let cert = cert.map(|c| {
        json!({
            "subject": c.subject,
            "issuer": c.issuer,
            "from": c.valid_from,
            "to": c.valid_to,
        })
    });
    crate::start_page::embed_state(
        include_str!("error_page.html"),
        &json!({
            "kind": kind,
            "url": url,
            "shown": shown,
            "host": host,
            "code": code,
            "title": title,
            "text": text,
            "cert": cert,
        }),
    )
}

/// Sayfanın HTML'i. `query`, adresin `?` sonrası.
pub fn render(query: Option<&str>) -> String {
    let query = query.unwrap_or_default();
    let raw = param(query, "url").unwrap_or_default();
    let web = raw.starts_with("http://") || raw.starts_with("https://");
    let url = if web { raw.clone() } else { String::new() };
    // Engellenen edge:// adresi yalnızca gösterilir, düğmelere verilmez.
    let shown = if web || address::is_engine_page(&raw) {
        raw
    } else {
        String::new()
    };
    let kind = param(query, "kind")
        .filter(|k| k.chars().all(|c| c.is_ascii_lowercase()))
        .unwrap_or_default();
    let code = param(query, "code")
        .filter(|c| c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_'))
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let cert = (kind == "cert").then(|| cert_of(query)).flatten();
    page(&kind, &code, &url, &shown, cert)
}

/// Adresteki sertifika bilgisi; eksik ya da bozuksa `None`.
fn cert_of(query: &str) -> Option<CertInfo> {
    let text = |name| {
        param(query, name)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(200).collect::<String>())
    };
    let time = |name| param(query, name)?.parse::<i64>().ok();
    Some(CertInfo {
        subject: text("subject")?,
        issuer: text("issuer").unwrap_or_default(),
        valid_from: time("from")?,
        valid_to: time("to")?,
    })
}

/// Sinekkuşu oyunu (`julie://fly`): internet yokken çıkan oyunun kendisi.
pub fn render_game() -> String {
    page("game", "", "", "", None)
}

/// HTTPS önce: şifreli açılamayan `http://` site için uyarı.
pub fn render_insecure(url: &str) -> String {
    let url = if url.starts_with("http://") { url } else { "" };
    page("insecure", "HTTPS_UNAVAILABLE", url, url, None)
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
        for status in [0, 14, 17, 18] {
            assert_eq!(kind_for_status(status), None, "{status}");
        }
    }

    #[test]
    fn page_round_trip() {
        let url = error_url("dns", 13, "https://yok.example/a?b=c");
        let html = render(url.split_once('?').map(|(_, q)| q));
        assert!(html.contains(r#""url":"https://yok.example/a?b=c""#));
        assert!(html.contains("Bu adreste bir site yok"));
        assert!(html.contains("NAME_NOT_RESOLVED"));
        assert!(html.contains(r#""kind":"dns""#));

        let cert = error_url("cert", 2, "https://eski.example/");
        let html = render(cert.split_once('?').map(|(_, q)| q));
        assert!(html.contains(r#""kind":"cert""#));
        assert!(html.contains("süresi dolmuş"));
        assert!(html.contains(r#""cert":null"#));

        let info = CertInfo {
            subject: "*.eski.example".into(),
            issuer: "Örnek & Kök <CA>".into(),
            valid_from: 1_428_537_600,
            valid_to: 1_428_796_800,
        };
        let cert = cert_url(2, "https://eski.example/", Some(&info));
        assert_eq!(cert_of(cert.split_once('?').unwrap().1), Some(info));
        let html = render(cert.split_once('?').map(|(_, q)| q));
        assert!(html.contains(r#""subject":"*.eski.example""#));
        assert!(html.contains(r#""to":1428796800"#));
        // Sertifikadaki metin sayfanın betiğinden dışarı taşamaz.
        assert!(!html.contains("<CA>"));
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
