//! Sayfaya gitmeden önce adrese uygulanan korumalar.
//!
//! - **HTTPS önce:** `http://` adresleri önce şifreli (`https://`) açılmaya
//!   çalışılır. Site şifreli bağlantı desteklemiyorsa kullanıcıya bir uyarı
//!   sayfası gösterilir; "yine de devam et" derse o site bu oturumda şifresiz
//!   açılır. (LibreWolf'un "yalnızca HTTPS" kipi, Brave'in HTTPS yükseltmesi.)
//! - **İzleme parametresi temizleme:** Bağlantılara reklam ve sosyal medya
//!   şirketlerinin eklediği `utm_source`, `fbclid`, `gclid` gibi takip
//!   parametreleri adresten çıkarılır. (Brave ve Firefox'un katı kipi.)

use std::collections::{HashMap, HashSet};

use crate::address;

/// Brave ve Firefox'un listelerinden derlenen, yalnızca takip için kullanılan
/// parametreler. `utm_` ile başlayanların hepsi ayrıca temizlenir.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "yclid",
    "twclid",
    "ttclid",
    "li_fat_id",
    "igshid",
    "mc_eid",
    "mkt_tok",
    "_hsenc",
    "_hsmi",
    "__hssc",
    "__hstc",
    "__hsfp",
    "hsctatracking",
    "_openstat",
    "oly_anon_id",
    "oly_enc_id",
    "rb_clickid",
    "vero_conv",
    "vero_id",
    "wickedid",
    "s_cid",
    "ml_subscriber",
    "ml_subscriber_hash",
    "epik",
];

fn is_tracking_param(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("utm_") || TRACKING_PARAMS.contains(&name.as_str())
}

/// Adresten takip parametrelerini çıkarır. Çıkarılacak bir şey yoksa `None`.
pub fn strip_tracking(url: &str) -> Option<String> {
    let (before_fragment, fragment) = match url.find('#') {
        Some(i) => url.split_at(i),
        None => (url, ""),
    };
    let (base, query) = before_fragment.split_once('?')?;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let name = pair.split('=').next().unwrap_or_default();
            !is_tracking_param(name)
        })
        .collect();
    if kept.len() == query.split('&').count() {
        return None;
    }
    let kept: Vec<&str> = kept.into_iter().filter(|p| !p.is_empty()).collect();
    let query = if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    };
    Some(format!("{base}{query}{fragment}"))
}

/// Bu ad için şifreli bağlantı denemeye değer mi? Yerel ağdaki cihazlar
/// (modem arayüzü, `localhost`, IP adresleri) çoğunlukla yalnızca `http`
/// konuşur; onlara dokunulmaz.
fn upgradable(host: &str) -> bool {
    const LOCAL_SUFFIXES: [&str; 6] = [
        ".localhost",
        ".local",
        ".lan",
        ".home",
        ".internal",
        ".home.arpa",
    ];
    if host.is_empty() || host == "localhost" || !host.contains('.') {
        return false;
    }
    if host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok() {
        return false;
    }
    !LOCAL_SUFFIXES.iter().any(|s| host.ends_with(s))
}

/// Bir gezintiye ne yapılacağı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Olduğu gibi git.
    Allow,
    /// Bu gezintiyi durdurup şu adrese git.
    Redirect(String),
    /// Site şifreli açılamıyor: uyarı sayfasını göster (şifresiz adres).
    Warn(String),
}

/// Ayarlar ve oturum boyunca tutulan durum (bütün sekmeler paylaşır).
#[derive(Debug, Default)]
pub struct Shield {
    pub https_first: bool,
    pub strip_tracking: bool,
    /// Kullanıcının "yine de devam et" dediği siteler (yalnızca bu oturum).
    allowed_http: HashSet<String>,
    /// Sekmede şifreliye çevrilen ve sonucu beklenen gezinti:
    /// (site, asıl şifresiz adres).
    pending: HashMap<u64, (String, String)>,
    /// Sekmede en son temizlenen adres. Site temizlenmiş adresi yeniden
    /// parametreli adrese yönlendirirse ikinci kez temizlenmez (döngü olmasın).
    stripped: HashMap<u64, String>,
    /// Geçersiz sertifikaya rağmen "yine de devam et" denen siteler
    /// (yalnızca bu oturum).
    allowed_cert: HashSet<String>,
    /// Sekmede en son başlayan gezintinin adresi (hata sayfası için).
    started: HashMap<u64, String>,
}

