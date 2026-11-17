//! Reklam ve izleyici engelleyici.
//!
//! Kurallar EasyList ve uBlock Origin listelerinden gelir ve Brave
//! tarayıcısının kullandığı `adblock` kütüphanesiyle eşleştirilir. Derlenmiş
//! kurallar diske kaydedilir; sonraki açılışlarda listeler yeniden
//! ayrıştırılmaz, önbellek milisaniyeler içinde yüklenir.
//!
//! Sayfaya enjekte edilen betikler (scriptlet) ve engellenen isteklerin yerine
//! verilen sahte dosyalar uBlock Origin'den alınır
//! (`resources/ubo-resources.json`, `tools/build-ubo-resources.mjs` ile üretilir).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::resources::{PermissionMask, Resource};

/// uBlock Origin'in betikleri ve yönlendirme kaynakları (GPLv3).
const UBO_RESOURCES: &str = include_str!("../resources/ubo-resources.json");

/// Güvenilir listelere verilen izin; `build-ubo-resources.mjs` içindeki
/// `TRUSTED` ile aynı olmalı.
const TRUSTED: PermissionMask = PermissionMask::from_bits(1);

/// Bir isteğe ne yapılacağı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
    /// Engelle ama yerine bu `data:` adresindeki zararsız içeriği ver
    /// (ör. sahte Google Analytics). Bazı siteler reklam/izleme betiği
    /// hiç gelmezse bozulur; uBlock bunu böyle çözer.
    Redirect(String),
}

/// İndirilmiş bir filtre listesi.
pub struct ListText {
    pub text: String,
    pub trusted: bool,
}

/// Listeler bundan eskiyse arka planda yenilenir.
pub const MAX_AGE: Duration = Duration::from_secs(4 * 24 * 60 * 60);

pub struct Blocker {
    engine: Engine,
}

impl Blocker {
    /// Liste metinlerinden kuralları derler (birkaç yüz milisaniye sürer).
    pub fn compile(lists: impl IntoIterator<Item = ListText>) -> Blocker {
        let mut set = FilterSet::new(false);
        for list in lists {
            let options = ParseOptions {
                permissions: if list.trusted {
                    TRUSTED
                } else {
                    PermissionMask::default()
                },
                ..ParseOptions::default()
            };
            set.add_filter_list(list.text, options);
        }
        Blocker::with_resources(Engine::new_with_filter_set(set))
    }

    /// Önceden kaydedilmiş derlenmiş kuralları yükler.
    pub fn deserialize(bytes: &[u8]) -> Option<Blocker> {
        let mut engine = Engine::default();
        engine.deserialize(bytes).ok()?;
        Some(Blocker::with_resources(engine))
    }

    /// Kaynaklar önbelleğe yazılmaz; her yüklemede programın içinden eklenir.
    fn with_resources(mut engine: Engine) -> Blocker {
        engine.use_resources(ubo_resources());
        Blocker { engine }
    }

    pub fn serialize(&self) -> Vec<u8> {
        self.engine.serialize()
    }

    /// İstek engellenmeli mi?
    ///
    /// - `url`: istenen kaynak,
    /// - `page_url`: isteği yapan sayfa (üçüncü taraf kontrolü için),
    /// - `request_type`: `script`, `image`, `sub_frame` ...
    #[cfg(test)]
    pub fn should_block(&self, url: &str, page_url: &str, request_type: &str) -> bool {
        self.check(url, page_url, request_type) != Verdict::Allow
    }

    /// İsteğe ne yapılacağına karar verir.
    pub fn check(&self, url: &str, page_url: &str, request_type: &str) -> Verdict {
        // Ana sayfanın kendisi asla engellenmez.
        if request_type == "main_frame" {
            return Verdict::Allow;
        }
        let Ok(request) = Request::new(url, page_url, request_type, "GET") else {
            return Verdict::Allow;
        };
        let result = self.engine.check_network_request(&request);
        match (result.should_block(), result.redirect) {
            (true, Some(data_url)) => Verdict::Redirect(data_url),
            // `redirect-rule`: yalnızca başka bir kural da engelliyorsa yönlendir.
            (false, _) => Verdict::Allow,
            (true, None) => Verdict::Block,
        }
    }

    /// Sayfa yüklenmeden önce enjekte edilecek betikler (boş olabilir).
    pub fn scriptlets(&self, url: &str) -> String {
        self.engine.url_cosmetic_resources(url).injected_script
    }
}

