//! Adres çubuğundaki kilit ve site bilgisi paneli: bağlantının güvenliği ve
//! sitenin izinleri (kamera, mikrofon, konum ...).
//!
//! İzinlerin kendisi WebView2 profilinde saklanır; burada yalnızca
//! listelenecek satırlar ve durum geçişleri hesaplanır.

use crate::address;
use crate::ui::layout::{PermState, Security};

/// WebView2'nin izin türleri (`COREWEBVIEW2_PERMISSION_KIND`) ve adları.
/// İlk dördü site hiç istemese de panelde gösterilir.
const KINDS: &[(i32, &str)] = &[
    (2, "Kamera"),
    (1, "Mikrofon"),
    (3, "Konum"),
    (4, "Bildirimler"),
    (9, "Otomatik oynatma"),
    (6, "Pano okuma"),
    (7, "Birden çok indirme"),
    (8, "Dosya düzenleme"),
    (5, "Hareket sensörleri"),
    (10, "Yerel yazı tipleri"),
    (11, "MIDI cihazları"),
    (12, "Pencere yönetimi"),
];
const ALWAYS_SHOWN: usize = 4;

/// WebView2 izin durumu (`COREWEBVIEW2_PERMISSION_STATE`).
pub const STATE_DEFAULT: i32 = 0;
pub const STATE_ALLOW: i32 = 1;
pub const STATE_DENY: i32 = 2;

pub fn state_of(raw: i32) -> PermState {
    match raw {
        STATE_ALLOW => PermState::Allow,
        STATE_DENY => PermState::Block,
        _ => PermState::Ask,
    }
}

pub fn raw_state(state: PermState) -> i32 {
    match state {
        PermState::Allow => STATE_ALLOW,
        PermState::Block => STATE_DENY,
        PermState::Ask => STATE_DEFAULT,
    }
}

/// Tıklanınca bir sonraki durum: Sor → İzin ver → Engelle → Sor.
pub fn next_state(state: PermState) -> PermState {
    match state {
        PermState::Ask => PermState::Allow,
        PermState::Allow => PermState::Block,
        PermState::Block => PermState::Ask,
    }
}

/// Panelde gösterilecek izin satırları: (tür, ad, durum). `set`, sitenin
/// profilde kayıtlı (varsayılan olmayan) izinleri: (tür, durum).
pub fn permission_rows(set: &[(i32, i32)]) -> Vec<(i32, &'static str, PermState)> {
    KINDS
        .iter()
        .enumerate()
        .filter_map(|(i, &(kind, name))| {
            let state = set
                .iter()
                .find(|(k, _)| *k == kind)
                .map_or(PermState::Ask, |(_, s)| state_of(*s));
            (i < ALWAYS_SHOWN || state != PermState::Ask).then_some((kind, name, state))
        })
        .collect()
}

