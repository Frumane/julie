//! Varsayılan tarayıcı: Windows'tan gelen bağlantıları açmak, tek pencere
//! (Julie açıkken yeni bağlantı açık pencerede yeni sekme olur) ve Julie'yi
//! Windows'un "Varsayılan uygulamalar" listesine kaydetmek.
//!
//! Windows, bir programın kendini sessizce varsayılan yapmasına izin
//! vermez; Julie kendini kaydeder ve Windows'un ayar sayfasını açar,
//! seçimi kullanıcı yapar (Chrome ve Firefox da böyle).

use std::path::Path;

/// Windows'un bağlantıyı tek parça olarak verdiği komut satırı işareti.
/// Kayıtta `julie.exe --single-argument %1` yazılır; bağlantının içinde
/// tırnak ya da boşluk olsa bile hepsi tek adres sayılır, araya başka
/// seçenek sıkıştırılamaz (Chrome da aynı yöntemi kullanır).
pub const SINGLE_ARGUMENT: &str = "--single-argument";

/// Ham komut satırında `--single-argument`tan sonra gelen her şey (tek
/// adres). İşaret yoksa `None`.
pub fn single_argument(command_line: &str) -> Option<String> {
    let at = command_line.find(&format!(" {SINGLE_ARGUMENT}"))?;
    let rest = command_line[at + 1 + SINGLE_ARGUMENT.len()..].trim();
    let rest = rest
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(rest);
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Dışarıdan gelen bir hedefi açılacak adrese çevirir. Yalnızca web
/// adresleri ve bilgisayardaki dosyalar kabul edilir; `javascript:` ya da
/// Julie'nin kendi sayfaları gibi adresler dışarıdan açılamaz.
/// `file`: hedef bilgisayarda var olan bir dosyaysa tam yolu.
pub fn target_url(arg: &str, file: Option<&Path>) -> Option<String> {
    if let Some(path) = file {
        return Some(file_url(path));
    }
    let arg = arg.trim();
    let lower = arg.to_ascii_lowercase();
    let web = lower.starts_with("http://") || lower.starts_with("https://");
    let local = lower.starts_with("file:///");
    let internal = crate::address::is_internal_page(&lower);
    ((web && !internal) || local).then(|| arg.to_string())
}

/// `C:\Belgeler\a b.pdf` → `file:///C:/Belgeler/a%20b.pdf`,
/// `\\sunucu\paylaşım\x.html` → `file://sunucu/payla%C5%9F%C4%B1m/x.html`.
pub fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let (prefix, rest) = match text.strip_prefix("//") {
        Some(unc) => ("file://", unc.to_string()),
        None => ("file:///", text),
    };
    let mut out = String::from(prefix);
    for (i, byte) in rest.bytes().enumerate() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~')
            // Sürücü harfinden sonraki iki nokta (`C:`).
            || (byte == b':' && i == 1);
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Açık Julie'ye gönderilen mesaj: her satırda bir adres. Boş mesaj
/// "yalnızca pencereyi öne getir" demektir.
pub fn encode_message(urls: &[String]) -> String {
    urls.iter()
        .filter(|u| !u.contains(['\n', '\r']))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn decode_message(message: &str) -> Vec<String> {
    message
        .lines()
        .filter_map(|line| target_url(line, None))
        .collect()
}

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};

    use tao::event_loop::EventLoopProxy;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND, ReadFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW,
        RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

    use super::{SINGLE_ARGUMENT, decode_message, encode_message, single_argument, target_url};
    use crate::UserEvent;

    pub(crate) fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// Komut satırında açılması istenen adresler. Dosya yolları tam yola
    /// çevrilir (açık Julie başka bir klasörde çalışıyor olabilir).
    pub fn launch_urls() -> Vec<String> {
        let raw = unsafe {
            let line = windows_sys::Win32::System::Environment::GetCommandLineW();
            let mut len = 0;
            while *line.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(line, len))
        };
        let args: Vec<String> = match single_argument(&raw) {
            Some(one) => vec![one],
            None => std::env::args()
                .skip(1)
                .filter(|a| !a.starts_with("--"))
                .collect(),
        };
        args.iter()
            .filter_map(|arg| {
                let file = local_file(arg);
                target_url(arg, file.as_deref())
            })
            .collect()
    }

    fn local_file(arg: &str) -> Option<PathBuf> {
        if arg.contains("://") {
            return None;
        }
        let path = std::path::absolute(Path::new(arg)).ok()?;
        path.is_file().then_some(path)
    }

    /// Bu Windows kullanıcısının Julie'si için borunun adı. Veri klasörü
    /// de ada katılır: farklı klasörle çalışan iki Julie birbirine
    /// karışmaz.
    fn pipe_name(app_dir: &Path) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(app_dir.to_string_lossy().to_lowercase().as_bytes());
        let short: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
        format!(r"\\.\pipe\julie-{short}")
    }

    /// İlk açılan Julie'nin dinlediği boru.
    pub struct Server(HANDLE);

    pub enum Instance {
        /// Bu ilk Julie: bağlantıları bu dinleyecek.
        First(Server),
        /// Julie zaten açıktı; adresler ona gönderildi, bu süreç kapanmalı.
        Forwarded,
        /// Boru kurulamadı (ör. veri klasörü yok): tek başına çalış.
        Alone,
    }

    /// Julie zaten açıksa adresleri ona gönderir; değilse dinleyecek boruyu
    /// kurar. Boruyu ilk kuran kazanır (`FILE_FLAG_FIRST_PIPE_INSTANCE`),
    /// aynı anda açılan iki Julie'den yalnızca biri pencere açar.
    pub fn claim(app_dir: Option<&Path>, urls: &[String]) -> Instance {
        let Some(app_dir) = app_dir else {
            return Instance::Alone;
        };
        let name = pipe_name(app_dir);
        let handle = unsafe {
            CreateNamedPipeW(
                wide(&name).as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return Instance::First(Server(handle));
        }
        if send(&name, urls) {
            Instance::Forwarded
        } else {
            crate::log::error("açık Julie'ye ulaşılamadı; yeni pencere açılıyor");
            Instance::Alone
        }
    }

    fn send(name: &str, urls: &[String]) -> bool {
        use std::io::Write;
        // Açık Julie penceresini öne getirebilsin (Windows, öndeki
        // programın izni olmadan başka bir pencerenin öne çıkmasını
        // engeller).
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
        }
        let message = encode_message(urls);
        // Boru o an başka bir mesajla meşgulse kısa bir süre yeniden dene.
        for _ in 0..40 {
            if let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(name) {
                return pipe.write_all(message.as_bytes()).is_ok();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    impl Server {
        /// Gelen mesajları arka planda dinler; her biri `OpenExternal`
        /// olarak olay döngüsüne gider.
        pub fn listen(self, proxy: EventLoopProxy<UserEvent>) {
            // Tutamaç sayı olarak taşınır (işaretçi iş parçacıkları arasında
            // geçemez); boruyu yalnızca bu iş parçacığı kullanır.
            let raw = self.0 as usize;
            std::thread::spawn(move || {
                let handle = raw as HANDLE;
                loop {
                    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0
                        || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                    if connected {
                        let message = read_all(handle);
                        let urls = decode_message(&message);
                        if proxy.send_event(UserEvent::OpenExternal(urls)).is_err() {
                            break;
                        }
                    }
                    unsafe {
                        DisconnectNamedPipe(handle);
                    }
                }
                unsafe {
                    CloseHandle(handle);
                }
            });
        }
    }

    fn read_all(handle: HANDLE) -> String {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            } != 0;
            if read > 0 {
                out.extend_from_slice(&buf[..read as usize]);
            }
            // Gönderen yazıp kapatınca okuma "boru kapandı" ile biter.
            if !ok || read == 0 || out.len() > 1 << 20 {
                if !ok && unsafe { GetLastError() } != ERROR_BROKEN_PIPE {
                    crate::log::error("açılış mesajı okunamadı");
                }
                break;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    // ------------------------------------------------------------- kayıt

    const APP: &str = "Julie";
    const CLIENT_KEY: &str = r"Software\Clients\StartMenuInternet\Julie";
    const URL_PROG_ID: &str = "JulieURL";
    const FILE_PROG_ID: &str = "JulieHTML";
    /// Julie ile açılabilecek dosya türleri.
    const FILE_TYPES: [&str; 7] = [".htm", ".html", ".shtml", ".xht", ".xhtml", ".svg", ".pdf"];

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    fn create(path: &str) -> Option<Key> {
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        (status == 0).then_some(Key(key))
    }

    /// `path` anahtarına `name` (yoksa varsayılan değer) = `value` yazar.
    pub(crate) fn set(path: &str, name: Option<&str>, value: &str) -> bool {
        let Some(key) = create(path) else {
            return false;
        };
        let data = wide(value);
        let name = name.map(wide);
        let status = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * 2) as u32,
            )
        };
        status == 0
    }

    /// `path` anahtarına sayı (DWORD) yazar.
    pub(crate) fn set_dword(path: &str, name: &str, value: u32) -> bool {
        let Some(key) = create(path) else {
            return false;
        };
        let status = unsafe {
            RegSetValueExW(
                key.0,
                wide(name).as_ptr(),
                0,
                REG_DWORD,
                &value as *const u32 as *const u8,
                4,
            )
        };
        status == 0
    }

    /// Anahtarı alt anahtarlarıyla birlikte siler.
    pub(crate) fn delete_tree(path: &str) {
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(path).as_ptr());
            windows_sys::Win32::System::Registry::RegDeleteKeyW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
            );
        }
    }

    fn delete_value(path: &str, name: &str) {
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(path).as_ptr(), wide(name).as_ptr());
        }
    }

    pub(crate) fn get(path: &str, name: Option<&str>) -> Option<String> {
        let path = wide(path);
        let name = name.map(wide);
        let name_ptr = name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());
        let mut size = 0u32;
        unsafe {
            if RegGetValueW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                name_ptr,
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            ) != 0
            {
                return None;
            }
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            if RegGetValueW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                name_ptr,
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
            ) != 0
            {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }

    pub(crate) fn key_exists(path: &str) -> bool {
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                KEY_READ,
                &mut key,
            )
        };
        if status == 0 {
            drop(Key(key));
        }
        status == 0
    }

    /// Julie'yi Windows'a tarayıcı olarak tanıtır (yalnızca bu kullanıcı
    /// için; yönetici izni gerekmez). Varsayılan yapmaz, seçenek olarak
    /// listeye ekler.
    pub fn register() -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let exe = exe.to_string_lossy().into_owned();
        let icon = format!("\"{exe}\",0");
        let open = format!("\"{exe}\" {SINGLE_ARGUMENT} %1");
        let caps = format!(r"{CLIENT_KEY}\Capabilities");
        let description = "Hafif ve hızlı bir web tarayıcısı";

        let mut ok = true;
        for prog_id in [URL_PROG_ID, FILE_PROG_ID] {
            let base = format!(r"Software\Classes\{prog_id}");
            let label = if prog_id == URL_PROG_ID {
                "Julie bağlantısı"
            } else {
                "Julie belgesi"
            };
            ok &= set(&base, None, label);
            ok &= set(&base, Some("FriendlyTypeName"), label);
            ok &= set(&format!(r"{base}\DefaultIcon"), None, &icon);
            ok &= set(&format!(r"{base}\shell\open\command"), None, &open);
            ok &= set(
                &format!(r"{base}\Application"),
                Some("ApplicationName"),
                APP,
            );
            ok &= set(
                &format!(r"{base}\Application"),
                Some("ApplicationIcon"),
                &icon,
            );
        }
        ok &= set(
            &format!(r"Software\Classes\{URL_PROG_ID}"),
            Some("URL Protocol"),
            "",
        );

        ok &= set(CLIENT_KEY, None, APP);
        ok &= set(&format!(r"{CLIENT_KEY}\DefaultIcon"), None, &icon);
        ok &= set(
            &format!(r"{CLIENT_KEY}\shell\open\command"),
            None,
            &format!("\"{exe}\""),
        );
        ok &= set(&caps, Some("ApplicationName"), APP);
        ok &= set(&caps, Some("ApplicationDescription"), description);
        ok &= set(&caps, Some("ApplicationIcon"), &icon);
        ok &= set(
            &format!(r"{caps}\StartMenu"),
            Some("StartMenuInternet"),
            APP,
        );
        for scheme in ["http", "https"] {
            ok &= set(
                &format!(r"{caps}\URLAssociations"),
                Some(scheme),
                URL_PROG_ID,
            );
        }
        for ext in FILE_TYPES {
            ok &= set(
                &format!(r"{caps}\FileAssociations"),
                Some(ext),
                FILE_PROG_ID,
            );
            ok &= set(
                &format!(r"Software\Classes\{ext}\OpenWithProgids"),
                Some(FILE_PROG_ID),
                "",
            );
        }
        ok &= set(r"Software\RegisteredApplications", Some(APP), &caps);
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
        if !ok {
            crate::log::error("Julie Windows'a tarayıcı olarak kaydedilemedi");
        }
        ok
    }

    /// Julie'nin tarayıcı kaydını siler (kaldırırken).
    pub fn unregister() {
        for prog_id in [URL_PROG_ID, FILE_PROG_ID] {
            delete_tree(&format!(r"Software\Classes\{prog_id}"));
        }
        for ext in FILE_TYPES {
            delete_value(
                &format!(r"Software\Classes\{ext}\OpenWithProgids"),
                FILE_PROG_ID,
            );
        }
        delete_value(r"Software\RegisteredApplications", APP);
        delete_tree(CLIENT_KEY);
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }

    /// Daha önce kaydedildiyse kaydı tazeler: julie.exe başka bir yere
    /// taşındıysa bağlantılar eski yolu aramasın.
    pub fn refresh_registration() {
        if !key_exists(CLIENT_KEY) {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let expected = format!("\"{}\" {SINGLE_ARGUMENT} %1", exe.to_string_lossy());
        let current = get(
            &format!(r"Software\Classes\{URL_PROG_ID}\shell\open\command"),
            None,
        );
        if current.as_deref() != Some(expected.as_str()) {
            register();
        }
    }

    /// Julie şu an web bağlantılarını açan tarayıcı mı?
    pub fn is_default() -> bool {
        ["UserChoiceLatest", "UserChoice"].iter().any(|choice| {
            get(
                &format!(
                    r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\{choice}"
                ),
                Some("ProgId"),
            )
            .is_some_and(|id| id.eq_ignore_ascii_case(URL_PROG_ID))
        })
    }

    /// Kaydeder ve Windows'un varsayılan uygulamalar sayfasını Julie'nin
    /// satırında açar; seçimi kullanıcı yapar.
    pub fn make_default() {
        register();
        // Windows 11: doğrudan Julie'nin satırı. Eski Windows bu adresi
        // tanımazsa genel varsayılan uygulamalar sayfası açılır.
        let page = format!("ms-settings:defaultapps?registeredAppUser={APP}");
        if !shell_open(&page) && !shell_open("ms-settings:defaultapps") {
            crate::log::error("Windows'un varsayılan uygulamalar sayfası açılamadı");
        }
    }

    /// Adresi Windows'un kendisine açtırır (`explorer.exe adres` biçimi
    /// `?` içeren ayar adreslerini anlamayıp Belgeler'i açıyor).
    fn shell_open(target: &str) -> bool {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide(target).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // 32'den büyükse başarılı (ShellExecute'un eski kuralı).
        result as isize > 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_argument_takes_the_whole_rest() {
        assert_eq!(
            single_argument(r#""C:\j\julie.exe" --single-argument https://a.com/?q=x y --private"#),
            Some("https://a.com/?q=x y --private".into())
        );
        assert_eq!(
            single_argument(r#"julie.exe --single-argument "C:\a b\x.pdf""#),
            Some(r"C:\a b\x.pdf".into())
        );
        assert_eq!(single_argument("julie.exe --private"), None);
        assert_eq!(single_argument("julie.exe --single-argument "), None);
    }

    #[test]
    fn only_web_and_file_targets_open() {
        assert_eq!(
            target_url("https://youtube.com", None),
            Some("https://youtube.com".into())
        );
        assert_eq!(
            target_url("HTTP://a.com", None),
            Some("HTTP://a.com".into())
        );
        assert_eq!(
            target_url("file:///C:/a.html", None),
            Some("file:///C:/a.html".into())
        );
        assert_eq!(target_url("javascript:alert(1)", None), None);
        assert_eq!(target_url("http://julie.settings/", None), None);
        assert_eq!(target_url("youtube.com", None), None);
        assert_eq!(
            target_url("x", Some(Path::new(r"C:\a b\ödev.pdf"))),
            Some("file:///C:/a%20b/%C3%B6dev.pdf".into())
        );
    }

    #[test]
    fn file_urls() {
        assert_eq!(
            file_url(Path::new(r"C:\Belgeler\Kitap.pdf")),
            "file:///C:/Belgeler/Kitap.pdf"
        );
        assert_eq!(
            file_url(Path::new(r"\\sunucu\paylaşım\x#1.html")),
            "file://sunucu/payla%C5%9F%C4%B1m/x%231.html"
        );
    }

    #[test]
    fn messages_round_trip_and_are_filtered() {
        let urls = vec![
            "https://a.com".to_string(),
            "file:///C:/x.pdf".to_string(),
            "https://b.com/\nhttps://evil".to_string(),
        ];
        let message = encode_message(&urls);
        assert_eq!(
            decode_message(&message),
            vec!["https://a.com".to_string(), "file:///C:/x.pdf".to_string()]
        );
        assert_eq!(
            decode_message("javascript:x\nhttp://julie.history/"),
            Vec::<String>::new()
        );
        assert!(decode_message("").is_empty());
    }
}