/// Sertifika hatasında ne yapılacağı.
#[derive(Debug, PartialEq, Eq)]
pub enum CertAction {
    /// Kullanıcı bu sitede devam etmeyi seçmişti.
    Allow,
    /// Şifreliye çevirme denemesiydi: şifresiz uyarı sayfasını göster.
    Warn(String),
    /// Sertifika hatası sayfasını göster.
    Error,
}

impl Shield {
    pub fn new(https_first: bool, strip_tracking: bool) -> Shield {
        Shield {
            https_first,
            strip_tracking,
            ..Shield::default()
        }
    }

    /// Sekmede bir sayfaya gidilmeden hemen önce çağrılır.
    pub fn decide(&mut self, tab: u64, url: &str) -> Decision {
        let decision = self.decide_inner(tab, url);
        if decision == Decision::Allow {
            self.started.insert(tab, url.to_string());
        }
        decision
    }

    fn decide_inner(&mut self, tab: u64, url: &str) -> Decision {
        if address::is_internal_page(url) {
            self.pending.remove(&tab);
            return Decision::Allow;
        }
        let Some(host) = address::exact_host(url) else {
            return Decision::Allow;
        };
        if self.strip_tracking
            && let Some(clean) = strip_tracking(url)
        {
            if self.stripped.get(&tab) != Some(&clean) {
                self.stripped.insert(tab, clean.clone());
                return Decision::Redirect(clean);
            }
        } else if self.stripped.get(&tab).is_some_and(|last| last != url) {
            self.stripped.remove(&tab);
        }
        let waiting = self.pending.get(&tab).map(|(h, _)| h.clone());
        if waiting.as_deref() != Some(host.as_str()) {
            self.pending.remove(&tab);
        }
        let Some(rest) = url.strip_prefix("http://") else {
            return Decision::Allow;
        };
        if !self.https_first || !upgradable(&host) || self.allowed_http.contains(&host) {
            return Decision::Allow;
        }
        if waiting.as_deref() == Some(host.as_str()) {
            // Şifreli adres bizi yeniden şifresize yönlendirdi: site şifreli
            // bağlantıyı desteklemiyor, yeniden denemek döngüye sokar.
            self.pending.remove(&tab);
            return Decision::Warn(url.to_string());
        }
        self.pending.insert(tab, (host, url.to_string()));
        Decision::Redirect(format!("https://{rest}"))
    }

    /// Sekmedeki gezinti bitti. Şifreliye çevrilmiş bir gezinti başarısız
    /// olduysa uyarı sayfası için asıl şifresiz adresi verir. Kullanıcının ya
    /// da bizim durdurduğumuz gezintiler (`cancelled`) sayılmaz.
    pub fn finished(&mut self, tab: u64, success: bool, cancelled: bool) -> Option<String> {
        if cancelled {
            return None;
        }
        let (_, http_url) = self.pending.remove(&tab)?;
        (!success).then_some(http_url)
    }

    /// Kullanıcı uyarı sayfasında "yine de devam et" dedi.
    pub fn allow_http(&mut self, url: &str) {
        if let Some(host) = address::exact_host(url) {
            self.allowed_http.insert(host);
        }
    }

    pub fn forget_tab(&mut self, tab: u64) {
        self.pending.remove(&tab);
        self.stripped.remove(&tab);
        self.started.remove(&tab);
    }

    /// Sekmede en son başlayan gezintinin adresi.
    pub fn last_started(&self, tab: u64) -> Option<&str> {
        self.started.get(&tab).map(String::as_str)
    }

    /// Sekmede `url` açılırken sunucunun sertifikası geçersiz çıktı.
    pub fn cert_error(&mut self, tab: u64, url: &str) -> CertAction {
        let Some(host) = address::exact_host(url) else {
            return CertAction::Error;
        };
        if self.allowed_cert.contains(&host) {
            return CertAction::Allow;
        }
        if self.pending.get(&tab).is_some_and(|(h, _)| *h == host) {
            let (_, http) = self.pending.remove(&tab).expect("az önce bakıldı");
            return CertAction::Warn(http);
        }
        CertAction::Error
    }

