//! Tarayıcı arayüzünün Windows tarafı: çizim, fare, adres kutusu.
//!
//! Arayüz (sekmeler, butonlar) `ui::layout` ile yerleştirilir ve Direct2D ile
//! ana pencerenin kendisine çizilir. Tek yerel kontrol adres kutusudur
//! (Win32 EDIT): Türkçe klavye, IME, seçim ve pano gibi metin düzenleme
//! ayrıntıları için Windows'un kendi kontrolünü kullanmak en güvenlisi.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use tao::event_loop::EventLoopProxy;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmGetColorizationColor, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DeleteObject, FW_NORMAL,
    HBRUSH, HDC, HFONT, InvalidateRect, OUT_DEFAULT_PRECIS, ScreenToClient, SetBkColor,
    SetTextColor, ValidateRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::Controls::{EM_SETCUEBANNER, EM_SETSEL, WM_MOUSELEAVE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_CONTROL, VK_ESCAPE, VK_MENU, VK_RETURN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, ES_AUTOHSCROLL, GWL_STYLE, GetWindowLongPtrW, GetWindowTextLengthW,
    GetWindowTextW, MoveWindow, SendMessageW, SetWindowLongPtrW, SetWindowTextW, WM_CHAR,
    WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETFONT, WM_SETTINGCHANGE, WM_SYSCHAR, WM_SYSKEYDOWN,
    WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_TABSTOP, WS_VISIBLE,
};

use crate::UserEvent;
use crate::shortcuts::{self, Modifiers};
use crate::ui::d2d::Renderer;
use crate::ui::layout::{self, Layout, Model, Pointer, Target};
use crate::ui::theme::{Appearance, Color, Theme};

const SUBCLASS_MAIN: usize = 1;
const SUBCLASS_ADDRESS: usize = 2;

// DWM pencere öznitelikleri (Windows 10 20H1+ ve Windows 11).
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_TEXT_COLOR: u32 = 36;

pub struct Chrome {
    hwnd: HWND,
    edit: HWND,
    renderer: Option<Renderer>,
    appearance: Appearance,
    theme: Theme,
    system_dark: bool,
    system_accent: Color,
    model: Model,
    layout: Layout,
    pointer: Pointer,
    scale: f32,
    size: (u32, u32),
    edit_font: HFONT,
    edit_brush: HBRUSH,
    tracking_leave: bool,
    /// Site ikonları: anahtar (alan adı) → (sürüm, PNG).
    favicons: std::collections::HashMap<String, (u64, Vec<u8>)>,
    favicon_version: u64,
    /// Adres kutusunun son konumu (gereksiz taşımaları önlemek için).
    edit_rect: std::cell::Cell<(i32, i32, i32, i32)>,
}

