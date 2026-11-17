//! Reklam ve izleyici engelleyici.
//!
//! Kurallar EasyList biçimindeki listelerden gelir ve Brave tarayıcısının
//! kullandığı `adblock` kütüphanesiyle eşleştirilir. Derlenmiş kurallar diske
//! kaydedilir; sonraki açılışlarda listeler yeniden ayrıştırılmaz, önbellek
//! milisaniyeler içinde yüklenir.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;

/// Kullanılan filtre listeleri.
pub const LISTS: [(&str, &str); 2] = [
    ("EasyList", "https://easylist.to/easylist/easylist.txt"),
    (
        "EasyPrivacy",
        "https://easylist.to/easylist/easyprivacy.txt",
    ),
];

/// Listeler bundan eskiyse arka planda yenilenir.
pub const MAX_AGE: Duration = Duration::from_secs(4 * 24 * 60 * 60);

pub struct Blocker {
    engine: Engine,
}

impl Blocker {
    /// Liste metinlerinden kuralları derler (birkaç yüz milisaniye sürer).
    pub fn compile(lists: impl IntoIterator<Item = String>) -> Blocker {
        let mut set = FilterSet::new(false);
        for list in lists {
            set.add_filter_list(list, ParseOptions::default());
        }
        Blocker {
            engine: Engine::new_with_filter_set(set),
        }
    }

    /// Önceden kaydedilmiş derlenmiş kuralları yükler.
    pub fn deserialize(bytes: &[u8]) -> Option<Blocker> {
        let mut engine = Engine::default();
        engine.deserialize(bytes).ok()?;
        Some(Blocker { engine })
    }

    pub fn serialize(&self) -> Vec<u8> {
        self.engine.serialize()
    }

    /// İstek engellenmeli mi?
    ///
    /// - `url`: istenen kaynak,
    /// - `page_url`: isteği yapan sayfa (üçüncü taraf kontrolü için),
    /// - `request_type`: `script`, `image`, `sub_frame` ...
    pub fn should_block(&self, url: &str, page_url: &str, request_type: &str) -> bool {
        // Ana sayfanın kendisi asla engellenmez.
        if request_type == "main_frame" {
            return false;
        }
        match Request::new(url, page_url, request_type, "GET") {
            Ok(request) => self.engine.check_network_request(&request).should_block(),
            Err(_) => false,
        }
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
pub fn cache_path(app_dir: &Path) -> PathBuf {
    app_dir.join("filters").join("engine.dat")
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
    let lists = LISTS
        .iter()
        .map(|(name, url)| download(url).map_err(|e| format!("{name} indirilemedi: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = Blocker::compile(lists).serialize();
    write_atomic(cache, &bytes).map_err(|e| format!("önbellek yazılamadı: {e}"))?;
    Ok(bytes)
}

#[cfg(windows)]
fn download(url: &str) -> Result<String, ureq::Error> {
    use ureq::tls::{TlsConfig, TlsProvider};
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
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

    fn blocker() -> Blocker {
        Blocker::compile([RULES.to_string()])
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

    /// Gerçek listelerle hız ölçümü:
    /// `JULIE_LISTS=easylist.txt,easyprivacy.txt cargo test --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn benchmark_real_lists() {
        use std::time::Instant;
        let paths = std::env::var("JULIE_LISTS").expect("JULIE_LISTS ayarlanmalı");
        let lists: Vec<String> = paths
            .split(',')
            .map(|p| std::fs::read_to_string(p).unwrap())
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