    /// Kullanıcı sertifika hatası sayfasında "yine de devam et" dedi.
    pub fn allow_cert(&mut self, url: &str) {
        if let Some(host) = address::exact_host(url) {
            self.allowed_cert.insert(host);
        }
    }

    /// Bu sitede geçersiz sertifikaya rağmen devam edildi mi?
    pub fn cert_bypassed(&self, url: &str) -> bool {
        url.starts_with("https://")
            && address::exact_host(url).is_some_and(|h| self.allowed_cert.contains(&h))
    }
}

/// Edge'in izleme önleme seviyesi (WebView2'de profil başına).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackingLevel {
    Off,
    Basic,
    #[default]
    Balanced,
    Strict,
}

impl TrackingLevel {
    pub const ALL: [TrackingLevel; 4] = [Self::Off, Self::Basic, Self::Balanced, Self::Strict];

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Basic => "basic",
            Self::Balanced => "balanced",
            Self::Strict => "strict",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Kapalı",
            Self::Basic => "Temel",
            Self::Balanced => "Dengeli",
            Self::Strict => "Katı",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Self::Off => "Edge'in izleyici listesi kullanılmaz (reklam engelleyici yine çalışır)",
            Self::Basic => {
                "Yalnızca zararlı izleyiciler: parmak izi toplayanlar, kripto madencileri"
            }
            Self::Balanced => "Ziyaret etmediğin sitelerin izleyicileri de engellenir (önerilen)",
            Self::Strict => {
                "İzleyicilerin çoğu engellenir; bazı sitelerde giriş ya da video bozulabilir"
            }
        }
    }

    pub fn from_id(id: &str) -> Option<TrackingLevel> {
        Self::ALL.into_iter().find(|l| l.id() == id.trim())
    }
}

/// Uyarı sayfasının adresi: `http://julie.insecure/?url=<şifresiz adres>`.
pub fn warning_url(http_url: &str) -> String {
    format!(
        "{}?url={}",
        address::INSECURE_URL,
        address::encode_query(http_url)
    )
}