/// Programa gömülü uBlock kaynaklarını okur.
fn ubo_resources() -> Vec<Resource> {
    serde_json::from_str::<serde_json::Value>(UBO_RESOURCES)
        .ok()
        .and_then(|mut doc| serde_json::from_value(doc["resources"].take()).ok())
        .unwrap_or_default()
}

/// Bir sayfa için kozmetik (gizleme) kuralları.
#[derive(Debug, Default)]
pub struct PageCosmetics {
    /// Sayfada gizlenecek CSS seçicileri.
    pub hide_selectors: Vec<String>,
    /// Genel sınıf/kimlik kurallarından muaf seçiciler.
    pub exceptions: HashSet<String>,
    /// `true` ise bu sayfada genel kurallar uygulanmaz.
    pub generichide: bool,
}

impl Blocker {
    /// Adrese özgü gizleme kuralları (ör. `example.com##.reklam`).
    pub fn page_cosmetics(&self, url: &str) -> PageCosmetics {
        let resources = self.engine.url_cosmetic_resources(url);
        PageCosmetics {
            hide_selectors: resources.hide_selectors.into_iter().collect(),
            exceptions: resources.exceptions,
            generichide: resources.generichide,
        }
    }

    /// Sayfada görülen sınıf ve kimliklere uyan genel gizleme kuralları
    /// (ör. `##.ad-banner`). Bütün genel kuralları her sayfaya basmak yerine
    /// yalnızca sayfada gerçekten bulunanlar sorulur.
    pub fn generic_selectors(
        &self,
        classes: &[&str],
        ids: &[&str],
        exceptions: &HashSet<String>,
    ) -> Vec<String> {
        self.engine
            .hidden_class_id_selectors(classes, ids, exceptions)
    }
}

/// WebView2'nin kaynak türünü (`COREWEBVIEW2_WEB_RESOURCE_CONTEXT`)
/// filtre listelerinin kullandığı türe çevirir.
pub fn request_type(context: i32, is_main_frame: bool) -> &'static str {
    match context {
        1 if is_main_frame => "main_frame",
        1 => "sub_frame",
        2 => "stylesheet",
        3 => "image",
        4 | 9 => "media",
        5 => "font",
        6 => "script",
        7 | 8 | 10 => "xmlhttprequest",
        11 => "websocket",
        14 | 15 => "ping",
        _ => "other",
    }
}

/// Derlenmiş kuralların önbellek dosyası.
/// Liste seti değiştiğinde dosya adı da değişir; böylece eski önbellek
/// kullanılmaz, yeni listeler hemen indirilir.
pub fn cache_path(app_dir: &Path) -> PathBuf {
    app_dir.join("filters").join("engine-v2.dat")
}

/// Önbellek yoksa ya da `MAX_AGE`'den eskiyse `true`.
pub fn needs_update(cache: &Path) -> bool {
    let modified = std::fs::metadata(cache).and_then(|m| m.modified());
    match modified {
        Ok(time) => SystemTime::now()
            .duration_since(time)
            .is_ok_and(|age| age > MAX_AGE),
        Err(_) => true,
    }
}

/// Listeleri indirir, derler ve önbelleğe yazar. Derlenmiş veriyi döndürür.
/// Ağ isteği yaptığı için arka plan iş parçacığında çağrılmalıdır.
#[cfg(windows)]
pub fn update(cache: &Path) -> Result<Vec<u8>, String> {
    use crate::filter_lists::{LISTS, include_url, preprocess};
    let mut lists = Vec::new();
    for list in &LISTS {
        let text = download(list.url).map_err(|e| format!("{} indirilemedi: {e}", list.name))?;
        // `!#include` edilen dosyalar aynı klasörden indirilir; inmeyen
        // parça atlanır, listenin geri kalanı yine kullanılır.
        let text = preprocess(
            &text,
            &mut |name| match download(&include_url(list.url, name)) {
                Ok(sub) => Some(sub),
                Err(e) => {
                    crate::log::error(&format!("{} / {name} indirilemedi: {e}", list.name));
                    None
                }
            },
        );
        lists.push(ListText {
            text,
            trusted: list.trusted,
        });
    }
    let bytes = Blocker::compile(lists).serialize();
    write_atomic(cache, &bytes).map_err(|e| format!("önbellek yazılamadı: {e}"))?;
    Ok(bytes)
}

