//! Pencere, sekmeler ve olay döngüsü.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowExtWindows;
use tao::window::{Window, WindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, ICoreWebView2_3, ICoreWebView2_8,
    ICoreWebView2Settings8,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, IsDocumentPlayingAudioChangedEventHandler,
    SourceChangedEventHandler, TrySuspendCompletedHandler, take_pwstr,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::core::{BOOL, Interface, PWSTR};
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use wry::http::{Response, StatusCode};
use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
};

use crate::UserEvent;
use crate::address::{self, HISTORY_URL, SETTINGS_URL, START_URL};
use crate::blocking::{self, Blocker};
use crate::chrome::Chrome;
use crate::cosmetic;
use crate::history_page;
use crate::places::{self, Places};
use crate::request_filter::{self, TabState};
use crate::scriptlet_injector;
use crate::session::{SavedTab, Session};
use crate::settings::{self, Engine, Settings};
use crate::settings_page;
use crate::shortcuts::{self, Modifiers};
use crate::sleep::{self, Action, Power};
use crate::start_page::{self, Message};
use crate::ui::layout::{BookmarkInfo, Model, TabInfo};

/// WebView2 tarayıcı sürecine verilen argümanlar. Aynı profili kullanan
/// bütün WebView'lerde birebir aynı olmalı, yoksa WebView2 oluşturulamaz.
///
/// - `--process-per-site`: Aynı sitenin sekmeleri tek renderer sürecini
///   paylaşır (farklı siteler yine ayrı süreçlerde, güvenlik aynı kalır).
///   Aynı siteden çok sekme açıldığında belleği belirgin düşürür.
/// - `msWebOOUI`, `msPdfOOUI`: Edge'in metin seçince çıkan mini menüsü.
/// - `MediaRouter`, `DialMediaRouteProvider`: Ağda Chromecast arayan arka
///   plan taraması (CPU ve ağ harcar, tarayıcımızda yayın özelliği yok).
/// - `AutofillServerCommunication`: Form alanı bilgilerinin sunucuya
///   gönderilmesi (otomatik doldurmanın kendisi çalışmaya devam eder).
///
/// SmartScreen burada kapatılmıyor; sekme başına ayardan yönetiliyor.
const BROWSER_ARGS: &str = "--process-per-site \
    --disable-features=msWebOOUI,msPdfOOUI,MediaRouter,DialMediaRouteProvider,AutofillServerCommunication";

struct Tab {
    id: u64,
    /// Sekme bellekten atıldıysa (uyutulduysa) `None`.
    webview: Option<WebView>,
    title: String,
    url: String,
    filter: TabState,
    power: Power,
    /// Sekme ne zamandan beri arka planda (öndeyse `None`).
    hidden_since: Option<Instant>,
    /// Geçmişe en son kaydedilen adres (aynı adres art arda kaydedilmesin).
    recorded_url: String,
    /// Sekmede ses çalıyor mu (sekme şeridindeki hoparlör ikonu).
    audio: bool,
    /// Geçerli sayfada genel gizleme kurallarından muaf seçiciler.
    cosmetic_exceptions: HashSet<String>,
    /// Geçerli sayfada genel gizleme kuralları kapalı mı (`$generichide`)?
    generichide: bool,
}

struct Browser {
    window: Window,
    chrome: Rc<RefCell<Chrome>>,
    context: WebContext,
    proxy: EventLoopProxy<UserEvent>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    /// Ctrl+Shift+T için son kapatılan sekmelerin adresleri.
    closed: Vec<String>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    /// Başlangıç sayfasını üreten işleyici WebView2'nin iş parçacığından
    /// çağrılabildiği için seçili motor burada paylaşılır.
    start_state: Arc<Mutex<start_page::State>>,
    filter: request_filter::Shared,
    /// Filtre listeleri indirilemediyse `true` (buton bunu gösterir).
    filters_error: bool,
    sleep_timing: sleep::Timing,
    session_path: Option<PathBuf>,
    /// Geçmiş ve yer imleri. Açılamazsa (disk hatası) `None`; tarayıcı
    /// yine çalışır, yalnızca bu özellikler devre dışı kalır.
    places: Option<Arc<Mutex<Places>>>,
    /// Son kayıttan beri sekmelerde değişiklik oldu mu?
    session_dirty: bool,
    /// Arayüzde gösterilen durum (her olayda yeniden hesaplanmasın diye).
    bookmarks: Vec<BookmarkInfo>,
    can_back: bool,
    can_forward: bool,
    star: (bool, bool),
    shield: (String, bool),
}

