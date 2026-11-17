//! Pencere, sekmeler ve olay döngüsü.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
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
use webview2_com::{AcceleratorKeyPressedEventHandler, TrySuspendCompletedHandler};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::core::{BOOL, Interface};
use wry::http::{Response, StatusCode};
use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
};

use crate::UserEvent;
use crate::address::{self, START_URL};
use crate::blocking::{self, Blocker};
use crate::cosmetic;
use crate::request_filter::{self, TabState};
use crate::settings::{self, Engine, Settings};
use crate::shortcuts::{self, Modifiers};
use crate::sleep::{self, Action, Power};
use crate::start_page::{self, Message};
use crate::toolbar::Toolbar;

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
    /// Geçerli sayfada genel gizleme kurallarından muaf seçiciler.
    cosmetic_exceptions: HashSet<String>,
    /// Geçerli sayfada genel gizleme kuralları kapalı mı (`$generichide`)?
    generichide: bool,
}

struct Browser {
    window: Window,
    toolbar: Toolbar,
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

    let toolbar = Toolbar::new(window.hwnd() as _, proxy.clone());

    let app_dir = settings::app_dir();
    let settings_path = app_dir.as_ref().map(|dir| dir.join("settings.ini"));
    let settings = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();

    let filter = request_filter::Shared::default();
    *filter.disabled.borrow_mut() = settings.blocking_disabled.clone();
    if let Some(dir) = &app_dir {
        load_filters(&filter, &blocking::cache_path(dir), &proxy);
    }

