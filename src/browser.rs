//! Pencere, sekmeler ve olay döngüsü.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowExtWindows;
use tao::window::{Window, WindowBuilder};
use webview2_com::AcceleratorKeyPressedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use wry::http::{Response, StatusCode};
use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
};

use crate::UserEvent;
use crate::address::{self, START_URL};
use crate::settings::{self, Engine, Settings};
use crate::shortcuts::{self, Modifiers};
use crate::start_page::{self, Message};
use crate::toolbar::Toolbar;

/// WebView2'ye verilen başlangıç argümanları. wry'nin varsayılanlarını
/// koruyoruz (mini menü ve SmartScreen kapalı); bellek ve CPU ayarları
/// sonraki adımlarda buraya eklenecek.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

struct Tab {
    id: u64,
    webview: WebView,
    title: String,
    url: String,
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
    shared_engine: Arc<Mutex<Option<Engine>>>,
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

    let mut browser = Browser {
        window,
        toolbar,
        context: WebContext::new(app_dir.map(|dir| dir.join("WebView2"))),
        proxy,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
        closed: Vec::new(),
        shared_engine: Arc::new(Mutex::new(settings.engine)),
        settings,
        settings_path,
    };
    browser.layout();
    browser.open_tab(START_URL);
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
fn serve(path: &str, engine: Option<Engine>) -> Response<Cow<'static, [u8]>> {
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
        .body(Cow::Owned(start_page::render(engine).into_bytes()))
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
                let webview = &self.current().webview;
                let _ = webview.go_back();
                let _ = webview.focus();
            }
            UserEvent::Forward => {
                let webview = &self.current().webview;
                let _ = webview.go_forward();
                let _ = webview.focus();
            }
            UserEvent::Reload => {
                let webview = &self.current().webview;
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
                let tab = self.current();
                self.show_address(&tab.url);
                let _ = tab.webview.focus();
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
        let webview = &self.tabs[index].webview;
        let _ = webview.load_url(&url);
        let _ = webview.focus();
    }

    fn set_engine(&mut self, engine: Engine) {
        self.settings.engine = Some(engine);
        *self.shared_engine.lock().unwrap() = Some(engine);
        if let Some(path) = &self.settings_path
            && let Err(err) = self.settings.save(path)
        {
            eprintln!("ayarlar kaydedilemedi: {err}");
        }
    }

    /// Adres çubuğunu günceller; başlangıç sayfasında kutu boş kalır.
    fn show_address(&self, url: &str) {
        self.toolbar
            .set_address(if address::is_start_page(url) { "" } else { url });
    }

    fn current(&self) -> &Tab {
        &self.tabs[self.active]
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
        for tab in &self.tabs {
            let _ = tab.webview.set_bounds(bounds);
        }
    }

    fn open_tab(&mut self, url: &str) {
        let id = self.next_id;
        self.next_id += 1;

        let title_proxy = self.proxy.clone();
        let load_proxy = self.proxy.clone();
        let window_proxy = self.proxy.clone();
        let ipc_proxy = self.proxy.clone();
        let engine = Arc::clone(&self.shared_engine);
        let bounds = self.content_bounds();

        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(url)
            .with_bounds(bounds)
            .with_additional_browser_args(BROWSER_ARGS)
            // Ctrl + tekerlek / Ctrl + artı-eksi ile yakınlaştırma.
            .with_hotkeys_zoom(true)
            .with_custom_protocol("julie".into(), move |_, request| {
                serve(request.uri().path(), *engine.lock().unwrap())
            })
            // `window.ipc` her sayfada vardır; yalnızca başlangıç sayfasından
            // gelen mesajları kabul et.
            .with_ipc_handler(move |request| {
                if address::is_start_page(&request.uri().to_string())
                    && let Some(message) = Message::parse(request.body())
                {
                    let _ = ipc_proxy.send_event(UserEvent::StartPage(id, message));
                }
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(UserEvent::TitleChanged(id, title));
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

        let index = self.tabs.len();
        self.tabs.push(Tab {
            id,
            webview,
            title: String::new(),
            url: url.to_string(),
        });
        self.toolbar.insert_tab(index, "");
        self.activate(index);
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
        for (i, tab) in self.tabs.iter().enumerate() {
            if i != index {
                let _ = tab.webview.set_visible(false);
                let _ = tab.webview.set_memory_usage_level(MemoryUsageLevel::Low);
            }
        }
        self.active = index;
        let tab = &self.tabs[index];
        let _ = tab.webview.set_memory_usage_level(MemoryUsageLevel::Normal);
        let _ = tab.webview.set_visible(true);
        let _ = tab.webview.focus();
        self.toolbar.select_tab(index);
        self.show_address(&tab.url);
        let title = tab.title.clone();
        self.set_window_title(&title);
    }

    fn set_window_title(&self, page_title: &str) {
        if page_title.trim().is_empty() {
            self.window.set_title("Julie");
        } else {
            self.window.set_title(&format!("{page_title} — Julie"));
        }
    }
}