pub fn run() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Julie")
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        .build(&event_loop)
        .expect("pencere oluşturulamadı");

    let app_dir = settings::app_dir();
    let settings_path = app_dir.as_ref().map(|dir| dir.join("settings.ini"));
    let session_path = app_dir.as_ref().map(|dir| dir.join("session.txt"));
    let places = app_dir
        .as_ref()
        .and_then(|dir| open_places(&dir.join("places.sqlite")));
    let settings = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();

    let size = window.inner_size();
    let chrome = Chrome::create(
        window.hwnd() as _,
        proxy.clone(),
        settings.appearance,
        window.scale_factor() as f32,
        (size.width, size.height),
    );
    let system_accent = chrome.borrow().system_accent();

    let filter = request_filter::Shared::default();
    *filter.disabled.borrow_mut() = settings.blocking_disabled.clone();
    if let Some(dir) = &app_dir {
        load_filters(&filter, &blocking::cache_path(dir), &proxy);
    }

    let mut browser = Browser {
        window,
        chrome,
        context: WebContext::new(app_dir.map(|dir| dir.join("WebView2"))),
        proxy,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
        closed: Vec::new(),
        start_state: Arc::new(Mutex::new(start_page::State {
            engine: settings.engine,
            smartscreen: !settings.smartscreen_off,
            appearance: settings.appearance,
            system_accent,
        })),
        settings,
        settings_path,
        filter,
        filters_error: false,
        sleep_timing: sleep::Timing::from_env(),
        session_path,
        places,
        session_dirty: false,
        bookmarks: Vec::new(),
        can_back: false,
        can_forward: false,
        star: (false, false),
        shield: (String::new(), false),
    };
    browser.reload_bookmarks();
    browser.layout();
    browser.restore_session();
    start_ticker(browser.proxy.clone(), browser.sleep_timing.check_every);
    browser.chrome.borrow().focus_address();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    browser.save_session();
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    browser.layout()
                }
                _ => {}
            },
            Event::UserEvent(event) => {
                if browser.handle(event) == Flow::Exit {
                    browser.save_session();
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

/// Önbellekteki derlenmiş kuralları yükler. Önbellek yoksa ya da eskiyse
/// listeleri arka planda indirir; hazır olunca `FiltersReady` gelir.
fn load_filters(
    filter: &request_filter::Shared,
    cache: &std::path::Path,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let cached = std::fs::read(cache)
        .ok()
        .and_then(|b| Blocker::deserialize(&b));
    let have_cache = cached.is_some();
    *filter.blocker.borrow_mut() = cached;

    if !have_cache || blocking::needs_update(cache) {
        let proxy = proxy.clone();
        let cache = cache.to_path_buf();
        std::thread::spawn(move || match blocking::update(&cache) {
            Ok(bytes) => {
                let _ = proxy.send_event(UserEvent::FiltersReady(bytes));
            }
            Err(err) => {
                let _ = proxy.send_event(UserEvent::FiltersFailed(err));
            }
        });
    }
}

/// Uyutma kontrolü için olay döngüsüne periyodik `Tick` gönderir.
fn start_ticker(proxy: EventLoopProxy<UserEvent>, every: std::time::Duration) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(every);
            // Olay döngüsü kapandıysa iş parçacığı da biter.
            if proxy.send_event(UserEvent::Tick).is_err() {
                break;
            }
        }
    });
}

/// SmartScreen zararlı site korumasını sekmede açar/kapatır.
fn set_reputation_checking(webview: &WebView, on: bool) {
    unsafe {
        let _ = webview
            .webview()
            .Settings()
            .and_then(|settings| settings.cast::<ICoreWebView2Settings8>())
            .and_then(|settings| settings.SetIsReputationCheckingRequired(on));
    }
}

/// Sekmede ses ya da video çalıyor mu?
fn is_playing_audio(webview: &WebView) -> bool {
    unsafe {
        webview
            .webview()
            .cast::<ICoreWebView2_8>()
            .and_then(|core| {
                let mut playing = BOOL::default();
                core.IsDocumentPlayingAudio(&mut playing)?;
                Ok(playing.as_bool())
            })
            .unwrap_or(false)
    }
}

/// WebView2'den sayfayı dondurmasını ister (gizli olmalı).
fn try_suspend(webview: &WebView) -> bool {
    unsafe {
        webview
            .webview()
            .cast::<ICoreWebView2_3>()
            .and_then(|core| {
                core.TrySuspend(&TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(()))))
            })
            .is_ok()
    }
}

/// Ctrl+Shift+T ile geri açılabilecek en fazla sekme sayısı.
const MAX_CLOSED: usize = 25;

/// Geçmiş veritabanını açar ve süresi dolmuş kayıtları siler.
fn open_places(path: &std::path::Path) -> Option<Arc<Mutex<Places>>> {
    match Places::open(path) {
        Ok(p) => {
            let cutoff = places::now() - places::HISTORY_DAYS * 24 * 60 * 60;
            if let Err(err) = p.expire(cutoff) {
                crate::log::error(&format!("eski geçmiş silinemedi: {err}"));
            }
            Some(Arc::new(Mutex::new(p)))
        }
        Err(err) => {
            crate::log::error(&format!("geçmiş veritabanı açılamadı: {err}"));
            None
        }
    }
}

