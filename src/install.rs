//! Kurulum ve kaldırma. Ayrı bir kurulum programı yok: julie.exe kendini
//! kurar.
//!
//! İndirilen julie.exe ilk açılışta "Bilgisayara kurulsun mu?" diye sorar.
//! Evet denirse kendini `%LOCALAPPDATA%\Programs\Julie` klasörüne kopyalar,
//! Başlat menüsüne ve masaüstüne kısayol koyar, Windows'un "Yüklü
//! uygulamalar" listesine ekler ve kurulan kopyayı açar. Yalnızca bu
//! kullanıcı için kurulur; yönetici izni gerekmez.
//!
//! Kaldırmak için Windows'un "Yüklü uygulamalar" listesi kullanılır
//! (`julie.exe --uninstall`).

use std::path::{Path, PathBuf};

/// Kurulan kopyanın açılışı: tarayıcı olarak kaydolur.
pub const INSTALLED_ARG: &str = "--installed";
/// Windows'un "Kaldır" düğmesi.
pub const UNINSTALL_ARG: &str = "--uninstall";

/// Kurulum klasörü: `%LOCALAPPDATA%\Programs\Julie`.
pub fn install_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join("Julie")
}

/// Kurulu Julie'nin yolu.
pub fn installed_exe(local_app_data: &Path) -> PathBuf {
    install_dir(local_app_data).join("julie.exe")
}

/// Aynı dosya mı? (Windows'ta büyük/küçük harf ve `/`–`\` farkı sayılmaz.)
pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// Geliştirme derlemesi mi (`cargo run`, `target\release\julie.exe`)?
/// Bunlar kurulum sormaz.
pub fn is_dev_build(exe: &Path) -> bool {
    exe.to_string_lossy()
        .split(['\\', '/'])
        .any(|part| part.eq_ignore_ascii_case("target"))
}

/// Açılışta kurulum sorulmalı mı? `declined`: kullanıcı bu dosya için
/// daha önce "Hayır" dedi.
pub fn should_offer(exe: &Path, local_app_data: &Path, declined: Option<&Path>) -> bool {
    !same_path(exe, &installed_exe(local_app_data))
        && !is_dev_build(exe)
        && !declined.is_some_and(|d| same_path(d, exe))
}

/// Windows'un "Yüklü uygulamalar" listesindeki kayıt (ad, değer).
pub fn uninstall_entry(exe: &Path, version: &str) -> Vec<(&'static str, String)> {
    let exe = exe.to_string_lossy();
    let dir = exe
        .rsplit_once(['\\', '/'])
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default();
    vec![
        ("DisplayName", "Julie".to_string()),
        ("DisplayVersion", version.to_string()),
        ("Publisher", "Julie".to_string()),
        ("DisplayIcon", format!("\"{exe}\",0")),
        ("InstallLocation", dir),
        ("UninstallString", format!("\"{exe}\" {UNINSTALL_ARG}")),
        (
            "URLInfoAbout",
            "https://github.com/Frumane/julie".to_string(),
        ),
    ]
}

