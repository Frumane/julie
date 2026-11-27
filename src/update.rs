//! Otomatik güncelleme.
//!
//! Kurulu Julie açılışta (ve açık kaldıkça günde bir) GitHub'daki son
//! sürüme bakar. Yeni sürüm varsa bildirim gösterir; kullanıcı tıklarsa
//! `julie.exe`'yi indirir, GitHub'ın verdiği SHA-256 özetiyle
//! karşılaştırır ve kurulu dosyayla değiştirir. Julie yeniden başlayınca
//! yeni sürüm açılır. Kullanıcı istemeden hiçbir şey indirilmez ya da
//! kurulmaz.
//!
//! Çalışan bir programın dosyası silinemez ama adı değiştirilebilir:
//! `julie.exe` → `julie.exe.old`, indirilen → `julie.exe`. Eski dosya
//! sonraki açılışta silinir.

use sha2::{Digest, Sha256};

/// Son sürümün bilgisi (GitHub API).
pub const LATEST_URL: &str = "https://api.github.com/repos/Frumane/julie/releases/latest";
/// İndirilen dosyanın gelebileceği tek yer.
const DOWNLOAD_PREFIX: &str = "https://github.com/Frumane/julie/releases/download/";
/// Sürümdeki programın adı.
const ASSET: &str = "julie.exe";
/// İndirilecek dosyanın en fazla boyutu.
pub const MAX_SIZE: u64 = 200 * 1024 * 1024;

/// Yayımlanmış bir sürüm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `0.3.0`
    pub version: String,
    pub url: String,
    /// Dosyanın SHA-256 özeti (küçük harf, onaltılık).
    pub sha256: String,
    pub size: u64,
}

/// `1.2.3` (başında `v` olabilir) → (1, 2, 3).
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.').map(|p| p.parse::<u32>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// `candidate`, `current`'tan yeni mi?
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// GitHub'ın "son sürüm" yanıtını okur. Taslak ya da ön sürüm, eksik ya
/// da şüpheli bilgi varsa `None`.
pub fn parse_latest(json: &str) -> Option<Release> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value["draft"].as_bool() == Some(true) || value["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = value["tag_name"].as_str()?;
    let (major, minor, patch) = parse_version(tag)?;
    let asset = value["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"].as_str() == Some(ASSET))?;
    let url = asset["browser_download_url"].as_str()?;
    if !url.starts_with(DOWNLOAD_PREFIX) || !url.ends_with(&format!("/{ASSET}")) {
        return None;
    }
    let sha256 = asset["digest"]
        .as_str()?
        .strip_prefix("sha256:")?
        .to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let size = asset["size"].as_u64()?;
    if size == 0 || size > MAX_SIZE {
        return None;
    }
    Some(Release {
        version: format!("{major}.{minor}.{patch}"),
        url: url.to_string(),
        sha256,
        size,
    })
}

/// İndirilen dosya sürümdeki dosyanın aynısı mı?
pub fn verify(bytes: &[u8], release: &Release) -> bool {
    let hash: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    bytes.len() as u64 == release.size && hash == release.sha256
}

/// Yeniden başlatılan Julie'ye eski sürecin numarası verilir: o kapanana
/// kadar bekler (yoksa açık Julie'ye bağlanıp hemen kapanırdı).
pub const WAIT_ARG: &str = "--wait-for";

/// Komut satırındaki `--wait-for <numara>`.
pub fn wait_pid(args: &[String]) -> Option<u32> {
    let at = args.iter().position(|a| a == WAIT_ARG)?;
    args.get(at + 1)?.parse().ok()
}

#[cfg(windows)]
pub mod net {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{LATEST_URL, MAX_SIZE, Release, is_newer, parse_latest, verify};

    fn agent() -> ureq::Agent {
        use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
        ureq::Agent::config_builder()
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(format!("Julie/{}", crate::install::version()))
            .build()
            .into()
    }