/// Sekmede ses çalmaya başlayınca ya da durunca haber verir.
fn listen_for_audio(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let Ok(core) = webview.webview().cast::<ICoreWebView2_8>() else {
        return;
    };
    let handler = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(core) = sender.and_then(|s| s.cast::<ICoreWebView2_8>().ok()) {
            let mut playing = BOOL::default();
            unsafe { core.IsDocumentPlayingAudio(&mut playing)? };
            let _ = proxy.send_event(UserEvent::AudioChanged(id, playing.as_bool()));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = core.add_IsDocumentPlayingAudioChanged(&handler, &mut token);
    }
}

/// Sayfanın adresi her değiştiğinde haber verir. Sayfa yüklemesinden farklı
/// olarak tek sayfalık uygulamaların (YouTube'da video değiştirmek gibi)
/// yeniden yükleme olmadan yaptığı adres değişikliklerini de yakalar.
fn listen_for_source_changes(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(core) = sender {
            let mut uri = PWSTR::null();
            unsafe { core.Source(&mut uri)? };
            let _ = proxy.send_event(UserEvent::UrlChanged(id, take_pwstr(uri)));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview.webview().add_SourceChanged(&handler, &mut token);
    }
}

/// Sayfa odaktayken basılan kısayolları yakalar. Bizim kısayolumuzsa
/// WebView2'nin kendi davranışını durdurur ve komutu olay döngüsüne yollar;
/// değilse tuş sayfaya (ya da WebView2'nin yerleşik kısayollarına) gider.
fn listen_for_shortcuts(webview: &WebView, proxy: EventLoopProxy<UserEvent>) {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        unsafe {
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                return Ok(());
            }
            let mut key = 0u32;
            args.VirtualKey(&mut key)?;
            let down = |vk: u16| GetKeyState(vk.into()) < 0;
            let modifiers = Modifiers {
                ctrl: down(VK_CONTROL.0),
                shift: down(VK_SHIFT.0),
                alt: down(VK_MENU.0),
            };
            if let Some(event) = shortcuts::lookup(key, modifiers) {
                args.SetHandled(true)?;
                let _ = proxy.send_event(event);
            }
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview
            .controller()
            .add_AcceleratorKeyPressed(&handler, &mut token);
    }
}

/// `julie://` isteklerini karşılar. Şimdilik tek sayfa var: başlangıç sayfası.
fn serve(
    uri: &wry::http::Uri,
    state: start_page::State,
    places: Option<&Mutex<Places>>,
) -> Response<Cow<'static, [u8]>> {
    // WebView2 adresi `julie://history/` ya da `http://julie.history/`
    // biçiminde verebilir; ikisinde de sayfa adı ana bilgisayar kısmıdır.
    let host = uri.host().unwrap_or_default();
    let name = host.strip_prefix("julie.").unwrap_or(host);
    let html = match (name, uri.path()) {
        ("start", "/") => {
            let bookmarks = places
                .and_then(|p| p.lock().ok()?.bookmarks().ok())
                .unwrap_or_default();
            start_page::render(state, &bookmarks)
        }
        ("history", "/") => {
            let query = history_page::query_from(uri.query());
            let entries = places
                .and_then(|p| p.lock().ok()?.history(&query, history_page::LIMIT).ok())
                .unwrap_or_default();
            history_page::render(&query, &entries)
        }
        ("settings", "/") => settings_page::render(&state.appearance, state.system_accent),
        _ => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Cow::Borrowed(&[][..]))
                .expect("geçerli yanıt");
        }
    };
    Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .header(
            "Content-Security-Policy",
            start_page::CONTENT_SECURITY_POLICY,
        )
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(html.into_bytes()))
        .expect("geçerli yanıt")
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Exit,
}

