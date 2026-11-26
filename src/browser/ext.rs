//! Eklentileri WebView2 profiline kurar, açıp kapatır, kaldırır; eklentiler
//! sayfasını (`julie://extensions`) günceller. Dosya işleri
//! `crate::extensions` içinde.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_13, ICoreWebView2BrowserExtension, ICoreWebView2Profile7,
};
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
    take_pwstr,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::core::{BOOL, HSTRING, Interface, PWSTR, w};

use tao::platform::windows::WindowExtWindows;
use wry::WebViewExtWindows;

use super::Browser;
use crate::UserEvent;
use crate::address::{self, EXTENSIONS_URL};
use crate::extensions::{self, Command, Entry, Pending};

/// Eklentiler sayfasının durumu (sayfayı üreten iş parçacığı da okur).
pub type Shared = Arc<Mutex<extensions::State>>;

/// Eklentiye uygulanacak işlem (profilden bulunduktan sonra).
type ExtensionJob = Box<dyn FnOnce(ICoreWebView2BrowserExtension) -> windows::core::Result<()>>;

/// Mağazaya bildirilen Chromium sürümü: WebView2'nin kendi sürümü.
fn browser_version() -> String {
    wry::webview_version()
        .ok()
        .and_then(|v| v.split_whitespace().next().map(str::to_string))
        .filter(|v| extensions::is_version(v))
        .unwrap_or_else(|| "131.0.0.0".into())
}

fn text(read: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut value = PWSTR::null();
    match read(&mut value) {
        Ok(()) => take_pwstr(value),
        Err(_) => String::new(),
    }
}

/// Listedeki eklentinin satırı: adı ve durumu profilden, sürümü ve simgesi
/// klasöründeki `manifest.json`'dan.
fn entry_of(ext: &ICoreWebView2BrowserExtension, root: Option<&Path>) -> Entry {
    let id = text(|v| unsafe { ext.Id(v) });
    let name = text(|v| unsafe { ext.Name(v) });
    let mut enabled = BOOL(1);
    unsafe {
        let _ = ext.IsEnabled(&mut enabled);
    }
    let store = root.and_then(|r| extensions::installed_dir(r, &id));
    let folder = root.and_then(|r| {
        extensions::read_folders(r)
            .into_iter()
            .find(|(i, _)| *i == id)
            .map(|(_, path)| path)
    });
    let from_folder = store.is_none() && folder.is_some();
    let dir = store.as_ref().map(|(_, dir)| dir.clone()).or(folder);
    let manifest = dir
        .as_deref()
        .and_then(|d| extensions::read_manifest(d).ok());
    Entry {
        icon: dir.as_deref().and_then(|d| {
            extensions::icon_data(d, manifest.as_ref().and_then(|m| m.icon.as_deref()))
        }),
        name: if name.is_empty() {
            manifest
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or(id.clone())
        } else {
            name
        },
        version: manifest
            .as_ref()
            .map(|m| m.version.clone())
            .unwrap_or_default(),
        description: manifest.map(|m| m.description).unwrap_or_default(),
        from_store: store.is_some(),
        from_folder,
        update: None,
        enabled: enabled.as_bool(),
        id,
    }
}