/// Alt sınıflama işlevlerinin erişebildiği paylaşılan durum.
struct Shared {
    chrome: Rc<RefCell<Chrome>>,
    proxy: EventLoopProxy<UserEvent>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl Chrome {
    /// Arayüzü `parent` penceresinde oluşturur.
    pub fn create(
        parent: HWND,
        proxy: EventLoopProxy<UserEvent>,
        appearance: Appearance,
        scale: f32,
        size: (u32, u32),
    ) -> Rc<RefCell<Chrome>> {
        let (system_dark, system_accent) = (system_dark_mode(), system_accent());
        let theme = Theme::new(&appearance, system_dark, system_accent);
        unsafe {
            // Arayüz çizilirken alt pencereler (sayfalar, adres kutusu) üzerine çizilmesin.
            let style = GetWindowLongPtrW(parent, GWL_STYLE);
            SetWindowLongPtrW(parent, GWL_STYLE, style | WS_CLIPCHILDREN as isize);

            let class = wide("EDIT");
            let edit = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            let cue = wide("Ara ya da adres yaz");
            SendMessageW(edit, EM_SETCUEBANNER, 1, cue.as_ptr() as LPARAM);

            let renderer = Renderer::new(windows::Win32::Foundation::HWND(parent))
                .map_err(|e| crate::log::error(&format!("Direct2D başlatılamadı: {e}")))
                .ok();

            let chrome = Rc::new(RefCell::new(Chrome {
                hwnd: parent,
                edit,
                renderer,
                appearance,
                theme,
                system_dark,
                system_accent,
                model: Model::default(),
                layout: Layout::default(),
                pointer: Pointer::default(),
                scale,
                size,
                edit_font: null_mut(),
                edit_brush: null_mut(),
                tracking_leave: false,
                edit_rect: std::cell::Cell::new((0, 0, 0, 0)),
                favicons: std::collections::HashMap::new(),
                favicon_version: 0,
            }));
            {
                let mut c = chrome.borrow_mut();
                c.rebuild_edit_style();
                c.apply_window_colors();
                c.relayout();
            }

            let shared = Box::into_raw(Box::new(Shared {
                chrome: Rc::clone(&chrome),
                proxy,
            })) as usize;
            SetWindowSubclass(parent, Some(main_proc), SUBCLASS_MAIN, shared);
            SetWindowSubclass(edit, Some(address_proc), SUBCLASS_ADDRESS, shared);
            chrome
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn system_accent(&self) -> Color {
        self.system_accent
    }

    /// Pencere boyutu (fiziksel piksel) ya da ekran ölçeği değişti.
    pub fn set_size(&mut self, width: u32, height: u32, scale: f32) {
        let scale_changed = (scale - self.scale).abs() > 0.001;
        self.size = (width, height);
        self.scale = scale;
        if scale_changed {
            self.rebuild_edit_style();
        }
        self.relayout();
    }

    /// Sekmeler, butonların durumu gibi içerik değişti.
    pub fn set_model(&mut self, model: Model) {
        if self.model != model {
            let scroll = self.model.tab_scroll;
            self.model = model;
            self.model.tab_scroll = scroll;
            self.relayout();
        }
    }

    /// Görünüm ayarları değişti.
    pub fn set_appearance(&mut self, appearance: Appearance) {
        self.appearance = appearance;
        self.retheme();
    }

    /// Windows'un koyu/açık teması ya da vurgu rengi değişmiş olabilir.
    /// Bir şey değiştiyse `true`.
    pub fn refresh_system(&mut self) -> bool {
        let (dark, accent) = (system_dark_mode(), system_accent());
        if dark == self.system_dark && accent == self.system_accent {
            return false;
        }
        self.system_dark = dark;
        self.system_accent = accent;
        self.retheme();
        true
    }

    fn retheme(&mut self) {
        self.theme = Theme::new(&self.appearance, self.system_dark, self.system_accent);
        self.rebuild_edit_style();
        self.apply_window_colors();
        self.relayout();
        unsafe { InvalidateRect(self.edit, null(), 1) };
    }

    /// Sayfanın yeri (fiziksel piksel): x, y, genişlik, yükseklik.
    pub fn content_px(&self) -> (i32, i32, u32, u32) {
        let r = self.layout.content;
        let s = self.scale;
        let x = (r.x * s).round() as i32;
        let y = (r.y * s).round() as i32;
        let right = (r.right() * s).round() as i32;
        let bottom = (r.bottom() * s).round() as i32;
        (x, y, (right - x).max(0) as u32, (bottom - y).max(0) as u32)
    }

    /// Sayfa çerçevesinin köşe yarıçapı (fiziksel piksel).
    pub fn content_radius_px(&self) -> i32 {
        (self.layout.content_radius * self.scale).round() as i32
    }

    /// Sitenin ikonunu ekler ya da günceller.
    pub fn set_favicon(&mut self, key: &str, png: Vec<u8>) {
        if self.favicons.get(key).is_some_and(|(_, old)| *old == png) {
            return;
        }
        self.favicon_version += 1;
        self.favicons
            .insert(key.to_string(), (self.favicon_version, png));
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }

    pub fn set_address(&self, text: &str) {
        let text = wide(text);
        unsafe { SetWindowTextW(self.edit, text.as_ptr()) };
    }

    /// Adres kutusuna odaklanır ve içindeki metni seçer.
    pub fn focus_address(&self) {
        unsafe {
            SetFocus(self.edit);
            SendMessageW(self.edit, EM_SETSEL, 0, -1);
        }
    }

    fn relayout(&mut self) {
        let (w, h) = (
            self.size.0 as f32 / self.scale,
            self.size.1 as f32 / self.scale,
        );
        self.layout = layout::layout(
            self.appearance.layout,
            &self.theme,
            &self.model,
            w,
            h,
            self.pointer,
        );
        if self.layout.scroll_max < self.model.tab_scroll {
            self.model.tab_scroll = self.layout.scroll_max;
        }
        self.place_edit();
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }

    /// Adres kutusunu yerleştirir: tek satırlık EDIT yazıyı üstten çizdiği
    /// için yüksekliği tam bir satır yapılıp alanın ortasına konur.
    fn place_edit(&self) {
        let r = self.layout.address;
        let s = self.scale;
        let line = (self.theme.metrics.font * 1.45 * s).round();
        let x = (r.x * s).round() as i32;
        let w = (r.w * s).round() as i32;
        let y = (r.y * s + (r.h * s - line) / 2.0).round() as i32;
        let rect = (x, y, w.max(0), line as i32);
        if self.edit_rect.replace(rect) != rect {
            unsafe { MoveWindow(self.edit, rect.0, rect.1, rect.2, rect.3, 1) };
        }
    }

    fn rebuild_edit_style(&mut self) {
        unsafe {
            let face = wide("Segoe UI");
            let height = -(self.theme.metrics.font * self.scale).round() as i32;
            let font = CreateFontW(
                height,
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                face.as_ptr(),
            );
            SendMessageW(self.edit, WM_SETFONT, font as WPARAM, 1);
            if !self.edit_font.is_null() {
                DeleteObject(self.edit_font);
            }
            self.edit_font = font;

            let brush = CreateSolidBrush(self.theme.palette.surface.colorref());
            if !self.edit_brush.is_null() {
                DeleteObject(self.edit_brush);
            }
            self.edit_brush = brush;
        }
    }

    /// Windows'un başlık çubuğunu temaya uydurur (Windows 11'de renk,
    /// Windows 10'da yalnızca koyu/açık).
    fn apply_window_colors(&self) {
        let p = &self.theme.palette;
        let dark: i32 = self.theme.dark.into();
        let caption = p.ground.colorref();
        let text = p.text.colorref();
        unsafe {
            DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                4,
            );
            DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_CAPTION_COLOR,
                (&raw const caption).cast(),
                4,
            );
            DwmSetWindowAttribute(self.hwnd, DWMWA_TEXT_COLOR, (&raw const text).cast(), 4);
        }
    }

    fn paint(&mut self) {
        let (w, h) = self.size;
        if w == 0 || h == 0 {
            return;
        }
        if let Some(r) = self.renderer.as_mut()
            && let Err(e) = r.render(&self.layout.prims, &self.favicons, w, h, self.scale)
        {
            crate::log::error(&format!("arayüz çizilemedi: {e}"));
        }
    }

    /// Fiziksel pikseli DIP'e çevirip noktadaki öğeyi bulur.
    fn hit(&self, lparam: LPARAM) -> Option<Target> {
        let (x, y) = point(lparam);
        self.layout
            .hit(x as f32 / self.scale, y as f32 / self.scale)
    }

    fn set_pointer(&mut self, pointer: Pointer) {
        if self.pointer != pointer {
            self.pointer = pointer;
            self.relayout();
        }
    }

    fn scroll(&mut self, x: i32, y: i32, delta: i16) {
        let Some(area) = self.layout.scroll_area else {
            return;
        };
        let (dx, dy) = (x as f32 / self.scale, y as f32 / self.scale);
        if !area.contains(dx, dy) || self.layout.scroll_max <= 0.0 {
            return;
        }
        let step = self.theme.metrics.row * 1.5 * (delta as f32 / 120.0);
        self.model.tab_scroll = (self.model.tab_scroll - step).clamp(0.0, self.layout.scroll_max);
        self.relayout();
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        unsafe {
            if !self.edit_font.is_null() {
                DeleteObject(self.edit_font);
            }
            if !self.edit_brush.is_null() {
                DeleteObject(self.edit_brush);
            }
        }
    }
}

/// `lParam` içindeki işaretli x, y koordinatları.
fn point(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam & 0xffff) as i16 as i32,
        ((lparam >> 16) & 0xffff) as i16 as i32,
    )
}

