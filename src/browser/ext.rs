//! Eklentileri WebView2 profiline kurar, açıp kapatır, kaldırır; eklentiler
//! sayfasını (`julie://extensions`) günceller. Dosya işleri
//! `crate::extensions` içinde.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    let toolbar = dir
        .as_deref()
        .zip(manifest.as_ref())
        .map(|(d, m)| extensions::Toolbar::from_manifest(d, m));
    Entry {
        toolbar,
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
            let _ = proxy.send_event(UserEvent::ExtensionStaged(Box::new(result)));
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

/// Eklenti menüsündeki satırın işi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuRow {
    /// Eklentinin penceresini aç (menüden).
    Open(String),
    /// Araç çubuğuna sabitle / sabitlemeyi kaldır.
    Pin(String),
    /// Seçenekler sayfasını aç.
    Options(String),
    /// `julie://extensions`.
    Manage,
}

/// Araç çubuğundaki eklenti simgesinin anahtarı (`favicons` içinde).
fn icon_key(id: &str) -> String {
    format!("ext:{id}")
}

impl Browser {
    /// Araç çubuğunda gösterilecek eklentiler: açık ve dosyaları bulunan.
    fn toolbar_entries(&self) -> Vec<Entry> {
        if self.private {
            return Vec::new();
        }
        self.extensions
            .lock()
            .unwrap()
            .entries
            .iter()
            .filter(|e| e.enabled && e.toolbar.is_some())
            .cloned()
            .collect()
    }

    /// Sabitlenmiş ve hâlâ kurulu/açık eklentiler, sırasıyla.
    fn pinned_entries(&self) -> Vec<Entry> {
        let entries = self.toolbar_entries();
        self.settings
            .extension_pins
            .iter()
            .filter_map(|id| entries.iter().find(|e| e.id == *id).cloned())
            .collect()
    }

    /// Arayüz modeline yapboz düğmesini ve sabitlenmiş eklentileri ekler.
    pub(super) fn with_extension_buttons(
        &self,
        mut model: crate::ui::layout::Model,
    ) -> crate::ui::layout::Model {
        model.extensions = !self.toolbar_entries().is_empty();
        model.pinned = self
            .pinned_entries()
            .into_iter()
            .map(|e| crate::ui::layout::ExtensionButton {
                icon: e
                    .toolbar
                    .as_ref()
                    .and_then(|t| t.icon.as_ref())
                    .map(|_| icon_key(&e.id)),
                title: e.name,
            })
            .collect();
        model
    }

    /// Eklenti simgelerini arayüze verir ve araç çubuğunu yeniler.
    pub(super) fn sync_extension_toolbar(&mut self) {
        for entry in self.toolbar_entries() {
            if let Some(icon) = entry.toolbar.and_then(|t| t.icon) {
                self.chrome
                    .borrow_mut()
                    .set_favicon(&icon_key(&entry.id), icon);
            }
        }
        self.sync_chrome();
    }

    /// Yapboz düğmesi: kurulu eklentiler (raptiyeleriyle) ve "yönet".
    pub(super) fn open_extensions_menu(&mut self) {
        let pins = &self.settings.extension_pins;
        let mut entries = Vec::new();
        let mut rows = Vec::new();
        for entry in self.toolbar_entries() {
            entries.push(crate::ui::layout::MenuEntry::Extension {
                label: entry.name.clone(),
                icon: entry
                    .toolbar
                    .as_ref()
                    .and_then(|t| t.icon.as_ref())
                    .map(|_| icon_key(&entry.id)),
                pinned: pins.contains(&entry.id),
            });
            rows.push(Some(MenuRow::Open(entry.id)));
        }
        if !entries.is_empty() {
            entries.push(crate::ui::layout::MenuEntry::Separator);
            rows.push(None);
        }
        entries.push(crate::ui::layout::MenuEntry::item(
            "Eklentileri yönet",
            None,
            true,
        ));
        rows.push(Some(MenuRow::Manage));
        self.tab_menu = None;
        self.group_menu = None;
        self.extension_menu = Some(rows);
        self.chrome.borrow_mut().show_menu(entries, "");
    }

    /// Sabitlenmiş eklentiye sağ tık.
    pub(super) fn open_extension_button_menu(&mut self, index: usize) {
        let Some(entry) = self.pinned_entries().into_iter().nth(index) else {
            return;
        };
        let has_options = entry.toolbar.as_ref().is_some_and(|t| t.options.is_some());
        let entries = vec![
            crate::ui::layout::MenuEntry::item("Seçenekler", None, has_options),
            crate::ui::layout::MenuEntry::item("Araç çubuğundan kaldır", None, true),
            crate::ui::layout::MenuEntry::Separator,
            crate::ui::layout::MenuEntry::item("Eklentileri yönet", None, true),
        ];
        let rows = vec![
            Some(MenuRow::Options(entry.id.clone())),
            Some(MenuRow::Pin(entry.id)),
            None,
            Some(MenuRow::Manage),
        ];
        self.tab_menu = None;
        self.group_menu = None;
        self.extension_menu = Some(rows);
        self.chrome.borrow_mut().show_menu(entries, "");
    }