#[cfg(windows)]
fn download(url: &str) -> Result<String, ureq::Error> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                // Windows'un sertifika deposu: HTTPS trafiğini tarayan
                // antivirüslerin eklediği kök sertifikalar da tanınır.
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into();
    agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_string()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "\
||ads.example^
||tracker.example^$third-party
/banner/*$image
@@||ads.example/allowed.js
";

    fn untrusted(text: &str) -> ListText {
        ListText {
            text: text.to_string(),
            trusted: false,
        }
    }

    fn blocker() -> Blocker {
        Blocker::compile([untrusted(RULES)])
    }

    #[test]
    fn blocks_listed_hosts() {
        let b = blocker();
        let page = "https://news.example/article";
        assert!(b.should_block("https://ads.example/ad.js", page, "script"));
        assert!(b.should_block("https://cdn.ads.example/x.png", page, "image"));
        assert!(!b.should_block("https://news.example/app.js", page, "script"));
    }

    #[test]
    fn respects_options_and_exceptions() {
        let b = blocker();
        let page = "https://news.example/";
        // Yalnızca üçüncü taraf olarak engellenir.
        assert!(b.should_block("https://tracker.example/t.js", page, "script"));
        assert!(!b.should_block(
            "https://tracker.example/t.js",
            "https://tracker.example/",
            "script"
        ));
        // Yalnızca resimler.
        assert!(b.should_block("https://cdn.example/banner/1.png", page, "image"));
        assert!(!b.should_block("https://cdn.example/banner/1.js", page, "script"));
        // İstisna kuralı.
        assert!(!b.should_block("https://ads.example/allowed.js", page, "script"));
    }

    #[test]
    fn never_blocks_the_page_itself() {
        let b = blocker();
        assert!(!b.should_block("https://ads.example/", "https://ads.example/", "main_frame"));
        assert!(b.should_block("https://ads.example/", "https://news.example/", "sub_frame"));
    }

    #[test]
    fn survives_serialization() {
        let bytes = blocker().serialize();
        let b = Blocker::deserialize(&bytes).expect("önbellek okunmalı");
        assert!(b.should_block(
            "https://ads.example/ad.js",
            "https://news.example/",
            "script"
        ));
        assert!(Blocker::deserialize(b"bozuk veri").is_none());
    }

    const COSMETIC_RULES: &str = "\
##.ad-banner
###sponsor-box
news.example##.native-ad
other.example#@#.ad-banner
";

    #[test]
    fn cosmetic_rules() {
        let b = Blocker::compile([untrusted(COSMETIC_RULES)]);
        let page = b.page_cosmetics("https://news.example/article");
        assert!(page.hide_selectors.contains(&".native-ad".to_string()));
        assert!(!page.generichide);

        let generic = b.generic_selectors(
            &["ad-banner", "content"],
            &["sponsor-box"],
            &page.exceptions,
        );
        assert!(generic.contains(&".ad-banner".to_string()));
        assert!(generic.contains(&"#sponsor-box".to_string()));
        assert!(!generic.iter().any(|s| s.contains("content")));

        // İstisna: bu sitede .ad-banner gizlenmez.
        let other = b.page_cosmetics("https://other.example/");
        let generic = b.generic_selectors(&["ad-banner"], &[], &other.exceptions);
        assert!(generic.is_empty());
    }

    #[test]
    fn cosmetic_rules_survive_serialization() {
        let bytes = Blocker::compile([untrusted(COSMETIC_RULES)]).serialize();
        let b = Blocker::deserialize(&bytes).unwrap();
        let page = b.page_cosmetics("https://news.example/");
        assert!(page.hide_selectors.contains(&".native-ad".to_string()));
        assert!(
            !b.generic_selectors(&["ad-banner"], &[], &page.exceptions)
                .is_empty()
        );
    }

    #[test]
    fn maps_webview2_contexts() {
        assert_eq!(request_type(1, true), "main_frame");
        assert_eq!(request_type(1, false), "sub_frame");
        assert_eq!(request_type(3, false), "image");
        assert_eq!(request_type(6, false), "script");
        assert_eq!(request_type(8, false), "xmlhttprequest");
        assert_eq!(request_type(16, false), "other");
    }

    #[test]
    fn cache_age() {
        let dir = std::env::temp_dir().join(format!("julie-cache-{}", std::process::id()));
        let path = cache_path(&dir);
        assert!(needs_update(&path));
        write_atomic(&path, b"x").unwrap();
        assert!(!needs_update(&path));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Gerçek uBlock listeleriyle YouTube betiklerinin üretildiğini doğrular:
    /// `JULIE_LIST_DIR=<listelerin klasörü> cargo test --release youtube -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn youtube_scriptlets_with_real_lists() {
        use crate::filter_lists::{LISTS, preprocess};
        use std::time::Instant;
        let dir = PathBuf::from(std::env::var("JULIE_LIST_DIR").expect("JULIE_LIST_DIR"));
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        let lists: Vec<ListText> = LISTS
            .iter()
            .map(|list| {
                let file = list.url.rsplit('/').next().unwrap();
                let text = read(file).unwrap_or_else(|| panic!("{file} yok"));
                ListText {
                    text: preprocess(&text, &mut |name| read(name)),
                    trusted: list.trusted,
                }
            })
            .collect();
        let total: usize = lists.iter().map(|l| l.text.len()).sum();
        println!("toplam liste boyutu: {} KB", total / 1024);

        let t = Instant::now();
        let compiled = Blocker::compile(lists);
        println!("derleme: {:?}", t.elapsed());
        let bytes = compiled.serialize();
        println!("önbellek: {} KB", bytes.len() / 1024);
        let t = Instant::now();
        let b = Blocker::deserialize(&bytes).unwrap();
        println!("önbellekten yükleme (kaynaklar dahil): {:?}", t.elapsed());

        let script = b.scriptlets("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        println!("youtube betiği: {} KB", script.len() / 1024);
        for name in [
            "jsonPrune",
            "trustedReplaceXhrResponse",
            "setConstant",
            "replaceNodeText",
        ] {
            println!("  {name}: {}", script.contains(name));
        }
        assert!(
            script.contains("adPlacements"),
            "YouTube kuralları betiğe girmeli"
        );
        if let Some(path) = std::env::var_os("JULIE_DUMP_SCRIPT") {
            std::fs::write(path, &script).unwrap();
        }
        assert!(b.scriptlets("https://example.com/").len() < script.len());

        let ga = "https://www.google-analytics.com/analytics.js";
        println!(
            "google-analytics: {:?}",
            b.check(ga, "https://news.example/", "script")
        );
    }

    /// Gerçek listelerle hız ölçümü:
    /// `JULIE_LISTS=easylist.txt,easyprivacy.txt cargo test --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn benchmark_real_lists() {
        use std::time::Instant;
        let paths = std::env::var("JULIE_LISTS").expect("JULIE_LISTS ayarlanmalı");
        // "!" ile başlayan yol güvenilir (uBlock) liste sayılır.
        let lists: Vec<ListText> = paths
            .split(',')
            .map(|p| {
                let (trusted, p) = p.strip_prefix('!').map_or((false, p), |p| (true, p));
                ListText {
                    text: std::fs::read_to_string(p).unwrap(),
                    trusted,
                }
            })
            .collect();

        let t = Instant::now();
        let compiled = Blocker::compile(lists);
        println!("derleme: {:?}", t.elapsed());

        let bytes = compiled.serialize();
        println!("önbellek boyutu: {} KB", bytes.len() / 1024);

        let t = Instant::now();
        let b = Blocker::deserialize(&bytes).unwrap();
        println!("önbellekten yükleme: {:?}", t.elapsed());

        let page = "https://www.haberler.example/gundem/haber-123";
        let urls = [
            (
                "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
                "script",
            ),
            ("https://www.google-analytics.com/analytics.js", "script"),
            (
                "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
                "script",
            ),
            ("https://connect.facebook.net/en_US/fbevents.js", "script"),
            ("https://www.haberler.example/static/app.js", "script"),
            ("https://cdn.haberler.example/img/manset.jpg", "image"),
        ];
        for (url, kind) in urls {
            println!(
                "{:5} {url}",
                if b.should_block(url, page, kind) {
                    "ENGEL"
                } else {
                    "izin"
                }
            );
        }

        let n = 100_000;
        let t = Instant::now();
        for i in 0..n {
            let (url, kind) = urls[i % urls.len()];
            std::hint::black_box(b.should_block(url, page, kind));
        }
        println!("istek başına kontrol: {:?}", t.elapsed() / n as u32);
    }
}