/// Klasör seçme penceresi.
fn pick_folder(owner: isize) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;
        dialog.SetTitle(w!("Eklentinin klasörünü seç")).ok()?;
        dialog.Show(Some(HWND(owner as _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        path.map(PathBuf::from)
    }
}

impl Browser {
    /// Ortak profil (eklentiler bütün sekmelerde aynı). Gizli pencerede yok.
    fn extension_profile(&self) -> Option<ICoreWebView2Profile7> {
        if self.private {
            return None;
        }
        let webview = self.tabs.iter().find_map(|t| t.webview.as_ref())?;
        unsafe {
            webview
                .webview()
                .cast::<ICoreWebView2_13>()
                .ok()?
                .Profile()
                .ok()?
                .cast()
                .ok()
        }
    }

    fn with_extension_state(&self, change: impl FnOnce(&mut extensions::State)) {
        change(&mut self.extensions.lock().unwrap());
        self.push_extensions();
    }

    /// Açık eklentiler sayfalarına yeni durumu gönderir.
    pub(super) fn push_extensions(&self) {
        let json = HSTRING::from(self.extensions.lock().unwrap().json().to_string());
        for tab in &self.tabs {
            if address::internal_page(&tab.url) == Some("extensions")
                && let Some(webview) = &tab.webview
            {
                unsafe {
                    let _ = webview.webview().PostWebMessageAsString(&json);
                }
            }
        }
    }

    /// Eklenti listesini profilden yeniden okur.
    pub(super) fn refresh_extensions(&self) {
        let Some(profile) = self.extension_profile() else {
            self.push_extensions();
            return;
        };
        let state = Arc::clone(&self.extensions);
        let root = self.extensions_dir.clone();
        let proxy = self.proxy.clone();
        let done =
            ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |result, list| {
                let mut entries = Vec::new();
                if let (Ok(()), Some(list)) = (result, list) {
                    let mut count = 0u32;
                    unsafe { list.Count(&mut count)? };
                    for i in 0..count {
                        let ext = unsafe { list.GetValueAtIndex(i)? };
                        entries.push(entry_of(&ext, root.as_deref()));
                    }
                }
                let mut state = state.lock().unwrap();
                for entry in &mut entries {
                    entry.update = state
                        .entries
                        .iter()
                        .find(|old| old.id == entry.id)
                        .and_then(|old| old.update.clone())
                        .filter(|new| {
                            entry.from_store && extensions::is_newer(new, &entry.version)
                        });
                }
                // WebView2'nin kendi parçaları (pano, PDF görüntüleyici)
                // listelenmez: kaldırılırlarsa motorun özellikleri bozulur.
                entries
                    .retain(|e| e.from_store || e.from_folder || !extensions::is_builtin(&e.name));
                entries.sort_by_key(|e| e.name.to_lowercase());
                state.entries = entries;
                drop(state);
                let _ = proxy.send_event(UserEvent::ExtensionsChanged);
                Ok(())
            }));
        unsafe {
            if let Err(err) = profile.GetBrowserExtensions(&done) {
                crate::log::error(&format!("eklentiler okunamadı: {err}"));
            }
        }
    }

    /// Profildeki eklentiyi bulup işlemi uygular, sonra listeyi yeniler.
    fn with_extension(&self, id: String, job: ExtensionJob) {
        let Some(profile) = self.extension_profile() else {
            return;
        };
        let proxy = self.proxy.clone();
        let mut job = Some(job);
        let done =
            ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |result, list| {
                if let (Ok(()), Some(list)) = (result, list) {
                    let mut count = 0u32;
                    unsafe { list.Count(&mut count)? };
                    for i in 0..count {
                        let ext = unsafe { list.GetValueAtIndex(i)? };
                        if text(|v| unsafe { ext.Id(v) }) == id
                            && let Some(job) = job.take()
                        {
                            if let Err(err) = job(ext) {
                                crate::log::error(&format!("eklenti değiştirilemedi: {err}"));
                                let _ = proxy.send_event(UserEvent::ExtensionsRefresh);
                            }
                            return Ok(());
                        }
                    }
                }
                let _ = proxy.send_event(UserEvent::ExtensionsRefresh);
                Ok(())
            }));
        unsafe {
            let _ = profile.GetBrowserExtensions(&done);
        }
    }

    /// Eklentiler sayfası açıksa öne getirir, değilse açar (yenilemeden:
    /// sayfa durumu mesajla alır).
    fn show_extensions_page(&mut self) {
        match self
            .tabs
            .iter()
            .position(|t| address::internal_page(&t.url) == Some("extensions"))
        {
            Some(index) => self.activate(index),
            None => self.open_tab(EXTENSIONS_URL),
        }
    }

    pub(super) fn extension_command(&mut self, command: Command) {
        if self.private {
            self.push_extensions();
            return;
        }
        match command {
            Command::Opened => {
                self.refresh_extensions();
                if !self.extensions.lock().unwrap().checked {
                    self.check_extension_updates(false);
                }
            }
            Command::Store => self.open_tab(extensions::STORE_URL),
            Command::Add(text) => match extensions::find_id(&text) {
                Some(id) => self.offer_extension(id, false),
                None => self.with_extension_state(|s| {
                    s.notice = None;
                    s.error = Some(
                        "Bu bir eklenti adresi değil. Chrome Web Mağazası'ndaki eklentinin sayfasının adresini yapıştır."
                            .into(),
                    );
                }),
            },
            Command::Update(id) => self.offer_extension(id, true),
            Command::Install => self.install_pending(),
            Command::Cancel => self.with_extension_state(|s| {
                if let Some(pending) = s.pending.take() {
                    let _ = std::fs::remove_dir_all(&pending.dir);
                }
            }),
            Command::Toggle(id) => {
                let proxy = self.proxy.clone();
                self.with_extension(
                    id,
                    Box::new(move |ext| {
                        let mut enabled = BOOL(1);
                        unsafe { ext.IsEnabled(&mut enabled)? };
                        let done = BrowserExtensionEnableCompletedHandler::create(Box::new(
                            move |_| {
                                let _ = proxy.send_event(UserEvent::ExtensionsRefresh);
                                Ok(())
                            },
                        ));
                        unsafe { ext.Enable(!enabled.as_bool(), &done) }
                    }),
                );
            }
            Command::Remove(id) => self.remove_extension(id),
            Command::Folder => self.load_extension_folder(),
            Command::Check => self.check_extension_updates(true),
        }
    }

    /// Mağazadaki eklentiyi indirir; açılınca onay sorulur.
    pub(super) fn offer_extension(&mut self, id: String, update: bool) {
        if self.private {
            return;
        }
        let Some(root) = self.extensions_dir.clone() else {
            return;
        };
        {
            let mut state = self.extensions.lock().unwrap();
            if state.busy.is_some() {
                return;
            }
            if let Some(pending) = state.pending.take() {
                let _ = std::fs::remove_dir_all(&pending.dir);
            }
            state.busy = Some(if update {
                "Güncelleme indiriliyor…".into()
            } else {
                "Eklenti indiriliyor…".into()
            });
            state.error = None;
            state.notice = None;
        }
        self.show_extensions_page();
        self.push_extensions();
        let proxy = self.proxy.clone();
        let installed = self
            .extensions
            .lock()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.id == id && e.from_store)
            .map(|e| e.version.clone());
        std::thread::spawn(move || {
            let staging = root.join(".staging").join(&id);
            let result = extensions::net::download(&id, &browser_version())
                .and_then(|bytes| extensions::unpack(&bytes, &id, &staging))
                .map(|manifest| Pending {
                    icon: extensions::icon_data(&staging, manifest.icon.as_deref()),
                    update: update || installed.is_some(),
                    id: id.clone(),
                    manifest,
                    dir: staging.clone(),
                });
            // Kurulu sürümle aynı (ya da eski): soracak bir şey yok.
            let result = match (result, installed) {
                (Ok(pending), Some(old))
                    if !extensions::is_newer(&pending.manifest.version, &old) =>
                {
                    let _ = std::fs::remove_dir_all(&pending.dir);
                    Err(format!(
                        "{} zaten kurulu ve en yeni sürümde.",
                        pending.manifest.name
                    ))
                }
                (result, _) => result,
            };
            let _ = proxy.send_event(UserEvent::ExtensionStaged(result));
        });
    }

    pub(super) fn extension_staged(&mut self, result: Result<Pending, String>) {
        self.with_extension_state(|s| {
            s.busy = None;
            match result {
                Ok(pending) => s.pending = Some(pending),
                Err(err) if err.contains("zaten kurulu") => s.notice = Some(err),
                Err(err) => s.error = Some(err),
            }
        });
    }

    /// Onaylanan eklentiyi yerine taşır ve profile ekler.
    fn install_pending(&mut self) {
        let (Some(root), Some(profile)) = (self.extensions_dir.clone(), self.extension_profile())
        else {
            return;
        };
        let Some(pending) = self.extensions.lock().unwrap().pending.take() else {
            return;
        };
        let target = root.join(&pending.id).join(&pending.manifest.version);
        let moved = (|| {
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            std::fs::create_dir_all(root.join(&pending.id))?;
            std::fs::rename(&pending.dir, &target)
        })();
        if let Err(err) = moved {
            let _ = std::fs::remove_dir_all(&pending.dir);
            self.with_extension_state(|s| s.error = Some(format!("eklenti kurulamadı: {err}")));
            return;
        }
        self.with_extension_state(|s| s.busy = Some("Ekleniyor…".into()));

        let state = Arc::clone(&self.extensions);
        let proxy = self.proxy.clone();
        let name = pending.manifest.name.clone();
        let version = pending.manifest.version.clone();
        let id = pending.id.clone();
        let done =
            ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |result, _| {
                {
                    let mut s = state.lock().unwrap();
                    s.busy = None;
                    match result {
                        Ok(()) => {
                            extensions::remove_old_versions(&root, &id, &version);
                            s.notice = Some(if pending.update {
                                format!("{name} güncellendi.")
                            } else {
                                format!("{name} eklendi.")
                            });
                        }
                        Err(err) => {
                            crate::log::error(&format!("eklenti eklenemedi: {err}"));
                            // Güncellemede eski sürüm çalışmaya devam eder.
                            if !pending.update {
                                let _ = std::fs::remove_dir_all(root.join(&id));
                            } else {
                                let _ = std::fs::remove_dir_all(root.join(&id).join(&version));
                            }
                            s.error = Some(format!("{name} eklenemedi ({}).", err.message()));
                        }
                    }
                }
                let _ = proxy.send_event(UserEvent::ExtensionsRefresh);
                Ok(())
            }));
        unsafe {
            if let Err(err) = profile.AddBrowserExtension(&HSTRING::from(target.as_os_str()), &done)
            {
                self.with_extension_state(|s| {
                    s.busy = None;
                    s.error = Some(format!("eklenti eklenemedi ({}).", err.message()));
                });
            }
        }
    }

    fn remove_extension(&mut self, id: String) {
        let root = self.extensions_dir.clone();
        let state = Arc::clone(&self.extensions);
        let proxy = self.proxy.clone();
        let name = self
            .extensions
            .lock()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let target = id.clone();
        self.with_extension(
            id,
            Box::new(move |ext| {
                let done =
                    BrowserExtensionRemoveCompletedHandler::create(Box::new(move |result| {
                        if result.is_ok()
                            && let Some(root) = &root
                        {
                            let _ = std::fs::remove_dir_all(root.join(&target));
                            let mut folders = extensions::read_folders(root);
                            folders.retain(|(i, _)| *i != target);
                            extensions::write_folders(root, &folders);
                        }
                        {
                            let mut s = state.lock().unwrap();
                            s.error = None;
                            s.notice = Some(match result {
                                Ok(()) => format!("{name} kaldırıldı."),
                                Err(err) => format!("{name} kaldırılamadı ({}).", err.message()),
                            });
                        }
                        let _ = proxy.send_event(UserEvent::ExtensionsRefresh);
                        Ok(())
                    }));
                unsafe { ext.Remove(&done) }
            }),
        );
    }

    /// Bilgisayardaki açılmış bir eklentiyi (geliştirici gibi) yükler.
    fn load_extension_folder(&mut self) {
        let (Some(root), Some(profile)) = (self.extensions_dir.clone(), self.extension_profile())
        else {
            return;
        };
        let Some(folder) = pick_folder(self.window.hwnd()) else {
            return;
        };
        let manifest = match extensions::read_manifest(&folder) {
            Ok(m) => m,
            Err(err) => {
                self.with_extension_state(|s| {
                    s.notice = None;
                    s.error = Some(format!("Bu klasör bir eklenti değil: {err}."));
                });
                return;
            }
        };
        let state = Arc::clone(&self.extensions);
        let proxy = self.proxy.clone();
        let path = folder.clone();
        let done =
            ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |result, ext| {
                {
                    let mut s = state.lock().unwrap();
                    match (result, ext) {
                        (Ok(()), Some(ext)) => {
                            let id = text(|v| unsafe { ext.Id(v) });
                            let mut folders = extensions::read_folders(&root);
                            folders.retain(|(i, _)| *i != id);
                            folders.push((id, path.clone()));
                            extensions::write_folders(&root, &folders);
                            s.error = None;
                            s.notice = Some(format!("{} eklendi.", manifest.name));
                        }
                        (Err(err), _) => {
                            s.notice = None;
                            s.error =
                                Some(format!("{} eklenemedi ({}).", manifest.name, err.message()));
                        }
                        _ => {}
                    }
                }
                let _ = proxy.send_event(UserEvent::ExtensionsRefresh);
                Ok(())
            }));
        unsafe {
            let _ = profile.AddBrowserExtension(&HSTRING::from(folder.as_os_str()), &done);
        }
    }

    /// Mağazadan kurulanların yeni sürümü var mı? Güncelleme kendiliğinden
    /// kurulmaz: sayfada "Güncelle" düğmesi çıkar.
    fn check_extension_updates(&mut self, asked: bool) {
        let installed: Vec<(String, String)> = {
            let mut state = self.extensions.lock().unwrap();
            state.checked = true;
            if asked {
                if state.busy.is_some() {
                    return;
                }
                state.busy = Some("Güncellemeler denetleniyor…".into());
                state.error = None;
                state.notice = None;
            }
            state
                .entries
                .iter()
                .filter(|e| e.from_store && extensions::is_version(&e.version))
                .map(|e| (e.id.clone(), e.version.clone()))
                .collect()
        };
        self.push_extensions();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = if installed.is_empty() {
                Ok(Vec::new())
            } else {
                extensions::net::check_updates(&installed, &browser_version())
            };
            let _ = proxy.send_event(UserEvent::ExtensionUpdates(result, asked));
        });
    }

    pub(super) fn extension_updates(
        &mut self,
        result: Result<Vec<(String, String)>, String>,
        asked: bool,
    ) {
        self.with_extension_state(|s| {
            if asked {
                s.busy = None;
            }
            match result {
                Ok(updates) => {
                    for entry in &mut s.entries {
                        if let Some((_, version)) = updates.iter().find(|(id, _)| *id == entry.id) {
                            entry.update = Some(version.clone());
                        }
                    }
                    if asked {
                        s.notice = Some(match updates.len() {
                            0 => "Bütün eklentiler güncel.".into(),
                            1 => "Bir eklentinin yeni sürümü var.".into(),
                            n => format!("{n} eklentinin yeni sürümü var."),
                        });
                    }
                }
                Err(err) if asked => s.error = Some(err),
                Err(err) => crate::log::error(&err),
            }
        });
    }
}
