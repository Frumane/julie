//! Eklentinin açılır penceresi: araç çubuğundaki düğmesine basınca
//! düğmenin yanında açılan küçük sayfa (`chrome-extension://<kimlik>/popup.html`).
//! Boyutu içeriğe göre ayarlanır; dışarı tıklanınca kapanır.

use std::num::NonZeroIsize;
use std::ptr::{null, null_mut};

use tao::event_loop::EventLoopProxy;
use webview2_com::WindowCloseRequestedEventHandler;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, RegisterClassExW, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_ACTIVATE, WM_NCACTIVATE,
    WM_NCCREATE, WM_NCDESTROY, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use wry::{
    NewWindowResponse, Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows,
    WebViewExtWindows,
};

use crate::UserEvent;
use crate::extensions::popup::{MIN, clamp_size, parse_size, place};

const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: u32 = 2;

/// Sayfa boyutunu bildirmezse.
const FALLBACK: (u32, u32) = (360, 480);

/// Sayfanın içeriğinin boyutunu bildiren betik. Pencere küçük açılır;
/// içerik ne kadar yer isterse pencere o kadar büyür.
const SIZE_SCRIPT: &str = r#"(() => {
  if (!location.protocol.startsWith("chrome-extension")) return;
  let last = "";
  const send = () => {
    const d = document.documentElement, b = document.body;
    if (!b) return;
    const r = b.getBoundingClientRect();
    const w = Math.ceil(Math.max(d.scrollWidth, b.scrollWidth, r.width));
    const h = Math.ceil(Math.max(d.scrollHeight, b.scrollHeight, r.height));
    const now = w + "," + h;
    if (now === last) return;
    last = now;
    if (window.ipc) window.ipc.postMessage("julie-popup-size:" + now);
  };
  addEventListener("load", () => {
    send();
    new ResizeObserver(send).observe(document.body);
    new MutationObserver(send).observe(document.body, { childList: true, subtree: true });
    setTimeout(send, 250);
  });
})();"#;

/// Sayfanın boyutunu soran ifade (bildirim betiği çalışmazsa).
pub const SIZE_QUERY: &str = "(() => { const d = document.documentElement, b = document.body || d; \
     return [Math.ceil(Math.max(d.scrollWidth, b.scrollWidth)), Math.ceil(Math.max(d.scrollHeight, b.scrollHeight))]; })()";

/// wry'ye verilecek üst pencere.
struct Parent(HWND);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = NonZeroIsize::new(self.0 as isize).ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(handle));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// Pencere işlevinin bilmesi gerekenler.
struct Shared {
    proxy: EventLoopProxy<UserEvent>,
    owner: HWND,
    serial: u64,
}

/// Her açılır pencerenin numarası: eski pencereden gelen "kapan" yenisini
/// kapatmasın.
static NEXT_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let shared = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared;
        match msg {
            WM_ACTIVATE if !shared.is_null() => {
                let shared = &*shared;
                if wparam & 0xffff == 0 {
                    // Başka yere tıklandı: kapan. Ana pencere de başka bir
                    // uygulamaya geçildiyse etkin görünmesin.
                    if lparam as HWND != shared.owner {
                        SendMessageW(shared.owner, WM_NCACTIVATE, 0, 0);
                    }
                    let _ = shared
                        .proxy
                        .send_event(UserEvent::ExtensionPopupClose(shared.serial));
                } else {
                    // Açılır pencere açıkken ana pencerenin başlığı soluklaşmasın.
                    SendMessageW(shared.owner, WM_NCACTIVATE, 1, 0);
                }
            }
            WM_NCDESTROY if !shared.is_null() => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(shared as *mut Shared));
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Açılacak pencerenin bilgileri.
pub struct Spec<'a> {
    /// Ana pencere.
    pub owner: isize,
    /// Eklentinin kimliği.
    pub id: &'a str,
    pub url: &'a str,
    /// Düğmenin ekrandaki yeri (sol, üst, sağ, alt).
    pub anchor: (i32, i32, i32, i32),
    pub scale: f64,
    pub dark: bool,
    /// Sekmelerle aynı olmalı: WebView2 aynı veri klasöründe farklı
    /// başlangıç ayarlı motoru açmaz.
    pub browser_args: String,
}

pub struct Popup {
    /// Eklentinin kimliği.
    pub id: String,
    pub serial: u64,
    hwnd: HWND,
    webview: Option<WebView>,
    anchor: (i32, i32, i32, i32),
    scale: f64,
    shown: bool,
}