/// Tıklanan öğenin tarayıcı komutu.
fn command(target: Target) -> UserEvent {
    match target {
        Target::Back => UserEvent::Back,
        Target::Forward => UserEvent::Forward,
        Target::Reload => UserEvent::Reload,
        Target::Tab(i) => UserEvent::SelectTab(i),
        Target::CloseTab(i) => UserEvent::CloseTabAt(i),
        Target::NewTab => UserEvent::NewTab,
        Target::Star => UserEvent::ToggleBookmark,
        Target::Shield => UserEvent::ToggleBlocking,
        Target::History => UserEvent::OpenHistory,
        Target::Settings => UserEvent::OpenSettings,
        Target::Bookmark(i) => UserEvent::OpenBookmark(i),
    }
}

/// Windows uygulamalar için koyu tema mı kullanıyor?
fn system_dark_mode() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut value: u32 = 1;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    status == 0 && value == 0
}

/// Windows'un vurgu rengi (alınamazsa Windows'un varsayılan mavisi).
fn system_accent() -> Color {
    let mut argb = 0u32;
    let mut opaque = 0;
    let ok = unsafe { DwmGetColorizationColor(&mut argb, &mut opaque) } >= 0;
    if ok {
        Color::hex(argb & 0x00ff_ffff)
    } else {
        Color::hex(0x0078d4)
    }
}

