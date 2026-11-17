//! Yerel Win32 kontrolleriyle çizilen araç çubuğu ve sekme şeridi.
//!
//! Arayüz için ayrı bir web sayfası kullanmıyoruz: yerel kontroller birkaç
//! KB bellek harcar, bir web arayüzü ise kendi renderer sürecini açar.

use std::ptr::null;

use tao::event_loop::EventLoopProxy;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateFontIndirectW, HFONT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    EM_SETSEL, ICC_STANDARD_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx,
    NMHDR, TCIF_TEXT, TCITEMW, TCM_DELETEITEM, TCM_GETCURSEL, TCM_INSERTITEMW, TCM_SETCURSEL,
    TCM_SETITEMW, TCN_SELCHANGE, TCS_FOCUSNEVER,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, BS_PUSHBUTTON, CreateWindowExW, ES_AUTOHSCROLL, GetWindowTextLengthW,
    GetWindowTextW, HMENU, MoveWindow, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SendMessageW,
    SetWindowTextW, SystemParametersInfoW, WM_CHAR, WM_COMMAND, WM_KEYDOWN, WM_NOTIFY, WM_SETFONT,
    WS_BORDER, WS_CHILD, WS_CLIPSIBLINGS, WS_TABSTOP, WS_VISIBLE,
};

use crate::UserEvent;

/// Sekme şeridinin mantıksal yüksekliği (DPI ölçeği ile çarpılır).
const TAB_ROW: f64 = 30.0;
/// Araç çubuğunun mantıksal yüksekliği.
const BAR_ROW: f64 = 34.0;
const BUTTON: f64 = 32.0;
const GAP: f64 = 4.0;

const ID_BACK: u16 = 101;
const ID_FORWARD: u16 = 102;
const ID_RELOAD: u16 = 103;
const ID_NEW_TAB: u16 = 104;
const ID_CLOSE_TAB: u16 = 105;
const ID_TABS: usize = 106;
const ID_ADDRESS: u16 = 107;

/// Ana pencere ve adres kutusu için alt sınıflama (subclass) kimlikleri.
const SUBCLASS_MAIN: usize = 1;
const SUBCLASS_ADDRESS: usize = 2;

pub struct Toolbar {
    tabs: HWND,
    new_tab: HWND,
    close_tab: HWND,
    back: HWND,
    forward: HWND,
    reload: HWND,
    address: HWND,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl Toolbar {
    /// Kontrolleri `parent` penceresinin içinde oluşturur. Kullanıcı
    /// etkileşimleri `proxy` üzerinden olay döngüsüne gönderilir.
    pub fn new(parent: HWND, proxy: EventLoopProxy<UserEvent>) -> Self {
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TAB_CLASSES | ICC_STANDARD_CLASSES,
            };
            InitCommonControlsEx(&icc);

            let child = |class: &str, text: &str, style: u32, id: usize| -> HWND {
                let class = wide(class);
                let text = wide(text);
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    text.as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | style,
                    0,
                    0,
                    0,
                    0,
                    parent,
                    id as HMENU,
                    GetModuleHandleW(null()),
                    null(),
                )
            };

            let button = WS_TABSTOP | BS_PUSHBUTTON as u32;
            let bar = Toolbar {
                tabs: child("SysTabControl32", "", TCS_FOCUSNEVER, ID_TABS),
                new_tab: child("BUTTON", "+", button, ID_NEW_TAB.into()),
                close_tab: child("BUTTON", "✕", button, ID_CLOSE_TAB.into()),
                back: child("BUTTON", "←", button, ID_BACK.into()),
                forward: child("BUTTON", "→", button, ID_FORWARD.into()),
                reload: child("BUTTON", "⟳", button, ID_RELOAD.into()),
                address: child(
                    "EDIT",
                    "",
                    WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
                    ID_ADDRESS.into(),
                ),
            };

