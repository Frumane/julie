//! Pencere, sekmeler ve olay döngüsü.

use std::path::PathBuf;

use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowExtWindows;
use tao::window::{Window, WindowBuilder};
use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
};

use crate::UserEvent;
use crate::address::{self, HOME_URL};
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

    let mut browser = Browser {
        window,
        toolbar,
        context: WebContext::new(data_directory()),
        proxy,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
    };
    browser.layout();
    browser.open_tab(HOME_URL);
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

/// Çerezler, önbellek ve WebView2 profili için klasör:
/// `%LOCALAPPDATA%\Julie\WebView2`.
fn data_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Julie").join("WebView2"))
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Exit,
}

impl Browser {
    fn handle(&mut self, event: UserEvent) -> Flow {
        match event {
            UserEvent::Back => {
                let _ = self.current().webview.go_back();
            }
            UserEvent::Forward => {
                let _ = self.current().webview.go_forward();
            }
            UserEvent::Reload => {
                let _ = self.current().webview.reload();
            }
            UserEvent::Navigate(text) => {
                let url = address::resolve(&text);
                self.toolbar.set_address(&url);
                let tab = self.current();
                let _ = tab.webview.load_url(&url);
                let _ = tab.webview.focus();
            }
            UserEvent::FocusPage => {
                let tab = self.current();
                self.toolbar.set_address(&tab.url);
                let _ = tab.webview.focus();
            }
            UserEvent::NewTab => {
                self.open_tab(HOME_URL);
                self.toolbar.focus_address();
            }
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
                        self.toolbar.set_address(&url);
                    }
                    self.tabs[index].url = url;
                }
            }
        }
        Flow::Continue
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
        let bounds = self.content_bounds();

        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(url)
            .with_bounds(bounds)
            .with_additional_browser_args(BROWSER_ARGS)
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
        self.tabs.remove(index);
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
        self.toolbar.set_address(&tab.url);
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