unsafe fn shared<'a>(refdata: usize) -> &'a Shared {
    unsafe { &*(refdata as *const Shared) }
}

/// Ana pencerenin mesajları: çizim, fare, sistem teması.
unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        let s = shared(refdata);
        // Arayüz zaten kullanılıyorsa (iç içe çağrı) varsayılana bırak.
        let Ok(mut c) = s.chrome.try_borrow_mut() else {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_PAINT => {
                c.paint();
                ValidateRect(hwnd, null());
                return 0;
            }
            WM_ERASEBKGND => return 1,
            WM_MOUSEMOVE => {
                if !c.tracking_leave {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    c.tracking_leave = true;
                }
                let hover = c.hit(lparam);
                let pointer = Pointer { hover, ..c.pointer };
                c.set_pointer(pointer);
            }
            WM_MOUSELEAVE => {
                c.tracking_leave = false;
                let pointer = Pointer {
                    hover: None,
                    ..c.pointer
                };
                c.set_pointer(pointer);
            }
            WM_LBUTTONDOWN => {
                let target = c.hit(lparam);
                if target.is_some() {
                    SetCapture(hwnd);
                }
                let pointer = Pointer {
                    pressed: target,
                    ..c.pointer
                };
                c.set_pointer(pointer);
                return 0;
            }
            WM_LBUTTONUP => {
                let pressed = c.pointer.pressed;
                let target = c.hit(lparam);
                let pointer = Pointer {
                    pressed: None,
                    hover: target,
                };
                c.set_pointer(pointer);
                drop(c);
                ReleaseCapture();
                if let (Some(t), Some(p)) = (target, pressed)
                    && t == p
                {
                    let _ = s.proxy.send_event(command(t));
                }
                return 0;
            }
            WM_MBUTTONUP => {
                // Orta tıkla sekme kapatma.
                if let Some(Target::Tab(i) | Target::CloseTab(i)) = c.hit(lparam) {
                    let _ = s.proxy.send_event(UserEvent::CloseTabAt(i));
                }
                return 0;
            }
            WM_MOUSEWHEEL => {
                let (sx, sy) = point(lparam);
                let mut p = POINT { x: sx, y: sy };
                ScreenToClient(hwnd, &mut p);
                c.scroll(p.x, p.y, (wparam >> 16) as u16 as i16);
                return 0;
            }
            WM_CTLCOLOREDIT => {
                let hdc = wparam as HDC;
                SetTextColor(hdc, c.theme.palette.text.colorref());
                SetBkColor(hdc, c.theme.palette.surface.colorref());
                return c.edit_brush as LRESULT;
            }
            WM_SETTINGCHANGE => {
                let _ = s.proxy.send_event(UserEvent::SystemThemeChanged);
            }
            _ => {}
        }
        drop(c);
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

thread_local! {
    /// Kısayol olarak işlenen tuşun ardından gelen karakter mesajı yutulsun mu?
    static SWALLOW_CHAR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Adres kutusu: Enter'da adrese gider, Esc ile sayfaya döner, kısayollar.
unsafe extern "system" fn address_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        let proxy = &shared(refdata).proxy;
        match msg {
            WM_KEYDOWN if wparam == VK_RETURN as WPARAM => {
                let len = GetWindowTextLengthW(hwnd);
                let mut buf = vec![0u16; len as usize + 1];
                let read = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
                let text = String::from_utf16_lossy(&buf[..read.max(0) as usize]);
                let _ = proxy.send_event(UserEvent::Navigate(text));
                return 0;
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as WPARAM => {
                let _ = proxy.send_event(UserEvent::FocusPage);
                return 0;
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                SWALLOW_CHAR.set(false);
                let down = |vk: u16| GetKeyState(vk.into()) < 0;
                let modifiers = Modifiers {
                    ctrl: down(VK_CONTROL),
                    shift: down(VK_SHIFT),
                    alt: down(VK_MENU),
                };
                if let Some(event) = shortcuts::lookup(wparam as u32, modifiers) {
                    SWALLOW_CHAR.set(true);
                    let _ = proxy.send_event(event);
                    return 0;
                }
            }
            // Enter/Esc karakterlerini yut, yoksa Windows "bip" sesi çıkarır.
            WM_CHAR if wparam == 0x0D || wparam == 0x1B => return 0,
            // Kısayolun ürettiği karakteri (Ctrl+L → 0x0C gibi) kutuya yazma.
            WM_CHAR | WM_SYSCHAR if SWALLOW_CHAR.replace(false) => return 0,
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}