    /// Yeni sürüm var mı? Yoksa `Ok(None)`.
    pub fn check() -> Result<Option<Release>, String> {
        let json = agent()
            .get(LATEST_URL)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("güncelleme denetlenemedi: {e}"))?
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| format!("güncelleme denetlenemedi: {e}"))?;
        let release = parse_latest(&json).ok_or("sürüm bilgisi okunamadı")?;
        Ok(is_newer(&release.version, crate::install::version()).then_some(release))
    }

    /// Yeni sürümü indirir, doğrular ve kurulu dosyanın yerine koyar.
    pub fn install(release: &Release, exe: &Path) -> Result<(), String> {
        let bytes = agent()
            .get(&release.url)
            .call()
            .map_err(|e| format!("indirilemedi: {e}"))?
            .body_mut()
            .with_config()
            .limit(MAX_SIZE)
            .read_to_vec()
            .map_err(|e| format!("indirilemedi: {e}"))?;
        if !verify(&bytes, release) {
            return Err("indirilen dosya sürümdekiyle aynı değil".to_string());
        }
        let new = with_suffix(exe, ".new");
        let old = with_suffix(exe, ".old");
        std::fs::write(&new, &bytes).map_err(|e| format!("dosya yazılamadı: {e}"))?;
        // Önceki güncellemeden kalan eski dosya (artık çalışmıyor).
        let _ = std::fs::remove_file(&old);
        if let Err(e) = std::fs::rename(exe, &old) {
            let _ = std::fs::remove_file(&new);
            return Err(format!("kurulu dosya taşınamadı: {e}"));
        }
        if let Err(e) = std::fs::rename(&new, exe) {
            // Geri al: Julie bozulmasın.
            let _ = std::fs::rename(&old, exe);
            let _ = std::fs::remove_file(&new);
            return Err(format!("yeni dosya yerine konamadı: {e}"));
        }
        Ok(())
    }

    fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    }

    /// Önceki güncellemeden kalan dosyaları siler.
    pub fn clean_up(exe: &Path) {
        for suffix in [".old", ".new"] {
            let _ = std::fs::remove_file(with_suffix(exe, suffix));
        }
    }

    /// Eski Julie kapanana kadar (en fazla 20 saniye) bekler.
    pub fn wait_for(pid: u32) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                return;
            }
            WaitForSingleObject(handle, 20_000);
            CloseHandle(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(tag: &str, url: &str, digest: &str) -> String {
        serde_json::json!({
            "tag_name": tag, "draft": false, "prerelease": false,
            "assets": [
                {"name": "notlar.txt", "browser_download_url": "https://x/y", "size": 1},
                {"name": "julie.exe", "browser_download_url": url, "size": 3, "digest": digest},
            ],
        })
        .to_string()
    }

    const URL: &str = "https://github.com/Frumane/julie/releases/download/v0.3.0/julie.exe";
    // "abc"
    const ABC: &str = "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10.0"), Some((0, 10, 0)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.x"), None);
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("bozuk", "0.2.0"));
    }

    #[test]
    fn reads_latest_release() {
        let r = parse_latest(&reply("v0.3.0", URL, ABC)).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.url, URL);
        assert_eq!(r.size, 3);
        assert!(verify(b"abc", &r));
        assert!(!verify(b"abd", &r));
        assert!(!verify(b"abcd", &r));
    }

    #[test]
    fn rejects_suspicious_releases() {
        // Başka bir adres.
        let evil = "https://evil.example/Frumane/julie/releases/download/v0.3.0/julie.exe";
        assert!(parse_latest(&reply("v0.3.0", evil, ABC)).is_none());
        let other = "https://github.com/someone/julie/releases/download/v0.3.0/julie.exe";
        assert!(parse_latest(&reply("v0.3.0", other, ABC)).is_none());
        // Özet yok ya da bozuk.
        assert!(parse_latest(&reply("v0.3.0", URL, "")).is_none());
        assert!(parse_latest(&reply("v0.3.0", URL, "sha256:abc")).is_none());
        assert!(parse_latest(&reply("v0.3.0", URL, "md5:00")).is_none());
        // Sürüm numarası değil.
        assert!(parse_latest(&reply("son", URL, ABC)).is_none());
        // Ön sürüm.
        let mut pre: serde_json::Value = serde_json::from_str(&reply("v0.3.0", URL, ABC)).unwrap();
        pre["prerelease"] = true.into();
        assert!(parse_latest(&pre.to_string()).is_none());
        assert!(parse_latest("bozuk").is_none());
    }

    #[test]
    fn wait_argument() {
        let args = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(
            wait_pid(&args(&["julie.exe", "--wait-for", "42"])),
            Some(42)
        );
        assert_eq!(wait_pid(&args(&["julie.exe", "--wait-for"])), None);
        assert_eq!(wait_pid(&args(&["julie.exe"])), None);
    }
}