    pub(super) fn extension_menu_choice(&mut self, row: usize) {
        // Raptiye: menü açık kalır.
        if row >= crate::ui::layout::PIN_HIT {
            let id = self.extension_menu.as_ref().and_then(|rows| {
                rows.get(row - crate::ui::layout::PIN_HIT)
                    .cloned()
                    .flatten()
            });
            if let Some(MenuRow::Open(id)) = id {
                self.toggle_pin(&id);
            }
            return;
        }
        let Some(Some(choice)) = self
            .extension_menu
            .take()
            .and_then(|rows| rows.get(row).cloned())
        else {
            return;
        };
        match choice {
            MenuRow::Open(id) => self.open_extension(&id, crate::ui::layout::Target::Extensions),
            MenuRow::Pin(id) => self.toggle_pin(&id),
            MenuRow::Options(id) => {
                let page = self
                    .toolbar_entries()
                    .into_iter()
                    .find(|e| e.id == id)
                    .and_then(|e| e.toolbar?.options);
                if let Some(page) = page {
                    self.open_tab(&extensions::page_url(&id, &page));
                }
            }
            MenuRow::Manage => self.show_extensions_page(),
        }
    }

    fn toggle_pin(&mut self, id: &str) {
        let pins = &mut self.settings.extension_pins;
        if let Some(at) = pins.iter().position(|p| p == id) {
            pins.remove(at);
        } else {
            pins.push(id.to_string());
        }
        self.save_settings();
        self.sync_chrome();
    }

    pub(super) fn extension_click(&mut self, index: usize) {
        let Some(entry) = self.pinned_entries().into_iter().nth(index) else {
            return;
        };
        self.open_extension(&entry.id, crate::ui::layout::Target::Extension(index));
    }

    /// Eklentinin penceresini düğmenin yanında açar; penceresi yoksa
    /// seçenekler sayfasını, o da yoksa eklentiler sayfasını.
    fn open_extension(&mut self, id: &str, anchor: crate::ui::layout::Target) {
        // Aynı düğmeye basarak kapatıldı (dışarı tıklama önce geldi).
        if self
            .popup_closed
            .as_ref()
            .is_some_and(|(closed, at)| closed == id && at.elapsed().as_millis() < 400)
        {
            self.popup_closed = None;
            return;
        }
        let was_open = self.extension_popup.as_ref().is_some_and(|p| p.id == id);
        self.close_extension_popup();
        if was_open {
            return;
        }
        let Some(toolbar) = self
            .toolbar_entries()
            .into_iter()
            .find(|e| e.id == id)
            .and_then(|e| e.toolbar)
        else {
            return;
        };
        let Some(page) = toolbar.popup else {
            match toolbar.options {
                Some(options) => self.open_tab(&extensions::page_url(id, &options)),
                None => self.show_extensions_page(),
            }
            return;
        };
        let anchor = self.chrome.borrow().anchor(anchor).unwrap_or_else(|| {
            let pos = self.window.outer_position().unwrap_or_default();
            let size = self.window.outer_size();
            let right = pos.x + size.width as i32 - 16;
            (right - 32, pos.y + 40, right, pos.y + 72)
        });
        let dark = self.chrome.borrow().theme().dark;
        let scale = self.window.scale_factor();
        match super::popup::Popup::open(
            &mut self.context,
            self.window.hwnd(),
            self.proxy.clone(),
            id,
            &extensions::page_url(id, &page),
            anchor,
            scale,
            dark,
        ) {
            Ok(popup) => self.extension_popup = Some(popup),
            Err(err) => crate::log::error(&format!("eklenti penceresi açılamadı: {err}")),
        }
    }

    pub(super) fn close_extension_popup(&mut self) {
        if let Some(popup) = self.extension_popup.take() {
            self.popup_closed = Some((popup.id.clone(), Instant::now()));
        }
    }

    /// Sayfa yüklendi: boyutunu bildirmediyse sorulur.
    pub(super) fn measure_extension_popup(&self) {
        let Some(webview) = self.extension_popup.as_ref().and_then(|p| p.webview()) else {
            return;
        };
        let proxy = self.proxy.clone();
        let serial = self.extension_popup.as_ref().map_or(0, |p| p.serial);
        let _ = webview.evaluate_script_with_callback(super::popup::SIZE_QUERY, move |json| {
            let size: Option<(u32, u32)> = serde_json::from_str::<Vec<f64>>(&json)
                .ok()
                .filter(|v| v.len() == 2)
                .map(|v| (v[0] as u32, v[1] as u32));
            if let Some((w, h)) = size.filter(|(w, h)| *w > 0 && *h > 0) {
                let _ = proxy.send_event(UserEvent::ExtensionPopupSize(serial, w, h));
            }
        });
    }
}