impl Browser {
    fn handle(&mut self, event: UserEvent) -> Flow {
        match event {
            // Butona tıklanınca odak butonda kalmasın, sayfaya dönsün.
            UserEvent::Back => {
                let webview = self.view();
                let _ = webview.go_back();
                let _ = webview.focus();
            }
            UserEvent::Forward => {
                let webview = self.view();
                let _ = webview.go_forward();
                let _ = webview.focus();
            }
            UserEvent::Reload => {
                let webview = self.view();
                let _ = webview.reload();
                let _ = webview.focus();
            }
            UserEvent::FocusAddress => self.chrome.borrow().focus_address(),
            UserEvent::DevTools => self.view().open_devtools(),
            UserEvent::ReopenTab => {
                if let Some(url) = self.closed.pop() {
                    self.open_tab(&url);
                }
            }
            UserEvent::NextTab => self.activate((self.active + 1) % self.tabs.len()),
            UserEvent::PrevTab => {
                self.activate((self.active + self.tabs.len() - 1) % self.tabs.len())
            }
            UserEvent::LastTab => self.activate(self.tabs.len() - 1),
            UserEvent::Navigate(text) => self.navigate(self.active, &text),
            UserEvent::FocusPage => {
                self.show_address(&self.current().url);
                let _ = self.view().focus();
            }
            UserEvent::NewTab => {
                self.open_tab(START_URL);
                self.chrome.borrow().focus_address();
            }
            UserEvent::StartPage(id, message) => match message {
                Message::Go(text) => {
                    if let Some(index) = self.index_of(id) {
                        self.navigate(index, &text);
                    }
                }
                Message::SetEngine(engine) => self.set_engine(engine),
                Message::SetSmartScreen(on) => self.set_smartscreen(on),
                Message::RemoveBookmark(url) => {
                    self.with_places(|p| p.remove_bookmark(&url));
                    self.reload_bookmarks();
                    self.update_star();
                }
                Message::DeleteHistory(url) => {
                    self.with_places(|p| p.delete_from_history(&url));
                }
                Message::ClearHistory => {
                    self.with_places(|p| p.clear_history());
                }
                Message::Appearance(key, value) => self.change_appearance(&key, &value),
            },
            UserEvent::ToggleBookmark => self.toggle_bookmark(),
            UserEvent::OpenHistory => self.open_internal("history", HISTORY_URL),
            UserEvent::OpenSettings => self.open_internal("settings", SETTINGS_URL),
            UserEvent::OpenBookmark(i) => {
                if let Some(url) = self.bookmarks.get(i).map(|b| b.url.clone()) {
                    let _ = self.view().load_url(&url);
                    let _ = self.view().focus();
                }
            }
            UserEvent::CloseTabAt(i) => {
                if i < self.tabs.len() {
                    return self.close_tab(i);
                }
            }
            UserEvent::SystemThemeChanged => {
                let changed = self.chrome.borrow_mut().refresh_system();
                if changed {
                    let accent = self.chrome.borrow().system_accent();
                    self.start_state.lock().unwrap().system_accent = accent;
                    self.after_theme_change();
                }
            }
            UserEvent::AudioChanged(id, playing) => {
                if let Some(index) = self.index_of(id) {
                    self.tabs[index].audio = playing;
                    self.sync_chrome();
                }
            }
            UserEvent::OpenInNewTab(url) => self.open_tab(&url),
            UserEvent::CloseTab => return self.close_tab(self.active),
            UserEvent::SelectTab(index) => self.activate(index),
            UserEvent::TitleChanged(id, title) => {
                if let Some(index) = self.index_of(id) {
                    if index == self.active {
                        self.set_window_title(&title);
                    }
                    let url = self.tabs[index].url.clone();
                    if places::is_recordable(&url) && !title.is_empty() {
                        self.with_places(|p| p.set_title(&url, &title));
                    }
                    self.tabs[index].title = title;
                    self.session_dirty = true;
                    self.sync_chrome();
                }
            }
            UserEvent::BlockedCountChanged(id) => {
                if self.index_of(id) == Some(self.active) {
                    self.update_shield();
                }
            }
            UserEvent::ToggleBlocking => self.toggle_blocking(),
            UserEvent::FiltersReady(bytes) => {
                match Blocker::deserialize(&bytes) {
                    Some(blocker) => {
                        *self.filter.blocker.borrow_mut() = Some(blocker);
                        self.filter.bump();
                    }
                    None => self.filters_failed("derlenen kurallar okunamadı".into()),
                }
                self.update_shield();
            }
            UserEvent::FiltersFailed(err) => self.filters_failed(err),
            UserEvent::Tick => {
                self.put_tabs_to_sleep();
                self.sync_chrome();
                if self.session_dirty {
                    self.save_session();
                }
            }
            UserEvent::Cosmetic(id, url, message) => self.apply_cosmetics(id, &url, message),
            UserEvent::UrlChanged(id, url) => {
                if let Some(index) = self.index_of(id) {
                    if index == self.active {
                        self.show_address(&url);
                    }
                    if places::is_recordable(&url) && self.tabs[index].recorded_url != url {
                        let time = places::now();
                        self.with_places(|p| p.record_visit(&url, time));
                        self.tabs[index].recorded_url = url.clone();
                    }
                    self.tabs[index].url = url;
                    self.session_dirty = true;
                    if index == self.active {
                        self.update_navigation();
                        self.update_star();
                    }
                }
            }
        }
        Flow::Continue
    }

    /// Adres çubuğu metnini çözümleyip sekmede açar.
    fn navigate(&self, index: usize, text: &str) {
        let search = self.settings.engine_or_fallback().search_prefix();
        let url = address::resolve(text, search);
        if index == self.active {
            self.show_address(&url);
        }
        if let Some(webview) = &self.tabs[index].webview {
            let _ = webview.load_url(&url);
            let _ = webview.focus();
        }
    }

    fn set_engine(&mut self, engine: Engine) {
        self.settings.engine = Some(engine);
        self.start_state.lock().unwrap().engine = Some(engine);
        self.save_settings();
    }