    let mut browser = Browser {
        window,
        toolbar,
        context: WebContext::new(app_dir.map(|dir| dir.join("WebView2"))),
        proxy,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
        closed: Vec::new(),
        start_state: Arc::new(Mutex::new(start_page::State {
            engine: settings.engine,
            smartscreen: !settings.smartscreen_off,
        })),
        settings,
        settings_path,
        filter,
        filters_error: false,
        sleep_timing: sleep::Timing::from_env(),
    };
    browser.layout();
    browser.open_tab(START_URL);
    start_ticker(browser.proxy.clone(), browser.sleep_timing.check_every);
    browser.toolbar.focus_address();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    browser.layout()
                }
                _ => {}
            },
            Event::UserEvent(event) => {
                if browser.handle(event) == Flow::Exit {
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
fn serve(path: &str, state: start_page::State) -> Response<Cow<'static, [u8]>> {
    if path != "/" {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Cow::Borrowed(&[][..]))
            .expect("geçerli yanıt");
    }
    Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .header(
            "Content-Security-Policy",
            start_page::CONTENT_SECURITY_POLICY,
        )
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(start_page::render(state).into_bytes()))
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
            UserEvent::FocusAddress => self.toolbar.focus_address(),
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
                self.toolbar.focus_address();
            }
            UserEvent::StartPage(id, message) => match message {
                Message::Go(text) => {
                    if let Some(index) = self.index_of(id) {
                        self.navigate(index, &text);
                    }
                }
                Message::SetEngine(engine) => self.set_engine(engine),
                Message::SetSmartScreen(on) => self.set_smartscreen(on),
            },
            UserEvent::OpenInNewTab(url) => self.open_tab(&url),
            UserEvent::CloseTab => return self.close_tab(self.active),
            UserEvent::SelectTab(index) => self.activate(index),
            UserEvent::TitleChanged(id, title) => {
                if let Some(index) = self.index_of(id) {
                    self.toolbar.set_tab_title(index, &title);
                    if index == self.active {
                        self.set_window_title(&title);
                    }
                    self.tabs[index].title = title;
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
                    Some(blocker) => *self.filter.blocker.borrow_mut() = Some(blocker),
                    None => self.filters_failed("derlenen kurallar okunamadı".into()),
                }
                self.update_shield();
            }
            UserEvent::FiltersFailed(err) => self.filters_failed(err),
            UserEvent::Tick => self.put_tabs_to_sleep(),
            UserEvent::Cosmetic(id, url, message) => self.apply_cosmetics(id, &url, message),
            UserEvent::UrlChanged(id, url) => {
                if let Some(index) = self.index_of(id) {
                    if index == self.active {
                        self.show_address(&url);
                    }
                    self.tabs[index].url = url;
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
        self.save_settings();
        let webview = self.view();
        let _ = webview.reload();
        let _ = webview.focus();
        self.update_shield();
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

    /// Engelleme butonunun yazısını geçerli sekmeye göre günceller.
    fn update_shield(&self) {
        let tab = self.current();
        let page_url = tab.filter.page_url.borrow();
        let text = if address::is_start_page(&page_url) || address::host_of(&page_url).is_none() {
            "Engelleyici açık".to_string()
        } else if self.filter.disabled_for(&page_url) {
            "Bu sitede kapalı".to_string()
        } else if self.filter.blocker.borrow().is_none() {
            if self.filters_error {
                "Liste indirilemedi".to_string()
            } else {
                "Listeler iniyor…".to_string()
            }
        } else {
            format!("{} engellendi", tab.filter.blocked.get())
        };
        self.toolbar.set_shield(&text);
    }

    /// Adres çubuğunu günceller; başlangıç sayfasında kutu boş kalır.
    fn show_address(&self, url: &str) {
        self.toolbar
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

    /// Sayfa alanı: araç çubuğunun altında kalan bölge (fiziksel piksel).
    fn content_bounds(&self) -> Rect {
        let size = self.window.inner_size();
        let top = Toolbar::height(self.window.scale_factor()).min(size.height);
        Rect {
            position: PhysicalPosition::new(0, top as i32).into(),
            size: PhysicalSize::new(size.width, size.height - top).into(),
        }
    }

    fn layout(&self) {
        self.toolbar
            .layout(self.window.inner_size().width, self.window.scale_factor());
        // Gizli sekmeler de yeniden boyutlanır ki geçişte sayfa zıplamasın.
        let bounds = self.content_bounds();
        for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            let _ = webview.set_bounds(bounds);
        }
    }

    fn open_tab(&mut self, url: &str) {
        let id = self.next_id;
        self.next_id += 1;
        let filter = TabState::default();
        let webview = self.create_webview(id, url, &filter);

        let index = self.tabs.len();
        self.tabs.push(Tab {
            id,
            webview: Some(webview),
            title: String::new(),
            url: url.to_string(),
            filter,
            power: Power::Awake,
            hidden_since: None,
            cosmetic_exceptions: HashSet::new(),
            generichide: false,
        });
        self.toolbar.insert_tab(index, "");
        self.activate(index);
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
        let bounds = self.content_bounds();

        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(url)
            .with_bounds(bounds)
            .with_additional_browser_args(BROWSER_ARGS)
            // Ctrl + tekerlek / Ctrl + artı-eksi ile yakınlaştırma.
            .with_hotkeys_zoom(true)
            .with_custom_protocol("julie".into(), move |_, request| {
                serve(request.uri().path(), *start_state.lock().unwrap())
            })
            // `window.ipc` her sayfada vardır; yalnızca başlangıç sayfasından
            // gelen mesajları kabul et.
            .with_ipc_handler(move |request| {
                let page = request.uri().to_string();
                let body = request.body();
                if address::is_start_page(&page)
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
        set_reputation_checking(&webview, !self.settings.smartscreen_off);
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
        self.toolbar.remove_tab(index);
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
        self.toolbar.select_tab(index);
        self.show_address(&tab.url);
        let title = tab.title.clone();
        self.set_window_title(&title);
        self.update_shield();
    }

    fn set_window_title(&self, page_title: &str) {
        if page_title.trim().is_empty() {
            self.window.set_title("Julie");
        } else {
            self.window.set_title(&format!("{page_title} — Julie"));
        }
    }
}