/// Julie'nin sürümü (sürüm derlemesinde etiketten, yoksa Cargo.toml'dan).
pub fn version() -> &'static str {
    option_env!("JULIE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(windows)]
mod windows_impl {
    use std::path::{Path, PathBuf};

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, IPersistFile,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
        ShellLink,
    };
    use windows::core::{GUID, HSTRING, Interface};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_SETFOREGROUND,
        MB_YESNO, MessageBoxW,
    };

    use super::{INSTALLED_ARG, installed_exe, should_offer, uninstall_entry, version};
    use crate::default_browser::{self, delete_tree, set, set_dword, wide};

    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Julie";
    /// "Hayır" denen dosyanın yolu (veri klasöründe).
    const DECLINED_FILE: &str = "install-declined.txt";

    fn local_app_data() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }

    fn ask(text: &str) -> bool {
        let flags = MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND;
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide(text).as_ptr(),
                wide("Julie").as_ptr(),
                flags,
            ) == IDYES
        }
    }

    fn tell(text: &str, error: bool) {
        let icon = if error {
            MB_ICONERROR
        } else {
            MB_ICONINFORMATION
        };
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide(text).as_ptr(),
                wide("Julie").as_ptr(),
                MB_OK | icon | MB_SETFOREGROUND,
            );
        }
    }

    /// Julie kurulu değilse (ya da başka bir kopya açıldıysa) kurmayı
    /// sorar. Kurup kurulan kopyayı açtıysa `true`: bu süreç kapanmalı.
    pub fn offer(app_dir: Option<&Path>) -> bool {
        let (Ok(exe), Some(local)) = (std::env::current_exe(), local_app_data()) else {
            return false;
        };
        let declined_path = app_dir.map(|d| d.join(DECLINED_FILE));
        let declined = declined_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| PathBuf::from(s.trim()));
        if !should_offer(&exe, &local, declined.as_deref()) {
            return false;
        }
        let target = installed_exe(&local);
        let question = if target.exists() {
            "Julie bu bilgisayarda zaten kurulu.\n\n\
             Kurulu Julie bu sürümle değiştirilsin mi?"
        } else {
            "Julie bilgisayara kurulsun mu?\n\n\
             Başlat menüsüne ve masaüstüne Julie kısayolu eklenir; Julie'yi \
             Windows'un \"Yüklü uygulamalar\" listesinden kaldırabilirsin.\n\n\
             \"Hayır\" dersen Julie kurulmadan bu dosyadan çalışır."
        };
        if !ask(question) {
            if let Some(path) = declined_path {
                let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
                let _ = std::fs::write(path, exe.to_string_lossy().as_bytes());
            }
            return false;
        }
        match install(&exe, &target) {
            Ok(()) => {
                if std::process::Command::new(&target)
                    .arg(INSTALLED_ARG)
                    .spawn()
                    .is_ok()
                {
                    return true;
                }
                tell(
                    "Julie kuruldu ama açılamadı. Başlat menüsünden açabilirsin.",
                    true,
                );
                true
            }
            Err(err) => {
                crate::log::error(&format!("kurulum başarısız: {err}"));
                tell(
                    &format!("Julie kurulamadı.\n\n{err}\n\nJulie bu dosyadan açılacak."),
                    true,
                );
                false
            }
        }
    }

    fn install(exe: &Path, target: &Path) -> Result<(), String> {
        let dir = target.parent().ok_or("kurulum klasörü yok")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Klasör oluşturulamadı: {e}"))?;
        // Önce yanına kopyalanır, sonra yer değiştirilir: yarım kalan
        // kopya kurulu Julie'yi bozmaz.
        let temp = dir.join("julie.exe.new");
        std::fs::copy(exe, &temp).map_err(|e| format!("Dosya kopyalanamadı: {e}"))?;
        if let Err(e) = std::fs::rename(&temp, target) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!(
                "Kurulu Julie değiştirilemedi (açık olabilir, kapatıp yeniden dene): {e}"
            ));
        }
        let size_kb = std::fs::metadata(target)
            .map(|m| (m.len() / 1024) as u32)
            .unwrap_or(0);
        for (name, value) in uninstall_entry(target, version()) {
            set(UNINSTALL_KEY, Some(name), &value);
        }
        set_dword(UNINSTALL_KEY, "NoModify", 1);
        set_dword(UNINSTALL_KEY, "NoRepair", 1);
        set_dword(UNINSTALL_KEY, "EstimatedSize", size_kb);
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        for folder in [&FOLDERID_Programs, &FOLDERID_Desktop] {
            if let Some(link) = known_folder(folder).map(|d| d.join("Julie.lnk"))
                && let Err(e) = shortcut(target, &link)
            {
                crate::log::error(&format!("kısayol oluşturulamadı ({link:?}): {e}"));
            }
        }
        Ok(())
    }

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        unsafe {
            let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let text = path.to_string().ok();
            CoTaskMemFree(Some(path.0 as *const _));
            text.map(PathBuf::from)
        }
    }

    fn shortcut(target: &Path, link: &Path) -> windows::core::Result<()> {
        unsafe {
            let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            shell.SetPath(&HSTRING::from(target.as_os_str()))?;
            if let Some(dir) = target.parent() {
                shell.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
            }
            shell.SetDescription(&HSTRING::from("Julie web tarayıcısı"))?;
            shell.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
            shell
                .cast::<IPersistFile>()?
                .Save(&HSTRING::from(link.as_os_str()), true)?;
        }
        Ok(())
    }

    /// Kurulan kopyanın ilk açılışı: kendini tarayıcı olarak kaydeder.
    pub fn first_run() {
        default_browser::register();
    }

    /// Bu süreç kurulu Julie ise dosyasının yolu.
    pub fn running_installed() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let installed = installed_exe(&local_app_data()?);
        super::same_path(&exe, &installed).then_some(installed)
    }

    /// "Yüklü uygulamalar" listesindeki sürüm numarasını günceller
    /// (otomatik güncellemeden sonra).
    pub fn refresh_version() {
        let current = default_browser::get(UNINSTALL_KEY, Some("DisplayVersion"));
        if current.is_some() && current.as_deref() != Some(version()) {
            set(UNINSTALL_KEY, Some("DisplayVersion"), version());
        }
    }

    /// Julie kuruluysa ve bu süreç kurulu kopya değilse `true`. (Kurulu
    /// Julie'nin tarayıcı kaydını başka bir kopya ele geçirmesin.)
    pub fn other_copy_installed() -> bool {
        let (Ok(exe), Some(local)) = (std::env::current_exe(), local_app_data()) else {
            return false;
        };
        let installed = installed_exe(&local);
        installed.exists() && !super::same_path(&exe, &installed)
    }

    /// `julie.exe --uninstall`: Julie'yi kaldırır. `running`: Julie şu an
    /// açık (önce kapatılmalı).
    pub fn uninstall(app_dir: Option<&Path>, running: bool) {
        if running {
            tell(
                "Julie açık. Kaldırmak için önce Julie'yi kapatıp yeniden dene.",
                true,
            );
            return;
        }
        if !ask("Julie bu bilgisayardan kaldırılsın mı?") {
            return;
        }
        let remove_data = app_dir.is_some_and(|d| d.exists())
            && ask(
                "Geçmiş, yer imleri, ayarlar ve eklentiler de silinsin mi?\n\n\
                 \"Hayır\" dersen Julie'yi yeniden kurduğunda hepsi geri gelir.",
            );
        default_browser::unregister();
        delete_tree(UNINSTALL_KEY);
        for folder in [&FOLDERID_Programs, &FOLDERID_Desktop] {
            if let Some(link) = known_folder(folder).map(|d| d.join("Julie.lnk")) {
                let _ = std::fs::remove_file(link);
            }
        }
        if remove_data
            && let Some(dir) = app_dir
            && let Err(e) = std::fs::remove_dir_all(dir)
        {
            crate::log::error(&format!("veri klasörü silinemedi: {e}"));
        }
        // Çalışan program kendi dosyasını silemez: bu süreç kapandıktan
        // sonra klasörü Windows'un komut satırı siler.
        if let Some(dir) = local_app_data().map(|l| super::install_dir(&l))
            && dir.exists()
        {
            delete_later(&dir);
        }
        tell("Julie kaldırıldı.", false);
    }

    fn delete_later(dir: &Path) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Klasör yolu yalnızca Julie'nin kendi kurulum klasörü; tırnak
        // içermez (Windows yollarında `"` olamaz).
        let command = format!(
            "ping -n 3 127.0.0.1 >NUL & rmdir /S /Q \"{}\"",
            dir.display()
        );
        let _ = std::process::Command::new("cmd.exe")
            .arg("/C")
            .raw_arg(command)
            .current_dir(std::env::temp_dir())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_only_for_downloaded_copies() {
        let local = Path::new(r"C:\Users\a\AppData\Local");
        let installed = Path::new(r"C:\Users\a\AppData\Local\Programs\Julie\julie.exe");
        let downloaded = Path::new(r"C:\Users\a\Downloads\julie.exe");
        assert!(should_offer(downloaded, local, None));
        assert!(!should_offer(installed, local, None));
        // Windows'ta harf farkı aynı dosyadır.
        let upper = Path::new(r"C:\USERS\a\AppData\Local\Programs\JULIE\julie.exe");
        assert!(!should_offer(upper, local, None));
        // cargo run
        assert!(!should_offer(
            Path::new(r"C:\kod\julie\target\release\julie.exe"),
            local,
            None
        ));
        // "Hayır" dendiyse aynı dosya bir daha sormaz, başkası sorar.
        assert!(!should_offer(downloaded, local, Some(downloaded)));
        assert!(should_offer(
            Path::new(r"C:\Users\a\Desktop\julie.exe"),
            local,
            Some(downloaded)
        ));
    }

    #[test]
    fn uninstall_entry_points_to_installed_exe() {
        let exe = Path::new(r"C:\L\Programs\Julie\julie.exe");
        let entry = uninstall_entry(exe, "0.2.0");
        let get = |k: &str| entry.iter().find(|(n, _)| *n == k).unwrap().1.clone();
        assert_eq!(get("DisplayVersion"), "0.2.0");
        assert_eq!(
            get("UninstallString"),
            r#""C:\L\Programs\Julie\julie.exe" --uninstall"#
        );
        assert_eq!(get("DisplayIcon"), r#""C:\L\Programs\Julie\julie.exe",0"#);
        assert_eq!(get("InstallLocation"), r"C:\L\Programs\Julie");
    }

    #[test]
    fn install_dir_is_per_user() {
        assert_eq!(
            installed_exe(Path::new("L")),
            Path::new("L")
                .join("Programs")
                .join("Julie")
                .join("julie.exe")
        );
    }
}