            // Sistem arayüz yazı tipini kullan (Windows 11'de Segoe UI).
            let mut metrics: NONCLIENTMETRICSW = std::mem::zeroed();
            metrics.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
            SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                (&raw mut metrics).cast(),
                0,
            );
            let font: HFONT = CreateFontIndirectW(&metrics.lfMessageFont);
            for hwnd in bar.all() {
                SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1);
            }

            // Proxy uygulama boyunca yaşar; alt sınıflama işlevleri onu
            // `dwrefdata` üzerinden okur.
            let proxy = Box::into_raw(Box::new(proxy)) as usize;
            SetWindowSubclass(parent, Some(main_proc), SUBCLASS_MAIN, proxy);
            SetWindowSubclass(bar.address, Some(address_proc), SUBCLASS_ADDRESS, proxy);

            bar
        }
    }

    fn all(&self) -> [HWND; 7] {
        [
            self.tabs,
            self.new_tab,
            self.close_tab,
            self.back,
            self.forward,
            self.reload,
            self.address,
        ]
    }

    /// Araç çubuğunun fiziksel piksel cinsinden toplam yüksekliği.
    pub fn height(scale: f64) -> u32 {
        ((TAB_ROW + BAR_ROW) * scale).round() as u32
    }

    /// Kontrolleri pencerenin fiziksel genişliğine göre yerleştirir.
    pub fn layout(&self, width: u32, scale: f64) {
        let px = |v: f64| (v * scale).round() as i32;
        let width = width as i32;
        let (tab_h, bar_h, btn, gap) = (px(TAB_ROW), px(BAR_ROW), px(BUTTON), px(GAP));
        unsafe {
            // Üst satır: sekmeler, sağda "+" ve "✕".
            let tabs_w = (width - 2 * (btn + gap) - gap).max(0);
            MoveWindow(self.tabs, gap, gap, tabs_w, tab_h - gap, 1);
            MoveWindow(self.new_tab, gap + tabs_w + gap, gap, btn, tab_h - gap, 1);
            MoveWindow(self.close_tab, width - btn - gap, gap, btn, tab_h - gap, 1);

            // Alt satır: geri, ileri, yenile, adres kutusu.
            let y = tab_h + gap / 2;
            let h = bar_h - gap;
            let mut x = gap;
            for hwnd in [self.back, self.forward, self.reload] {
                MoveWindow(hwnd, x, y, btn, h, 1);
                x += btn + gap;
            }
            MoveWindow(self.address, x, y, (width - x - gap).max(0), h, 1);
        }
    }

    pub fn set_address(&self, url: &str) {
        let text = wide(url);
        unsafe { SetWindowTextW(self.address, text.as_ptr()) };
    }

    /// Adres kutusuna odaklanır ve içindeki metni seçer.
    pub fn focus_address(&self) {
        unsafe {
            SetFocus(self.address);
            SendMessageW(self.address, EM_SETSEL, 0, -1);
        }
    }

    pub fn insert_tab(&self, index: usize, title: &str) {
        let mut text = wide(&tab_label(title));
        let item = TCITEMW {
            mask: TCIF_TEXT,
            pszText: text.as_mut_ptr(),
            iImage: -1,
            ..Default::default()
        };
        unsafe { SendMessageW(self.tabs, TCM_INSERTITEMW, index, &raw const item as LPARAM) };
    }

    pub fn set_tab_title(&self, index: usize, title: &str) {
        let mut text = wide(&tab_label(title));
        let item = TCITEMW {
            mask: TCIF_TEXT,
            pszText: text.as_mut_ptr(),
            iImage: -1,
            ..Default::default()
        };
        unsafe { SendMessageW(self.tabs, TCM_SETITEMW, index, &raw const item as LPARAM) };
    }

    pub fn remove_tab(&self, index: usize) {
        unsafe { SendMessageW(self.tabs, TCM_DELETEITEM, index, 0) };
    }

    pub fn select_tab(&self, index: usize) {
        unsafe { SendMessageW(self.tabs, TCM_SETCURSEL, index, 0) };
    }
}

/// Sekme başlıklarını sabit bir uzunlukta keser.
fn tab_label(title: &str) -> String {
    const MAX: usize = 24;
    let title = title.trim();
    let title = if title.is_empty() {
        "Yeni sekme"
    } else {
        title
    };
    if title.chars().count() <= MAX {
        title.to_string()
    } else {
        let short: String = title.chars().take(MAX - 1).collect();
        format!("{short}…")
    }
}

unsafe fn proxy<'a>(refdata: usize) -> &'a EventLoopProxy<UserEvent> {
    unsafe { &*(refdata as *const EventLoopProxy<UserEvent>) }
}

/// Ana pencerenin mesajlarını yakalar: buton tıklamaları ve sekme seçimi.
unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND if (wparam >> 16) as u32 == BN_CLICKED => {
                let event = match (wparam & 0xFFFF) as u16 {
                    ID_BACK => Some(UserEvent::Back),
                    ID_FORWARD => Some(UserEvent::Forward),
                    ID_RELOAD => Some(UserEvent::Reload),
                    ID_NEW_TAB => Some(UserEvent::NewTab),
                    ID_CLOSE_TAB => Some(UserEvent::CloseTab),
                    _ => None,
                };
                if let Some(event) = event {
                    let _ = proxy(refdata).send_event(event);
                    return 0;
                }
            }
            WM_NOTIFY => {
                let header = &*(lparam as *const NMHDR);
                if header.idFrom == ID_TABS && header.code == TCN_SELCHANGE {
                    let index = SendMessageW(header.hwndFrom, TCM_GETCURSEL, 0, 0);
                    if index >= 0 {
                        let _ = proxy(refdata).send_event(UserEvent::SelectTab(index as usize));
                    }
                    return 0;
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// Adres kutusunda Enter'a basılınca adrese gider, Esc ile sayfaya döner.
unsafe extern "system" fn address_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_KEYDOWN if wparam == VK_RETURN as WPARAM => {
                let len = GetWindowTextLengthW(hwnd);
                let mut buf = vec![0u16; len as usize + 1];
                let read = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
                let text = String::from_utf16_lossy(&buf[..read.max(0) as usize]);
                let _ = proxy(refdata).send_event(UserEvent::Navigate(text));
                return 0;
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as WPARAM => {
                let _ = proxy(refdata).send_event(UserEvent::FocusPage);
                return 0;
            }
            // Enter/Esc karakterlerini yut, yoksa Windows "bip" sesi çıkarır.
            WM_CHAR if wparam == 0x0D || wparam == 0x1B => return 0,
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}