/// İzinlerin anahtarı olan köken: `https://www.example.com:8443`.
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" || address::is_internal_page(url) {
        return None;
    }
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    (!authority.is_empty()).then(|| format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// İki köken aynı mı (sondaki `/` ve büyük/küçük harf önemsiz).
pub fn same_origin(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

/// Adres çubuğundaki simgenin durumu. `cert_bypassed`: kullanıcı bu sitede
/// geçersiz sertifikaya rağmen devam etti.
pub fn security_of(url: &str, cert_bypassed: bool) -> Security {
    match address::internal_page(url) {
        Some("insecure") => return Security::Insecure,
        Some("error") => {
            return if error_kind_of(url) == Some("cert") {
                Security::Danger
            } else {
                Security::None
            };
        }
        Some(_) => return Security::None,
        None => {}
    }
    if url.starts_with("https://") {
        if cert_bypassed {
            Security::Danger
        } else {
            Security::Secure
        }
    } else if url.starts_with("http://") {
        Security::Insecure
    } else {
        Security::None
    }
}

fn query_param<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let query = url.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or_default();
    query.split('&').find_map(|pair| {
        pair.strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

fn error_kind_of(url: &str) -> Option<&str> {
    query_param(url, "kind")
}

/// Uyarı ve hata sayfalarında adres çubuğunda gösterilen (ve "yeniden
/// dene"de açılan) asıl adres; diğer adresler olduğu gibi döner.
pub fn shown_url(url: &str) -> String {
    match address::internal_page(url) {
        Some("insecure" | "error") => query_param(url, "url")
            .map(crate::history_page::percent_decode)
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .unwrap_or_else(|| url.to_string()),
        _ => url.to_string(),
    }
}

/// İki adres aynı sayfa mı (http/https ve sondaki `/` farkı önemsiz):
/// şifreliye çevrilen adres asıl adresle eşleşsin.
pub fn same_page(a: &str, b: &str) -> bool {
    let norm = |u: &str| -> String {
        let u = u
            .strip_prefix("https://")
            .or_else(|| u.strip_prefix("http://"))
            .unwrap_or(u);
        u.trim_end_matches('/').to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_page_ignores_scheme() {
        assert!(same_page("https://a.com/x/", "http://a.com/x"));
        assert!(!same_page("https://a.com/x", "https://a.com/y"));
    }

    #[test]
    fn rows_always_show_common_permissions() {
        let rows = permission_rows(&[]);
        let names: Vec<_> = rows.iter().map(|r| r.1).collect();
        assert_eq!(names, ["Kamera", "Mikrofon", "Konum", "Bildirimler"]);
        assert!(rows.iter().all(|r| r.2 == PermState::Ask));

        let rows = permission_rows(&[(2, STATE_ALLOW), (9, STATE_DENY), (99, STATE_ALLOW)]);
        assert_eq!(rows[0], (2, "Kamera", PermState::Allow));
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[4], (9, "Otomatik oynatma", PermState::Block));
    }

    #[test]
    fn states_cycle_and_round_trip() {
        let mut s = PermState::Ask;
        for _ in 0..3 {
            assert_eq!(state_of(raw_state(s)), s);
            s = next_state(s);
        }
        assert_eq!(s, PermState::Ask);
    }

    #[test]
    fn origins() {
        assert_eq!(
            origin_of("https://WWW.Example.com:8443/a?b#c").as_deref(),
            Some("https://www.example.com:8443")
        );
        assert_eq!(origin_of("http://julie.start/"), None);
        assert_eq!(origin_of("about:blank"), None);
        assert!(same_origin("https://a.com/", "https://A.com"));
    }

    #[test]
    fn security_states() {
        assert_eq!(security_of("https://a.com/", false), Security::Secure);
        assert_eq!(security_of("https://a.com/", true), Security::Danger);
        assert_eq!(security_of("http://a.com/", false), Security::Insecure);
        assert_eq!(security_of("http://julie.start/", false), Security::None);
        assert_eq!(
            security_of("http://julie.insecure/?url=http%3A%2F%2Fa.com", false),
            Security::Insecure
        );
        assert_eq!(
            security_of("http://julie.error/?kind=cert&url=x", false),
            Security::Danger
        );
        assert_eq!(
            security_of("http://julie.error/?kind=dns&url=x", false),
            Security::None
        );
    }

    #[test]
    fn shown_url_unwraps_error_pages() {
        assert_eq!(
            shown_url("http://julie.error/?kind=dns&url=https%3A%2F%2Fa.com%2Fx"),
            "https://a.com/x"
        );
        assert_eq!(
            shown_url("http://julie.insecure/?url=http%3A%2F%2Fa.com"),
            "http://a.com"
        );
        assert_eq!(shown_url("https://b.com/"), "https://b.com/");
        // Adres olmayan bir değer gösterilmez.
        assert_eq!(
            shown_url("http://julie.error/?url=javascript%3Aalert(1)"),
            "http://julie.error/?url=javascript%3Aalert(1)"
        );
    }
}