/// Uyarı sayfasının HTML'i. `query`, sayfa adresinin `?` sonrası.
pub fn render_warning(query: Option<&str>) -> String {
    let url = query
        .unwrap_or_default()
        .split('&')
        .find_map(|pair| pair.strip_prefix("url="))
        .map(crate::history_page::percent_decode)
        .filter(|u| u.starts_with("http://"))
        .unwrap_or_default();
    crate::error_page::render_insecure(&url)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cert_errors() {
        let mut s = super::Shield::new(true, false);
        assert_eq!(
            s.cert_error(1, "https://bad.example/"),
            super::CertAction::Error
        );
        // Şifreliye çevirme denemesi: uyarı sayfası, sonra bekleyen yok.
        assert!(matches!(
            s.decide(2, "http://old.example/a"),
            super::Decision::Redirect(_)
        ));
        assert_eq!(
            s.cert_error(2, "https://old.example/a"),
            super::CertAction::Warn("http://old.example/a".into())
        );
        assert_eq!(s.finished(2, false, false), None);
        s.allow_cert("https://bad.example/x");
        assert_eq!(
            s.cert_error(1, "https://bad.example/y"),
            super::CertAction::Allow
        );
        assert!(s.cert_bypassed("https://bad.example/"));
        assert!(!s.cert_bypassed("https://good.example/"));
        assert_eq!(s.decide(1, "https://bad.example/z"), super::Decision::Allow);
        assert_eq!(s.last_started(1), Some("https://bad.example/z"));
    }

    use super::*;

    #[test]
    fn strips_tracking_params() {
        assert_eq!(
            strip_tracking("https://a.com/p?utm_source=x&id=5&fbclid=abc#top"),
            Some("https://a.com/p?id=5#top".into())
        );
        assert_eq!(
            strip_tracking("https://a.com/p?UTM_Medium=x&gclid=1"),
            Some("https://a.com/p".into())
        );
        assert_eq!(strip_tracking("https://a.com/p?id=5&q=utm_source"), None);
        assert_eq!(strip_tracking("https://a.com/p"), None);
        assert_eq!(strip_tracking("https://a.com/#?utm_source=x"), None);
    }

    #[test]
    fn strip_does_not_loop() {
        let mut s = Shield::new(false, true);
        let dirty = "https://a.com/?utm_source=x";
        assert_eq!(
            s.decide(1, dirty),
            Decision::Redirect("https://a.com/".into())
        );
        assert_eq!(s.decide(1, "https://a.com/"), Decision::Allow);
        // Site parametreyi geri ekleyerek yönlendirdi: bir daha temizleme.
        assert_eq!(s.decide(1, dirty), Decision::Allow);
        // Başka bir sayfadan sonra yine temizlenir.
        assert_eq!(s.decide(1, "https://b.com/"), Decision::Allow);
        assert_eq!(
            s.decide(1, dirty),
            Decision::Redirect("https://a.com/".into())
        );
    }

    #[test]
    fn upgrades_http_and_warns_on_failure() {
        let mut s = Shield::new(true, true);
        assert_eq!(
            s.decide(1, "http://example.com/a"),
            Decision::Redirect("https://example.com/a".into())
        );
        // Bizim durdurduğumuz ilk gezintinin bitişi sayılmaz.
        assert_eq!(s.finished(1, false, true), None);
        assert_eq!(s.decide(1, "https://example.com/a"), Decision::Allow);
        assert_eq!(
            s.finished(1, false, false),
            Some("http://example.com/a".into())
        );
        s.allow_http("http://example.com/a");
        assert_eq!(s.decide(1, "http://example.com/b"), Decision::Allow);
    }

    #[test]
    fn success_clears_pending() {
        let mut s = Shield::new(true, false);
        s.decide(1, "http://example.com/");
        s.decide(1, "https://example.com/");
        assert_eq!(s.finished(1, true, false), None);
        assert_eq!(s.finished(1, false, false), None);
    }

    #[test]
    fn redirect_back_to_http_warns() {
        let mut s = Shield::new(true, false);
        s.decide(1, "http://old.example/");
        s.decide(1, "https://old.example/");
        assert_eq!(
            s.decide(1, "http://old.example/"),
            Decision::Warn("http://old.example/".into())
        );
    }

    #[test]
    fn other_site_clears_pending() {
        let mut s = Shield::new(true, false);
        s.decide(1, "http://a.example/");
        s.decide(1, "https://b.example/");
        assert_eq!(s.finished(1, false, false), None);
    }

    #[test]
    fn leaves_local_and_disabled_alone() {
        let mut s = Shield::new(true, false);
        for url in [
            "http://localhost:8080/",
            "http://192.168.1.1/",
            "http://router/",
            "http://nas.local/",
            "http://[::1]/",
            "https://example.com/",
            "http://julie.start/",
        ] {
            assert_eq!(s.decide(1, url), Decision::Allow, "{url}");
        }
        let mut off = Shield::new(false, false);
        assert_eq!(off.decide(1, "http://example.com/"), Decision::Allow);
    }

    #[test]
    fn strips_before_upgrading() {
        let mut s = Shield::new(true, true);
        assert_eq!(
            s.decide(1, "http://example.com/?utm_source=a"),
            Decision::Redirect("http://example.com/".into())
        );
    }

    #[test]
    fn warning_page_round_trip() {
        let link = warning_url("http://example.com/a?b=c d");
        let query = link.split_once('?').map(|(_, q)| q);
        let html = render_warning(query);
        assert!(html.contains(r#""url":"http://example.com/a?b=c d""#));
        assert!(html.contains(r#""host":"example.com""#));
        // Şifresiz olmayan adres kabul edilmez.
        assert!(render_warning(Some("url=javascript:alert(1)")).contains(r#""url":"""#));
    }

    #[test]
    fn tracking_level_ids() {
        for l in TrackingLevel::ALL {
            assert_eq!(TrackingLevel::from_id(l.id()), Some(l));
        }
        assert_eq!(TrackingLevel::default(), TrackingLevel::Balanced);
    }
}