    fn set_smartscreen(&mut self, on: bool) {
        self.settings.smartscreen_off = !on;
        self.start_state.lock().unwrap().smartscreen = on;
        // Açık sekmelere hemen uygula; yeniden başlatma gerekmez.
        for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            set_reputation_checking(webview, on);
        }
        self.save_settings();
    }

    fn save_settings(&self) {
        if let Some(path) = &self.settings_path
            && let Err(err) = self.settings.save(path)
        {
            crate::log::error(&format!("ayarlar kaydedilemedi: {err}"));
        }
    }

    /// Geçerli sitede engelleyiciyi açar/kapatır ve sayfayı yeniler.
    fn toggle_blocking(&mut self) {
        let page_url = self.current().filter.page_url.borrow().clone();
        if address::is_start_page(&page_url) {
            return;
        }
        let Some(host) = address::host_of(&page_url) else {
            return;
        };
        let list = &mut self.settings.blocking_disabled;
        // Alt alan adında açarken üst alan adı kaydını da kaldır.
        if self.filter.disabled_for(&page_url) {
            list.retain(|entry| !settings::covers(entry, &host));
        } else {
            list.insert(host);
        }
        *self.filter.disabled.borrow_mut() = list.clone();
        self.filter.bump();
        self.save_settings();
        self.recreate_current_tab();
    }

    /// Öndeki sekmenin WebView'ini kapatıp aynı adreste sıfırdan açar.
    ///
    /// Engelleyici açılıp kapatıldığında yenilemek yetmez: sayfa yenilenirken
    /// WebView2 önceden kaydedilmiş betikleri (scriptlet) hâlâ çalıştırabilir.
    /// Yeni WebView'de eski betik olamaz. Bedeli: sekmenin geri/ileri geçmişi
    /// sıfırlanır.
    fn recreate_current_tab(&mut self) {
        let index = self.active;
        if let Some(url) = self.tabs[index].webview.as_ref().and_then(|w| w.url().ok()) {
            self.tabs[index].url = url;
        }
        // WebView'i bırakmak WebView2 sayfasını kapatır; `activate` uyuyan
        // sekmeyi uyandırır gibi yeniden oluşturur.
        self.tabs[index].webview = None;
        self.tabs[index].power = Power::Discarded;
        self.activate(index);
    }

    /// Geçmiş/yer imi veritabanında bir işlem yapar; hata olursa günlüğe yazar.
    fn with_places<T>(&self, f: impl FnOnce(&Places) -> rusqlite::Result<T>) -> Option<T> {
        let places = self.places.as_ref()?.lock().ok()?;
        f(&places)
            .map_err(|err| crate::log::error(&format!("geçmiş veritabanı: {err}")))
            .ok()
    }

    /// Öndeki sekmenin en güncel adresi.
    fn current_url(&self) -> String {
        self.view()
            .url()
            .unwrap_or_else(|_| self.current().url.clone())
    }

    /// Öndeki sayfayı yer imlerine ekler ya da çıkarır (Ctrl+D, yıldız).
    fn toggle_bookmark(&mut self) {
        let url = self.current_url();
        if !places::is_recordable(&url) {
            return;
        }
        let title = self.current().title.clone();
        let bookmarked = self.with_places(|p| p.is_bookmarked(&url)).unwrap_or(false);
        if bookmarked {
            self.with_places(|p| p.remove_bookmark(&url));
        } else {
            self.with_places(|p| p.add_bookmark(&url, &title, places::now()));
        }
        self.reload_bookmarks();
        self.update_star();
    }

    /// Yıldız butonunu öndeki sayfaya göre günceller.
    fn update_star(&mut self) {
        let url = self.current_url();
        let enabled = self.places.is_some() && places::is_recordable(&url);
        let bookmarked = enabled && self.with_places(|p| p.is_bookmarked(&url)).unwrap_or(false);
        self.star = (bookmarked, enabled);
        self.sync_chrome();
    }

    /// Tarayıcının iç sayfası (geçmiş, ayarlar) açıksa ona geçer ve
    /// yeniler; değilse yeni sekmede açar.
    fn open_internal(&mut self, name: &str, url: &str) {
        let existing = self
            .tabs
            .iter()
            .position(|t| address::internal_page(&t.url) == Some(name));
        match existing {
            Some(index) => {
                self.activate(index);
                let _ = self.view().reload();
            }
            None => self.open_tab(url),
        }
    }

    /// Yer imlerini veritabanından yeniden okur (kenar çubuğu için).
    fn reload_bookmarks(&mut self) {
        self.bookmarks = self
            .with_places(|p| p.bookmarks())
            .unwrap_or_default()
            .into_iter()
            .map(|e| BookmarkInfo {
                title: e.title,
                url: e.url,
            })
            .collect();
        self.sync_chrome();
    }

    /// Ayarlar sayfasından gelen görünüm değişikliği.
    fn change_appearance(&mut self, key: &str, value: &str) {
        let mut a = self.settings.appearance;
        if !a.set(key, value) || a == self.settings.appearance {
            return;
        }
        self.settings.appearance = a;
        self.save_settings();
        self.start_state.lock().unwrap().appearance = a;
        self.chrome.borrow_mut().set_appearance(a);
        self.after_theme_change();
    }

    /// Tema değişince sayfaların yeri ve renk düzeni de değişebilir.
    fn after_theme_change(&mut self) {
        self.layout();
        let theme = self.web_theme();
        for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            let _ = webview.set_theme(theme);
        }
    }

    /// Sayfaların (iç sayfalar ve sitelerin `prefers-color-scheme` sorgusu)
    /// kullanacağı renk düzeni.
    fn web_theme(&self) -> wry::Theme {
        if self.chrome.borrow().theme().dark {
            wry::Theme::Dark
        } else {
            wry::Theme::Light
        }
    }

    /// Arayüzün içeriğini (sekmeler, butonlar, rozetler) günceller.
    fn sync_chrome(&self) {
        // Henüz sekme yokken (açılışta) çizilecek bir şey yok.
        if self.tabs.is_empty() {
            return;
        }
        let asleep = |t: &Tab| t.webview.is_none() || t.power == Power::Suspended;
        let sleeping = self.tabs.iter().filter(|t| asleep(t)).count();
        let model = Model {
            tabs: self
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| TabInfo {
                    title: if address::is_start_page(&t.url) && t.title.is_empty() {
                        "Yeni sekme".to_string()
                    } else {
                        t.title.clone()
                    },
                    active: i == self.active,
                    audio: t.audio,
                    sleeping: i != self.active && asleep(t),
                })
                .collect(),
            bookmarks: self.bookmarks.clone(),
            can_back: self.can_back,
            can_forward: self.can_forward,
            bookmarked: self.star.0,
            star_enabled: self.star.1,
            shield: self.shield.0.clone(),
            shield_off: self.shield.1,
            status: if sleeping > 0 {
                format!("{sleeping} sekme uyuyor")
            } else {
                String::new()
            },
            tab_scroll: 0.0,
        };
        self.chrome.borrow_mut().set_model(model);
    }

    /// Geri/ileri butonlarının durumunu öndeki sekmeden okur.
    fn update_navigation(&mut self) {
        let webview = self.view();
        let back = webview.can_go_back().unwrap_or(false);
        let forward = webview.can_go_forward().unwrap_or(false);
        self.can_back = back;
        self.can_forward = forward;
    }

    fn filters_failed(&mut self, err: String) {
        crate::log::error(&format!("filtre listeleri güncellenemedi: {err}"));
        self.filters_error = true;
        self.update_shield();
    }

    /// Sayfadaki betiğin isteğine göre gizlenecek reklam kutularını bulur
    /// ve sayfaya CSS olarak uygular.
    fn apply_cosmetics(&mut self, id: u64, url: &str, message: cosmetic::Message) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if address::is_start_page(url) || self.filter.disabled_for(url) {
            return;
        }
        let blocker = self.filter.blocker.borrow();
        let Some(blocker) = blocker.as_ref() else {
            return;
        };
        let tab = &mut self.tabs[index];
        let selectors = match message {
            cosmetic::Message::Start => {
                let page = blocker.page_cosmetics(url);
                tab.cosmetic_exceptions = page.exceptions;
                tab.generichide = page.generichide;
                page.hide_selectors
            }
            cosmetic::Message::Seen { classes, ids } => {
                if tab.generichide {
                    return;
                }
                let classes: Vec<&str> = classes.iter().map(String::as_str).collect();
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                blocker.generic_selectors(&classes, &ids, &tab.cosmetic_exceptions)
            }
        };
        let css = cosmetic::hide_css(&selectors);
        if let (false, Some(webview)) = (css.is_empty(), &tab.webview) {
            let _ = webview.evaluate_script(&cosmetic::apply_script(&css));
        }
    }

    /// Engelleyici rozetini geçerli sekmeye göre günceller.
    fn update_shield(&mut self) {
        let tab = self.current();
        let page_url = tab.filter.page_url.borrow().clone();
        let blocked = tab.filter.blocked.get();
        self.shield =
            if address::is_internal_page(&page_url) || address::host_of(&page_url).is_none() {
                ("0".to_string(), false)
            } else if self.filter.disabled_for(&page_url) {
                ("Kapalı".to_string(), true)
            } else if self.filter.blocker.borrow().is_none() {
                let text = if self.filters_error {
                    "Liste yok"
                } else {
                    "…"
                };
                (text.to_string(), true)
            } else {
                (blocked.to_string(), false)
            };
        self.sync_chrome();
    }

    /// Adres çubuğunu günceller; başlangıç sayfasında kutu boş kalır.
    fn show_address(&self, url: &str) {
        self.chrome
            .borrow()
            .set_address(if address::is_start_page(url) { "" } else { url });
    }

    fn current(&self) -> &Tab {
        &self.tabs[self.active]
    }

    /// Öndeki sekmenin WebView'i. Öndeki sekme hiçbir zaman uyumaz.
    fn view(&self) -> &WebView {
        self.current()
            .webview
            .as_ref()
            .expect("öndeki sekme uyanık olmalı")
    }

    /// Uzun süredir arka planda olan sekmeleri dondurur ya da bellekten atar.
    fn put_tabs_to_sleep(&mut self) {
        let now = Instant::now();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == self.active {
                continue;
            }
            let (Some(webview), Some(since)) = (&tab.webview, tab.hidden_since) else {
                continue;
            };
            let action = sleep::decide(
                &self.sleep_timing,
                tab.power,
                now - since,
                is_playing_audio(webview),
                address::is_start_page(&tab.url),
            );
            match action {
                Some(Action::Suspend) => {
                    if try_suspend(webview) {
                        tab.power = Power::Suspended;
                    }
                }
                Some(Action::Discard) => {
                    // Tek sayfalık uygulamalar adresi sayfa yüklenmeden de
                    // değiştirebilir; en güncel adresi WebView'den al.
                    if let Ok(url) = webview.url() {
                        tab.url = url;
                    }
                    // WebView'in bırakılması WebView2 sayfasını ve belleğini serbest bırakır.
                    tab.webview = None;
                    tab.power = Power::Discarded;
                }
                None => {}
            }
        }
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Sayfa alanı (fiziksel piksel): arayüzün sayfaya ayırdığı bölge.
    fn content_bounds(&self) -> Rect {
        let (x, y, w, h) = self.chrome.borrow().content_px();
        Rect {
            position: PhysicalPosition::new(x, y).into(),
            size: PhysicalSize::new(w, h).into(),
        }
    }

    /// Sayfayı arayüzün çerçevesine yerleştirir ve köşelerini yuvarlar.
    fn place_webview(&self, webview: &WebView) {
        let _ = webview.set_bounds(self.content_bounds());
        let (_, _, w, h) = self.chrome.borrow().content_px();
        let r = self.chrome.borrow().content_radius_px();
        unsafe {
            let region = CreateRoundRectRgn(0, 0, w as i32 + 1, h as i32 + 1, 2 * r, 2 * r);
            // Pencere bölgeyi sahiplenir; silmek gerekmez.
            SetWindowRgn(webview.hwnd().0, region, 1);
        }
    }

    fn layout(&self) {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        self.chrome
            .borrow_mut()
            .set_size(size.width, size.height, scale);
        // Gizli sekmeler de yeniden boyutlanır ki geçişte sayfa zıplamasın.
        for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            self.place_webview(webview);
        }
    }

    fn open_tab(&mut self, url: &str) {
        let index = self.add_tab(url, "");
        self.activate(index);
    }

    /// Sekme şeridinin sonuna, henüz yüklenmemiş (uyuyan) bir sekme ekler.
    /// Sayfa, sekme öne geldiğinde `activate` içinde yüklenir.
    fn add_tab(&mut self, url: &str, title: &str) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let index = self.tabs.len();
        self.tabs.push(Tab {
            id,
            webview: None,
            title: title.to_string(),
            url: url.to_string(),
            filter: TabState::default(),
            power: Power::Discarded,
            hidden_since: None,
            recorded_url: String::new(),
            audio: false,
            cosmetic_exceptions: HashSet::new(),
            generichide: false,
        });
        self.sync_chrome();
        index
    }

    /// Önceki oturumun sekmelerini geri getirir. Yalnızca öndeki sekme
    /// yüklenir; diğerleri tıklanınca yüklenir (açılış hızlı, bellek az).
    fn restore_session(&mut self) {
        let session = self
            .session_path
            .as_deref()
            .map(Session::load)
            .unwrap_or_default();
        if session.tabs.is_empty() {
            self.open_tab(START_URL);
            return;
        }
        for tab in &session.tabs {
            self.add_tab(&tab.url, &tab.title);
        }
        self.activate(session.active);
    }

    /// Açık sekmeleri diske yazar.
    fn save_session(&mut self) {
        let Some(path) = &self.session_path else {
            return;
        };
        let tabs = self
            .tabs
            .iter()
            .map(|tab| SavedTab {
                // Tek sayfalık uygulamalar adresi sayfa yüklenmeden de
                // değiştirebilir; açık sekmede en güncel adresi WebView'den al.
                url: tab
                    .webview
                    .as_ref()
                    .and_then(|w| w.url().ok())
                    .unwrap_or_else(|| tab.url.clone()),
                title: tab.title.clone(),
            })
            .collect();
        let session = Session {
            tabs,
            active: self.active,
        };
        match session.save(path) {
            Ok(()) => self.session_dirty = false,
            Err(err) => crate::log::error(&format!("oturum kaydedilemedi: {err}")),
        }
    }

    /// Sekme için WebView2 oluşturur. Yeni sekmede ve uyuyan sekme
    /// uyandırılırken kullanılır.
    fn create_webview(&mut self, id: u64, url: &str, filter: &TabState) -> WebView {
        let title_proxy = self.proxy.clone();
        let load_proxy = self.proxy.clone();
        let window_proxy = self.proxy.clone();
        let ipc_proxy = self.proxy.clone();
        let nav_proxy = self.proxy.clone();
        let nav_filter = filter.clone();
        let start_state = Arc::clone(&self.start_state);
        let places = self.places.clone();
        let bounds = self.content_bounds();
        let web_theme = self.web_theme();

        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(url)
            .with_bounds(bounds)
            .with_additional_browser_args(BROWSER_ARGS)
            // Ctrl + tekerlek / Ctrl + artı-eksi ile yakınlaştırma.
            .with_hotkeys_zoom(true)
            .with_devtools(true)
            .with_theme(web_theme)
            .with_custom_protocol("julie".into(), move |_, request| {
                serve(
                    request.uri(),
                    *start_state.lock().unwrap(),
                    places.as_deref(),
                )
            })
            // `window.ipc` her sayfada vardır; tarayıcı komutlarını yalnızca
            // tarayıcının kendi sayfalarından kabul et.
            .with_ipc_handler(move |request| {
                let page = request.uri().to_string();
                let body = request.body();
                if address::is_internal_page(&page)
                    && let Some(message) = Message::parse(body)
                {
                    let _ = ipc_proxy.send_event(UserEvent::StartPage(id, message));
                } else if let Some(message) = cosmetic::Message::parse(body) {
                    let _ = ipc_proxy.send_event(UserEvent::Cosmetic(id, page, message));
                }
            })
            // Reklamlardan kalan boş kutuları gizleyen betik.
            .with_initialization_script(cosmetic::SCRIPT)
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(UserEvent::TitleChanged(id, title));
            })
            // Ana sayfa değişiyor: engelleme sayacını sıfırla.
            .with_navigation_handler(move |url| {
                nav_filter.start_page_load(&url);
                let _ = nav_proxy.send_event(UserEvent::BlockedCountChanged(id));
                true
            })
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, PageLoadEvent::Started) {
                    let _ = load_proxy.send_event(UserEvent::UrlChanged(id, url));
                }
            })
            // Yeni pencere açmak yerine yeni sekme aç.
            .with_new_window_req_handler(move |url, _features| {
                let _ = window_proxy.send_event(UserEvent::OpenInNewTab(url));
                NewWindowResponse::Deny
            })
            .build_as_child(&self.window)
            .expect("WebView2 oluşturulamadı (WebView2 Runtime yüklü mü?)");
        listen_for_shortcuts(&webview, self.proxy.clone());
        listen_for_source_changes(&webview, self.proxy.clone(), id);
        listen_for_audio(&webview, self.proxy.clone(), id);
        self.place_webview(&webview);
        set_reputation_checking(&webview, !self.settings.smartscreen_off);
        scriptlet_injector::install(&webview, self.filter.clone());
        request_filter::install(
            &webview,
            self.filter.clone(),
            filter.clone(),
            self.proxy.clone(),
            id,
        );
        webview
    }

    /// Sekmeyi kapatır. Son sekme kapanınca tarayıcı da kapanır.
    fn close_tab(&mut self, index: usize) -> Flow {
        if self.tabs.len() == 1 {
            return Flow::Exit;
        }
        // WebView drop edildiğinde WebView2 sayfası ve belleği serbest kalır.
        let tab = self.tabs.remove(index);
        if !address::is_start_page(&tab.url) {
            self.closed.push(tab.url);
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        self.session_dirty = true;
        if index != self.active {
            // Arka plandaki bir sekme kapandı: öndeki sekme aynı kalır,
            // yalnızca sırası kayabilir.
            if index < self.active {
                self.active -= 1;
            }
            self.sync_chrome();
            return Flow::Continue;
        }
        let next = if index >= self.tabs.len() {
            self.tabs.len() - 1
        } else {
            index
        };
        // `activate` eski sekmeyi gizlemeye çalışmasın diye geçerli bir indeks ver.
        self.active = next;
        self.activate(next);
        Flow::Continue
    }

    fn activate(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // Arka plandaki sekmeleri gizle ve bellek kullanımını düşük seviyeye çek.
        // WebView2 gizli sekmelerde zamanlayıcıları ve çizimi zaten yavaşlatır.
        self.session_dirty = true;
        let now = Instant::now();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == index || tab.hidden_since.is_some() {
                continue;
            }
            tab.hidden_since = Some(now);
            if let Some(webview) = &tab.webview {
                let _ = webview.set_visible(false);
                let _ = webview.set_memory_usage_level(MemoryUsageLevel::Low);
            }
        }
        self.active = index;

        // Bellekten atılmış sekmeyi kaldığı adresten yeniden yükle.
        if self.tabs[index].webview.is_none() {
            let (id, url, filter) = {
                let tab = &self.tabs[index];
                (tab.id, tab.url.clone(), tab.filter.clone())
            };
            self.tabs[index].webview = Some(self.create_webview(id, &url, &filter));
        }

        let tab = &mut self.tabs[index];
        tab.hidden_since = None;
        tab.power = Power::Awake;
        let tab = &self.tabs[index];
        let webview = tab.webview.as_ref().expect("az önce uyandırıldı");
        // Görünür yapmak dondurulmuş sayfayı da otomatik çözer.
        let _ = webview.set_memory_usage_level(MemoryUsageLevel::Normal);
        let _ = webview.set_visible(true);
        let _ = webview.focus();
        self.show_address(&tab.url);
        let title = tab.title.clone();
        self.set_window_title(&title);
        self.update_navigation();
        self.update_shield();
        self.update_star();
    }

    fn set_window_title(&self, page_title: &str) {
        if page_title.trim().is_empty() {
            self.window.set_title("Julie");
        } else {
            self.window.set_title(&format!("{page_title} — Julie"));
        }
    }
}