impl Popup {
    pub fn open(
        context: &mut WebContext,
        proxy: EventLoopProxy<UserEvent>,
        spec: Spec<'_>,
    ) -> wry::Result<Popup> {
        let Spec {
            owner,
            id,
            url,
            anchor,
            scale,
            dark,
            browser_args,
        } = spec;
        let owner = owner as HWND;
        let serial = NEXT_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let hwnd = unsafe {
            let class: Vec<u16> = "JulieExtensionPopup\0".encode_utf16().collect();
            let instance = GetModuleHandleW(null());
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(popup_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            // İkinci kayıt başarısız olur; sınıf zaten vardır.
            RegisterClassExW(&wc);
            let shared = Box::into_raw(Box::new(Shared {
                proxy: proxy.clone(),
                owner,
                serial,
            }));
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                null(),
                WS_POPUP,
                anchor.0,
                anchor.3,
                MIN.0 as i32,
                MIN.1 as i32,
                owner,
                null_mut(),
                instance,
                shared.cast(),
            );
            if hwnd.is_null() {
                drop(Box::from_raw(shared));
                return Err(wry::Error::WindowHandleError(HandleError::Unavailable));
            }
            let corner = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corner).cast(),
                4,
            );
            hwnd
        };

        let ipc_proxy = proxy.clone();
        let tab_proxy = proxy.clone();
        let load_proxy = proxy.clone();
        let built = WebViewBuilder::new_with_web_context(context)
            .with_url(url)
            .with_bounds(Rect {
                position: PhysicalPosition::new(0, 0).into(),
                size: PhysicalSize::new(MIN.0, MIN.1).into(),
            })
            .with_additional_browser_args(browser_args)
            .with_browser_extensions_enabled(true)
            .with_devtools(false)
            .with_theme(if dark {
                wry::Theme::Dark
            } else {
                wry::Theme::Light
            })
            .with_initialization_script(SIZE_SCRIPT)
            .with_ipc_handler(move |request| {
                if let Some((w, h)) = parse_size(request.body()) {
                    let _ = ipc_proxy.send_event(UserEvent::ExtensionPopupSize(serial, w, h));
                }
            })
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let _ = load_proxy.send_event(UserEvent::ExtensionPopupLoaded(serial));
                }
            })
            // Açılır penceredeki bağlantılar sekmede açılır.
            .with_new_window_req_handler(move |url, _| {
                let _ = tab_proxy.send_event(UserEvent::OpenInNewTab(url));
                let _ = tab_proxy.send_event(UserEvent::ExtensionPopupClose(serial));
                NewWindowResponse::Deny
            })
            .build_as_child(&Parent(hwnd));
        let webview = match built {
            Ok(webview) => webview,
            Err(err) => {
                unsafe { DestroyWindow(hwnd) };
                return Err(err);
            }
        };
        // Sayfa `window.close()` derse kapan.
        let close_proxy = proxy.clone();
        let handler = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
            let _ = close_proxy.send_event(UserEvent::ExtensionPopupClose(serial));
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview
                .webview()
                .add_WindowCloseRequested(&handler, &mut token);
        }
        // Sayfa boyutunu hiç bildirmezse varsayılan boyutla gösterilir.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = proxy.send_event(UserEvent::ExtensionPopupSize(serial, 0, 0));
        });
        Ok(Popup {
            id: id.to_string(),
            serial,
            hwnd,
            webview: Some(webview),
            anchor,
            scale,
            shown: false,
        })
    }

    pub fn webview(&self) -> Option<&WebView> {
        self.webview.as_ref()
    }

    /// İçerik boyutu geldi (CSS pikseli). `(0, 0)`: süre doldu; pencere
    /// henüz görünmüyorsa varsayılan boyutla gösterilir.
    pub fn resize(&mut self, w: u32, h: u32) {
        let (w, h) = if w == 0 || h == 0 {
            if self.shown {
                return;
            }
            FALLBACK
        } else {
            clamp_size(w, h)
        };
        let size = (
            (f64::from(w) * self.scale).round() as i32,
            (f64::from(h) * self.scale).round() as i32,
        );
        let work = unsafe {
            let r = RECT {
                left: self.anchor.0,
                top: self.anchor.1,
                right: self.anchor.2,
                bottom: self.anchor.3,
            };
            let monitor = MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut info);
            let w = info.rcWork;
            (w.left, w.top, w.right, w.bottom)
        };
        let (x, y, w, h) = place(self.anchor, size, work);
        let size = (w, h);
        unsafe {
            SetWindowPos(
                self.hwnd,
                null_mut(),
                x,
                y,
                size.0,
                size.1,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        if let Some(webview) = &self.webview {
            let _ = webview.set_bounds(Rect {
                position: PhysicalPosition::new(0, 0).into(),
                size: PhysicalSize::new(size.0 as u32, size.1 as u32).into(),
            });
        }
        if !self.shown {
            self.shown = true;
            unsafe {
                ShowWindow(self.hwnd, SW_SHOW);
                SetForegroundWindow(self.hwnd);
            }
            if let Some(webview) = &self.webview {
                let _ = webview.focus();
            }
        }
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        // Önce motor, sonra pencere.
        self.webview = None;
        unsafe { DestroyWindow(self.hwnd) };
    }
}
