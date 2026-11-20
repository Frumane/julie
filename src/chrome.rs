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
    CLEARTYPE_QUALITY, ClientToScreen, CreateFontW, CreateRoundRectRgn, CreateSolidBrush,
    DEFAULT_CHARSET, DeleteObject, FW_NORMAL, HBRUSH, HDC, HFONT, InvalidateRect,
    OUT_DEFAULT_PRECIS, ScreenToClient, SetBkColor, SetTextColor, SetWindowRgn, ValidateRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::Controls::{
    EM_GETSEL, EM_SETCUEBANNER, EM_SETSEL, TOOLTIPS_CLASSW, TTF_SUBCLASS, TTM_ADDTOOLW,
    TTM_NEWTOOLRECTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW, WM_MOUSELEAVE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_MENU, VK_RETURN,
    VK_SHIFT, VK_UP,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, EN_CHANGE, ES_AUTOHSCROLL, GWL_STYLE,
    GetCursorPos, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HTCLIENT, IDC_ARROW,
    IDC_SIZEWE, IsIconic, KillTimer, LoadCursorW, MA_NOACTIVATE, MoveWindow, RegisterClassExW,
    SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SendMessageW, SetCursor,
    SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WM_ACTIVATE, WM_CHAR,
    WM_COMMAND, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE,
    WM_NCACTIVATE, WM_PAINT, WM_SETCURSOR, WM_SETFONT, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCHAR,
    WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};

use crate::UserEvent;
use crate::shortcuts::{self, Modifiers};
use crate::ui::d2d::Renderer;
use crate::ui::layout::{
    self, AskInfo, AskPanel, AskTarget, FindBar, FindState, FindTarget, Layout, Model, Pointer,
    Popup, SiteInfo, SitePanel, SuggestionRow, Target,
};
use crate::ui::theme::{Appearance, Color, Theme};

const SUBCLASS_MAIN: usize = 1;
const SUBCLASS_ADDRESS: usize = 2;
const SUBCLASS_POPUP: usize = 3;
const SUBCLASS_FIND: usize = 4;
const SUBCLASS_FIND_EDIT: usize = 5;
const SUBCLASS_SITE: usize = 6;
const SUBCLASS_ASK: usize = 7;

// DWM pencere öznitelikleri (Windows 10 20H1+ ve Windows 11).
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_TEXT_COLOR: u32 = 36;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: u32 = 2;

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
    /// Adres çubuğu önerileri: kutunun altında açılan ayrı pencere.
    suggest: Suggest,
    /// Basılı tutulan ya da sürüklenen sekme.
    drag: Option<TabDrag>,
    /// Sayfada bul kutusu (Ctrl+F): sayfanın üstünde duran alt pencere.
    find: Find,
    /// Sağdaki geliştirici araçları panelinin kapladığı genişlik (mantıksal
    /// piksel, aradaki boşluk dahil). Bulma kutusu sayfanın üstünde kalsın.
    side_panel: f32,
    /// Sayfa ile araç paneli arasındaki ayracın yeri (fiziksel piksel x
    /// aralığı) ve sürükleniyor mu.
    splitter: Option<(i32, i32)>,
    splitter_drag: bool,
    /// Yeni yerine kayarak giden sekmeler.
    slides: Vec<Slide>,
    /// Gönderilen taşıma ve yer değiştirecek sekmeler; yeni sıra gelince
    /// animasyon başlar.
    pending_move: Option<PendingMove>,
    /// Kilide tıklanınca açılan site bilgisi paneli.
    site: Site,
    /// Site izin isteyince kilidin altında çıkan soru.
    ask: Ask,
}

/// İzin sorusu: odak almaz (sayfa kullanılmaya devam eder) ve dışarı
/// tıklanınca kapanmaz; yanıtlanınca ya da sayfadan çıkılınca kapanır.
struct Ask {
    hwnd: HWND,
    renderer: Option<Renderer>,
    info: Option<AskInfo>,
    panel: AskPanel,
    hover: Option<AskTarget>,
    size: (u32, u32),
    visible: bool,
}

/// Site bilgisi paneli: odak alan ayrı pencere; dışarı tıklanınca kapanır.
struct Site {
    hwnd: HWND,
    renderer: Option<Renderer>,
    info: Option<SiteInfo>,
    panel: SitePanel,
    hover: Option<usize>,
    /// Pencerenin fiziksel boyutu.
    size: (u32, u32),
    visible: bool,
    /// Son kapanma anı: kilide tıklayınca kapanan panel aynı tıklamayla
    /// yeniden açılmasın.
    hidden_at: Option<std::time::Instant>,
}

/// Taşınan sekme (nereden, nereye) ve yer değiştirecek komşular
/// (yeni sırası, eski yerine göre kayma).
type PendingMove = (usize, usize, Vec<(usize, f32, f32)>);

/// Yeni yerine kayan bir sekme: başlangıçtaki kayması ve başladığı an.
struct Slide {
    index: usize,
    dx: f32,
    dy: f32,
    started: std::time::Instant,
}

impl Slide {
    fn offset(&self, now: std::time::Instant) -> (f32, f32) {
        let ms = now.duration_since(self.started).as_secs_f32() * 1000.0;
        layout::slide_offset(self.dx, self.dy, ms)
    }
}

/// Sekme animasyonu zamanlayıcısı (~60 kare/sn).
const SLIDE_TIMER: usize = 1;

/// Sayfada bul kutusu.
struct Find {
    hwnd: HWND,
    edit: HWND,
    renderer: Option<Renderer>,
    state: FindState,
    bar: FindBar,
    visible: bool,
    /// Pencerenin fiziksel boyutu.
    size: (u32, u32),
    tracking_leave: bool,
    /// WebView2 seçili eşleşmenin sırasını 0'dan mı sayıyor?
    zero_based: bool,
    /// Düğmelerin ipuçları (Windows'un ipucu penceresi).
    tooltip: HWND,
}

/// Sekme sürükleme durumu.
struct TabDrag {
    /// Sürüklenen sekmenin şu anki sırası.
    index: usize,
    /// Basılan nokta (DIP); bu kadar oynamadan sürükleme başlamaz.
    start: (f32, f32),
    /// İmlecin sekmenin sol üst köşesine uzaklığı: sekme imleçle kayar.
    grab: (f32, f32),
    moving: bool,
    /// Taşıma isteği gönderildi, yeni sıra henüz arayüze gelmedi.
    waiting: bool,
}

/// Fare bu kadar (DIP) oynayınca tıklama sürüklemeye döner.
const DRAG_THRESHOLD: f32 = 5.0;

/// Öneri listesinin durumu.
struct Suggest {
    hwnd: HWND,
    renderer: Option<Renderer>,
    rows: Vec<SuggestionRow>,
    /// Satır seçilince adres çubuğuna verilecek metin (satır sırası).
    targets: Vec<String>,
    selected: usize,
    hover: Option<usize>,
    popup: Popup,
    /// Pencerenin fiziksel boyutu.
    size: (u32, u32),
    visible: std::cell::Cell<bool>,
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
            let popup = create_popup(parent);
            let (find_hwnd, find_edit, find_tooltip) = create_find(parent);
            let find_renderer = Renderer::new(windows::Win32::Foundation::HWND(find_hwnd))
                .map_err(|e| crate::log::error(&format!("bulma kutusu çizilemiyor: {e}")))
                .ok();
            let popup_renderer = Renderer::new(windows::Win32::Foundation::HWND(popup))
                .map_err(|e| crate::log::error(&format!("öneri listesi çizilemiyor: {e}")))
                .ok();
            let site_hwnd = create_site(parent);
            let ask_hwnd = create_popup(parent);
            let ask_renderer = Renderer::new(windows::Win32::Foundation::HWND(ask_hwnd))
                .map_err(|e| crate::log::error(&format!("izin sorusu çizilemiyor: {e}")))
                .ok();
            let site_renderer = Renderer::new(windows::Win32::Foundation::HWND(site_hwnd))
                .map_err(|e| crate::log::error(&format!("site paneli çizilemiyor: {e}")))
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
                suggest: Suggest {
                    hwnd: popup,
                    renderer: popup_renderer,
                    rows: Vec::new(),
                    targets: Vec::new(),
                    selected: 0,
                    hover: None,
                    popup: Popup::default(),
                    size: (0, 0),
                    visible: std::cell::Cell::new(false),
                },
                drag: None,
                side_panel: 0.0,
                splitter: None,
                splitter_drag: false,
                find: Find {
                    hwnd: find_hwnd,
                    edit: find_edit,
                    renderer: find_renderer,
                    state: FindState::default(),
                    bar: FindBar::default(),
                    visible: false,
                    size: (0, 0),
                    tracking_leave: false,
                    zero_based: false,
                    tooltip: find_tooltip,
                },
                slides: Vec::new(),
                pending_move: None,
                site: Site {
                    hwnd: site_hwnd,
                    renderer: site_renderer,
                    info: None,
                    panel: SitePanel::default(),
                    hover: None,
                    size: (0, 0),
                    visible: false,
                    hidden_at: None,
                },
                ask: Ask {
                    hwnd: ask_hwnd,
                    renderer: ask_renderer,
                    info: None,
                    panel: AskPanel::default(),
                    hover: None,
                    size: (0, 0),
                    visible: false,
                },
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
            SetWindowSubclass(popup, Some(popup_proc), SUBCLASS_POPUP, shared);
            SetWindowSubclass(find_hwnd, Some(find_proc), SUBCLASS_FIND, shared);
            SetWindowSubclass(find_edit, Some(find_edit_proc), SUBCLASS_FIND_EDIT, shared);
            SetWindowSubclass(site_hwnd, Some(site_proc), SUBCLASS_SITE, shared);
            SetWindowSubclass(ask_hwnd, Some(ask_proc), SUBCLASS_ASK, shared);
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
        // Gönderilen taşıma işlendi; sürükleme yeni sırayla devam edebilir.
        if let Some(drag) = &mut self.drag {
            drag.waiting = false;
        }
        // Yeni sıra geldi: yer değiştiren sekmeler eski yerlerinden kaysın.
        if let Some((from, to, moved)) = self.pending_move.take() {
            let now = std::time::Instant::now();
            for slide in &mut self.slides {
                // Süren animasyonlar da yeni sıraya taşınır.
                let (dx, dy) = slide.offset(now);
                *slide = Slide {
                    index: layout::follow_move(slide.index, from, to),
                    dx,
                    dy,
                    started: now,
                };
            }
            for (index, dx, dy) in moved {
                match self.slides.iter_mut().find(|s| s.index == index) {
                    Some(s) => {
                        s.dx += dx;
                        s.dy += dy;
                    }
                    None => self.slides.push(Slide {
                        index,
                        dx,
                        dy,
                        started: now,
                    }),
                }
            }
            self.start_slides();
        }
        let mut model = model;
        model.tab_scroll = self.model.tab_scroll;
        model.tab_shift = self.model.tab_shift.clone();
        if self.model != model {
            self.model = model;
            self.relayout();
        } else if !self.slides.is_empty() {
            self.relayout();
        }
    }

    fn start_slides(&self) {
        if !self.slides.is_empty() {
            unsafe { SetTimer(self.hwnd, SLIDE_TIMER, 15, None) };
        }
    }

    /// Animasyonun bir karesi: biten kaymaları atar, gerisini yeniden çizer.
    fn tick_slides(&mut self) {
        let now = std::time::Instant::now();
        self.slides
            .retain(|s| now.duration_since(s.started).as_secs_f32() * 1000.0 < layout::SLIDE_MS);
        if self.slides.is_empty() {
            unsafe { KillTimer(self.hwnd, SLIDE_TIMER) };
        }
        self.relayout();
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
        self.hide_suggestions();
        self.hide_site();
        self.theme = Theme::new(&self.appearance, self.system_dark, self.system_accent);
        self.rebuild_edit_style();
        self.apply_window_colors();
        self.relayout();
        unsafe { InvalidateRect(self.edit, null(), 1) };
        if self.ask.visible {
            self.layout_ask();
            self.place_ask();
        }
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
        self.hide_suggestions();
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

    // -------------------------------------------------------- sayfada bul

    /// Bulma kutusunu açıp yazı alanına odaklanır; içindeki metni döndürür
    /// (önceki aramadan kalmışsa yeniden aranır).
    pub fn open_find(&mut self) -> String {
        self.find.visible = true;
        self.find.state.has_text = !edit_text(self.find.edit).is_empty();
        self.place_find();
        unsafe {
            SetFocus(self.find.edit);
            SendMessageW(self.find.edit, EM_SETSEL, 0, -1);
        }
        edit_text(self.find.edit)
    }

    pub fn close_find(&mut self) {
        if !self.find.visible {
            return;
        }
        self.find.visible = false;
        self.find.state.active = 0;
        self.find.state.total = 0;
        unsafe { ShowWindow(self.find.hwnd, SW_HIDE) };
    }

    pub fn find_visible(&self) -> bool {
        self.find.visible
    }

    pub fn find_case(&self) -> bool {
        self.find.state.case_sensitive
    }

    pub fn set_find_count(&mut self, active: i32, total: i32) {
        // WebView2 seçili eşleşmeyi 0'dan sayıyorsa (eşleşme varken 0
        // gelirse) bundan sonra bir ekleyerek göster.
        if active == 0 && total > 0 {
            self.find.zero_based = true;
        }
        let shown = if self.find.zero_based && active >= 0 {
            active + 1
        } else {
            active
        };
        self.find.state.active = shown;
        self.find.state.total = total;
        self.redraw_find();
    }

    /// Öndeki sekmenin geliştirici araçları paneli ve sayfayla arasındaki
    /// boşluğun genişliği (fiziksel piksel, kapalıysa 0).
    pub fn set_side_panel(&mut self, panel_px: u32, gap_px: u32) {
        let width_px = panel_px + gap_px;
        self.splitter = (panel_px > 0).then(|| {
            let (x, _, w, _) = self.content_px();
            let left = x + w as i32 - width_px as i32;
            (left, left + gap_px as i32)
        });
        let side = width_px as f32 / self.scale.max(0.1);
        if (side - self.side_panel).abs() > 0.01 {
            self.side_panel = side;
            self.place_find();
        }
    }

    /// İmleç (istemci koordinatı) sayfa ile araç paneli arasındaki ayraçta mı?
    fn on_splitter(&self, (x, y): (i32, i32)) -> bool {
        let Some((left, right)) = self.splitter else {
            return false;
        };
        let (_, top, _, h) = self.content_px();
        x >= left - 2 && x <= right + 2 && y >= top && y < top + h as i32
    }

    /// Yeni oluşturulan sayfa pencereleri kutunun üstüne çıkmasın.
    pub fn raise_find(&self) {
        if self.find.visible {
            unsafe {
                SetWindowPos(
                    self.find.hwnd,
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            };
        }
    }

    fn redraw_find(&mut self) {
        self.find.bar = layout::find_bar(&self.theme, &self.find.state);
        unsafe { InvalidateRect(self.find.hwnd, null(), 0) };
    }

    /// Kutuyu ayarlardaki köşeye yerleştirir (sayfanın üstünde).
    fn place_find(&mut self) {
        if !self.find.visible {
            return;
        }
        self.find.bar = layout::find_bar(&self.theme, &self.find.state);
        let s = self.scale;
        let mut page = self.layout.content;
        page.w = (page.w - self.side_panel).max(0.0);
        let r = layout::find_rect(page, self.appearance.find);
        let (x, y) = ((r.x * s).round() as i32, (r.y * s).round() as i32);
        let (w, h) = (
            (r.w * s).round().max(1.0) as i32,
            (r.h * s).round().max(1.0) as i32,
        );
        let radius = (layout::FIND_RADIUS * 2.0 * s).round() as i32;
        let e = self.find.bar.edit;
        unsafe {
            if self.find.size != (w as u32, h as u32) {
                self.find.size = (w as u32, h as u32);
                SetWindowRgn(
                    self.find.hwnd,
                    CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius),
                    1,
                );
            }
            SetWindowPos(
                self.find.hwnd,
                null_mut(),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let line = (self.theme.metrics.font * 1.45 * s).round();
            MoveWindow(
                self.find.edit,
                (e.x * s).round() as i32,
                (e.y * s + (e.h * s - line) / 2.0).round() as i32,
                (e.w * s).round() as i32,
                line as i32,
                1,
            );
            InvalidateRect(self.find.hwnd, null(), 0);
        }
        self.place_find_tips();
    }

    /// İpuçlarını düğmelerin şimdiki yerine taşır.
    fn place_find_tips(&self) {
        let s = self.scale;
        for (id, target) in FindTarget::ALL.iter().enumerate() {
            let rect = self
                .find
                .bar
                .hits
                .iter()
                .find(|(_, t)| t == target)
                .map(|(r, _)| windows_sys::Win32::Foundation::RECT {
                    left: (r.x * s).round() as i32,
                    top: (r.y * s).round() as i32,
                    right: ((r.x + r.w) * s).round() as i32,
                    bottom: ((r.y + r.h) * s).round() as i32,
                })
                .unwrap_or(windows_sys::Win32::Foundation::RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                });
            let info = TTTOOLINFOW {
                cbSize: size_of::<TTTOOLINFOW>() as u32,
                hwnd: self.find.hwnd,
                uId: id,
                rect,
                ..Default::default()
            };
            unsafe {
                SendMessageW(
                    self.find.tooltip,
                    TTM_NEWTOOLRECTW,
                    0,
                    &info as *const _ as LPARAM,
                )
            };
        }
    }

    fn paint_find(&mut self) {
        let (w, h) = self.find.size;
        if w == 0 || h == 0 {
            return;
        }
        if let Some(r) = self.find.renderer.as_mut()
            && let Err(e) = r.render(&self.find.bar.prims, &self.favicons, w, h, self.scale)
        {
            crate::log::error(&format!("bulma kutusu çizilemedi: {e}"));
        }
    }

    fn find_hit(&self, lparam: LPARAM) -> Option<FindTarget> {
        let (x, y) = point(lparam);
        self.find
            .bar
            .hit(x as f32 / self.scale, y as f32 / self.scale)
    }

    /// Öneri listesini adres kutusunun altında gösterir. Kutu artık
    /// odakta değilse (kullanıcı sayfaya geçtiyse) gösterilmez.
    pub fn show_suggestions(&mut self, rows: Vec<SuggestionRow>, targets: Vec<String>) {
        if rows.is_empty() || unsafe { GetFocus() } != self.edit {
            self.hide_suggestions();
            return;
        }
        // Liste arka planda yenilenince (arama önerileri gelince) seçili
        // satır yerinde duruyorsa seçim korunur.
        let keep = self.suggest.visible.get()
            && targets.get(self.suggest.selected)
                == self.suggest.targets.get(self.suggest.selected);
        self.suggest.rows = rows;
        self.suggest.targets = targets;
        if !keep {
            self.suggest.selected = 0;
        }
        self.suggest.hover = None;
        self.layout_popup();
        let s = self.scale;
        let r = self.layout.address;
        let mut origin = POINT {
            x: ((r.x - POPUP_INSET) * s).round() as i32,
            y: ((r.bottom() + 6.0) * s).round() as i32,
        };
        let w = ((r.w + POPUP_INSET * 2.0) * s).round().max(1.0) as i32;
        let h = (self.suggest.popup.height * s).round().max(1.0) as i32;
        self.suggest.size = (w as u32, h as u32);
        unsafe {
            ClientToScreen(self.hwnd, &mut origin);
            SetWindowPos(
                self.suggest.hwnd,
                null_mut(),
                origin.x,
                origin.y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(self.suggest.hwnd, null(), 0);
        }
        self.suggest.visible.set(true);
    }

    /// Adres kutusunu geçmişten tamamlar: `typed` kutuda hâlâ aynen
    /// duruyorsa metni `completed` yapar ve eklenen kısmı seçer; kullanıcı
    /// yazmaya devam edince seçili kısım kendiliğinden silinir.
    pub fn complete_address(&self, typed: &str, completed: &str) {
        unsafe {
            if GetFocus() != self.edit || edit_text(self.edit) != typed {
                return;
            }
            let text = wide(completed);
            SetWindowTextW(self.edit, text.as_ptr());
            let start = typed.encode_utf16().count();
            SendMessageW(self.edit, EM_SETSEL, start, -1);
        }
    }

    pub fn suggestions_visible(&self) -> bool {
        self.suggest.visible.get()
    }

    pub fn hide_suggestions(&self) {
        if self.suggest.visible.replace(false) {
            unsafe { ShowWindow(self.suggest.hwnd, SW_HIDE) };
        }
    }

    fn layout_popup(&mut self) {
        let width = self.layout.address.w + POPUP_INSET * 2.0;
        self.suggest.popup = layout::suggestions(
            &self.theme,
            &self.suggest.rows,
            self.suggest.selected,
            self.suggest.hover,
            width,
        );
    }

    fn paint_popup(&mut self) {
        let (w, h) = self.suggest.size;
        if w == 0 || h == 0 {
            return;
        }
        if let Some(r) = self.suggest.renderer.as_mut()
            && let Err(e) = r.render(&self.suggest.popup.prims, &self.favicons, w, h, self.scale)
        {
            crate::log::error(&format!("öneri listesi çizilemedi: {e}"));
        }
    }

    /// Klavyeyle seçili satırı değiştirir; liste kapalıysa `false`.
    fn move_selection(&mut self, delta: isize) -> bool {
        if !self.suggest.visible.get() || self.suggest.rows.is_empty() {
            return false;
        }
        let last = self.suggest.rows.len() as isize - 1;
        self.suggest.selected = (self.suggest.selected as isize + delta).clamp(0, last) as usize;
        self.layout_popup();
        unsafe { InvalidateRect(self.suggest.hwnd, null(), 0) };
        true
    }

    /// Enter'a basılınca gidilecek yer: seçili öneri; ilk satır seçiliyse
    /// (ya da liste kapalıysa) kutudaki metnin kendisi.
    fn chosen(&self, row: usize) -> Option<String> {
        if !self.suggest.visible.get() || row == 0 {
            return None;
        }
        self.suggest.targets.get(row).cloned()
    }

    fn popup_hit(&self, lparam: LPARAM) -> Option<usize> {
        let (x, y) = point(lparam);
        self.suggest
            .popup
            .hit(x as f32 / self.scale, y as f32 / self.scale)
    }

    /// Site bilgisi panelini kilidin altında açar.
    pub fn show_site(&mut self, info: SiteInfo) {
        let Some(lock) = self
            .layout
            .hits
            .iter()
            .find(|(_, t)| *t == Target::Site)
            .map(|(r, _)| *r)
        else {
            return;
        };
        self.hide_suggestions();
        self.site.info = Some(info);
        self.site.hover = None;
        self.layout_site();
        let s = self.scale;
        let mut origin = POINT {
            x: (lock.x * s).round() as i32,
            y: ((lock.bottom() + 8.0) * s).round() as i32,
        };
        let w = (layout::SITE_W * s).round() as i32;
        let h = (self.site.panel.height * s).round().max(1.0) as i32;
        round_corners(self.site.hwnd, w, h, s);
        self.site.size = (w as u32, h as u32);
        self.site.visible = true;
        SITE_OPEN.set(true);
        unsafe {
            ClientToScreen(self.hwnd, &mut origin);
            SetWindowPos(
                self.site.hwnd,
                null_mut(),
                origin.x,
                origin.y,
                w,
                h,
                SWP_SHOWWINDOW,
            );
            InvalidateRect(self.site.hwnd, null(), 0);
        }
    }

    /// Açık paneldeki bilgileri (izin değişince) yeniler.
    pub fn update_site(&mut self, info: SiteInfo) {
        if !self.site.visible {
            return;
        }
        self.site.info = Some(info);
        self.layout_site();
        let h = (self.site.panel.height * self.scale).round().max(1.0) as i32;
        if h as u32 != self.site.size.1 {
            self.site.size.1 = h as u32;
            round_corners(self.site.hwnd, self.site.size.0 as i32, h, self.scale);
            unsafe {
                SetWindowPos(
                    self.site.hwnd,
                    null_mut(),
                    0,
                    0,
                    self.site.size.0 as i32,
                    h,
                    SWP_NOMOVE | SWP_NOACTIVATE,
                );
            }
        }
        unsafe { InvalidateRect(self.site.hwnd, null(), 0) };
    }

    pub fn site_visible(&self) -> bool {
        self.site.visible
    }

    pub fn hide_site(&mut self) {
        if !self.site.visible {
            return;
        }
        self.site.visible = false;
        self.site.hidden_at = Some(std::time::Instant::now());
        SITE_OPEN.set(false);
        unsafe { ShowWindow(self.site.hwnd, SW_HIDE) };
    }

    /// Panel az önce kilide basılınca mı kapandı?
    fn site_just_closed(&self) -> bool {
        self.site
            .hidden_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(300))
    }

    fn layout_site(&mut self) {
        if let Some(info) = &self.site.info {
            self.site.panel = layout::site_panel(&self.theme, info, self.site.hover);
        }
    }

    fn paint_site(&mut self) {
        let (w, h) = self.site.size;
        if w == 0 || h == 0 {
            return;
        }
        if let Some(r) = self.site.renderer.as_mut()
            && let Err(e) = r.render(&self.site.panel.prims, &self.favicons, w, h, self.scale)
        {
            crate::log::error(&format!("site paneli çizilemedi: {e}"));
        }
    }

    fn site_hit(&self, lparam: LPARAM) -> Option<usize> {
        let (x, y) = point(lparam);
        self.site
            .panel
            .hit(x as f32 / self.scale, y as f32 / self.scale)
    }

    /// İzin sorusunu gösterir (ya da gösterileni değiştirir).
    pub fn show_ask(&mut self, info: AskInfo) {
        if self.ask.info.as_ref() != Some(&info) {
            self.ask.hover = None;
        }
        self.ask.info = Some(info);
        self.ask.visible = true;
        self.layout_ask();
        self.place_ask();
    }

    pub fn hide_ask(&mut self) {
        self.ask.info = None;
        if std::mem::take(&mut self.ask.visible) {
            unsafe { ShowWindow(self.ask.hwnd, SW_HIDE) };
        }
    }

    fn layout_ask(&mut self) {
        if let Some(info) = &self.ask.info {
            self.ask.panel = layout::ask_panel(&self.theme, info, self.ask.hover);
        }
    }

    /// Soruyu kilidin (yoksa adres kutusunun) altına yerleştirir. Pencere
    /// simge durumundayken gizlenir, geri gelince yeniden çıkar.
    fn place_ask(&mut self) {
        if !self.ask.visible {
            return;
        }
        if unsafe { IsIconic(self.hwnd) } != 0 {
            unsafe { ShowWindow(self.ask.hwnd, SW_HIDE) };
            return;
        }
        let anchor = self
            .layout
            .hits
            .iter()
            .find(|(_, t)| *t == Target::Site)
            .map_or(self.layout.address, |(r, _)| *r);
        let s = self.scale;
        let mut origin = POINT {
            x: (anchor.x * s).round() as i32,
            y: ((anchor.bottom() + 8.0) * s).round() as i32,
        };
        let w = (layout::ASK_W * s).round() as i32;
        let h = (self.ask.panel.height * s).round().max(1.0) as i32;
        if self.ask.size != (w as u32, h as u32) {
            round_corners(self.ask.hwnd, w, h, s);
        }
        self.ask.size = (w as u32, h as u32);
        unsafe {
            ClientToScreen(self.hwnd, &mut origin);
            SetWindowPos(
                self.ask.hwnd,
                null_mut(),
                origin.x,
                origin.y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(self.ask.hwnd, null(), 0);
        }
    }

    fn paint_ask(&mut self) {
        let (w, h) = self.ask.size;
        if w == 0 || h == 0 {
            return;
        }
        if let Some(r) = self.ask.renderer.as_mut()
            && let Err(e) = r.render(&self.ask.panel.prims, &self.favicons, w, h, self.scale)
        {
            crate::log::error(&format!("izin sorusu çizilemedi: {e}"));
        }
    }

    fn ask_hit(&self, lparam: LPARAM) -> Option<AskTarget> {
        let (x, y) = point(lparam);
        self.ask
            .panel
            .hit(x as f32 / self.scale, y as f32 / self.scale)
    }

    fn relayout(&mut self) {
        let now = std::time::Instant::now();
        self.model.tab_shift = self
            .slides
            .iter()
            .map(|s| {
                let (dx, dy) = s.offset(now);
                (s.index, dx, dy)
            })
            .collect();
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
        self.place_find();
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
            SendMessageW(self.find.edit, WM_SETFONT, font as WPARAM, 1);
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

    /// Sürükleme sırasında fare hareketi: gerekiyorsa sekmeyi yer
    /// değiştirmek için olay döndürür.
    fn drag_move(&mut self, lparam: LPARAM) -> Option<UserEvent> {
        let (x, y) = point(lparam);
        let (x, y) = (x as f32 / self.scale, y as f32 / self.scale);
        let horizontal = matches!(
            self.appearance.layout,
            crate::ui::theme::LayoutKind::Classic | crate::ui::theme::LayoutKind::SingleRow
        );
        let drag = self.drag.as_mut()?;
        if !drag.moving {
            let (dx, dy) = (x - drag.start.0, y - drag.start.1);
            if dx.hypot(dy) < DRAG_THRESHOLD {
                return None;
            }
            drag.moving = true;
        }
        let mut event = None;
        let mut moved = None;
        if !drag.waiting
            && let Some(to) = layout::drag_target(&self.layout, drag.index, x, y, horizontal)
        {
            event = Some(UserEvent::MoveTab(drag.index, to));
            moved = Some((drag.index, to));
            drag.index = to;
            drag.waiting = true;
        }
        let (index, grab) = (drag.index, drag.grab);
        if let Some((from, to)) = moved {
            self.pending_move = Some((from, to, layout::displaced(&self.layout, from, to)));
        }
        // Sekmenin çizimi, durduğu yere göre imleç kadar kayar (yalnızca
        // sekmelerin dizildiği yönde).
        let (dx, dy) = tab_rect(&self.layout, index)
            .map_or((0.0, 0.0), |r| (x - grab.0 - r.x, y - grab.1 - r.y));
        let pointer = Pointer {
            pressed: Some(Target::Tab(index)),
            hover: Some(Target::Tab(index)),
            drag: Some(if horizontal {
                (index, dx, 0.0)
            } else {
                (index, 0.0, dy)
            }),
        };
        self.set_pointer(pointer);
        event
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
        Target::Site => UserEvent::SiteInfo,
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

/// Sekmenin bulunduğu yer (tıklama alanı).
fn tab_rect(layout: &Layout, index: usize) -> Option<crate::ui::scene::Rect> {
    layout
        .hits
        .iter()
        .find(|(_, t)| *t == Target::Tab(index))
        .map(|(r, _)| *r)
}

/// Öneri listesi adres kutusundan bu kadar (DIP) taşar.
const POPUP_INSET: f32 = 8.0;

/// Sayfada bul kutusu: ana pencerenin, sayfanın üstünde duran alt penceresi
/// ve içindeki yazı kutusu.
fn create_find(parent: HWND) -> (HWND, HWND, HWND) {
    unsafe {
        let class = wide("JulieFind");
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            parent,
            null_mut(),
            instance,
            null(),
        );
        let edit_class = wide("EDIT");
        let edit = CreateWindowExW(
            0,
            edit_class.as_ptr(),
            null(),
            WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
            0,
            0,
            0,
            0,
            hwnd,
            null_mut(),
            instance,
            null(),
        );
        let cue = wide("Sayfada bul");
        SendMessageW(edit, EM_SETCUEBANNER, 1, cue.as_ptr() as LPARAM);
        (hwnd, edit, create_find_tips(hwnd))
    }
}

/// Bulma kutusunun düğmeleri için ipucu penceresi. Yerleri kutu
/// yerleştirilince `place_find_tips` ile verilir.
fn create_find_tips(find: HWND) -> HWND {
    unsafe {
        let tooltip = CreateWindowExW(
            windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            null(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
            0,
            0,
            0,
            0,
            find,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        for (id, target) in FindTarget::ALL.iter().enumerate() {
            // İpucu penceresi metni kendine kopyalar.
            let mut text = wide(target.tip());
            let info = TTTOOLINFOW {
                cbSize: size_of::<TTTOOLINFOW>() as u32,
                uFlags: TTF_SUBCLASS,
                hwnd: find,
                uId: id,
                lpszText: text.as_mut_ptr(),
                ..Default::default()
            };
            SendMessageW(tooltip, TTM_ADDTOOLW, 0, &info as *const _ as LPARAM);
        }
        tooltip
    }
}

/// Öneri listesinin penceresi: odak almaz, sahibinin üstünde durur,
/// görev çubuğunda görünmez; Windows 11'de köşeleri yuvarlak, gölgeli.
fn create_popup(owner: HWND) -> HWND {
    unsafe {
        let class = wide("JulieSuggestions");
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            owner,
            null_mut(),
            instance,
            null(),
        );
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner).cast(),
            4,
        );
        hwnd
    }
}

/// Site bilgisi panelinin penceresi: öneri listesinden farklı olarak odak
/// alır; böylece dışarı tıklanınca (`WM_ACTIVATE`) ya da Esc ile kapanır.
fn create_site(owner: HWND) -> HWND {
    unsafe {
        let class = wide("JulieSite");
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            owner,
            null_mut(),
            instance,
            null(),
        );
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner).cast(),
            4,
        );
        hwnd
    }
}

/// Pencereyi köşeleri yuvarlak kesilmiş yapar (Windows 10'da da çalışır;
/// Windows 11'in kendi yuvarlatması yalnızca bazı pencerelerde var).
fn round_corners(hwnd: HWND, w: i32, h: i32, scale: f32) {
    let d = (layout::PANEL_RADIUS * 2.0 * scale).round() as i32;
    unsafe { SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d), 1) };
}

/// EDIT kutusundaki metin.
fn edit_text(edit: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(edit);
        let mut buf = vec![0u16; len as usize + 1];
        let read = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..read.max(0) as usize])
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
        // Site paneli açıkken pencere başlığı etkin görünmeye devam etsin
        // (panel, pencerenin bir parçası gibi).
        if msg == WM_NCACTIVATE && wparam == 0 && SITE_OPEN.get() {
            return DefSubclassProc(hwnd, msg, 1, lparam);
        }
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
            WM_SETCURSOR if (lparam & 0xffff) as u32 == HTCLIENT => {
                let mut p = POINT { x: 0, y: 0 };
                GetCursorPos(&mut p);
                ScreenToClient(hwnd, &mut p);
                if c.splitter_drag || c.on_splitter((p.x, p.y)) {
                    SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
                    return 1;
                }
            }
            WM_MOUSEMOVE if c.splitter_drag => {
                drop(c);
                let _ = s
                    .proxy
                    .send_event(UserEvent::DevToolsResize(point(lparam).0, false));
                return 0;
            }
            WM_LBUTTONDOWN if c.on_splitter(point(lparam)) => {
                c.hide_suggestions();
                c.splitter_drag = true;
                SetCapture(hwnd);
                return 0;
            }
            WM_LBUTTONUP if c.splitter_drag => {
                c.splitter_drag = false;
                drop(c);
                ReleaseCapture();
                let _ = s
                    .proxy
                    .send_event(UserEvent::DevToolsResize(point(lparam).0, true));
                return 0;
            }
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
                if c.drag.is_some() {
                    let event = c.drag_move(lparam);
                    if c.drag.as_ref().is_some_and(|d| d.moving) {
                        drop(c);
                        if let Some(event) = event {
                            let _ = s.proxy.send_event(event);
                        }
                        return 0;
                    }
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
                c.hide_suggestions();
                let target = c.hit(lparam);
                if target.is_some() {
                    SetCapture(hwnd);
                }
                c.drag = match target {
                    Some(Target::Tab(index)) => {
                        let (x, y) = point(lparam);
                        let (x, y) = (x as f32 / c.scale, y as f32 / c.scale);
                        let grab =
                            tab_rect(&c.layout, index).map_or((0.0, 0.0), |r| (x - r.x, y - r.y));
                        Some(TabDrag {
                            index,
                            start: (x, y),
                            grab,
                            moving: false,
                            waiting: false,
                        })
                    }
                    _ => None,
                };
                let pointer = Pointer {
                    pressed: target,
                    ..c.pointer
                };
                c.set_pointer(pointer);
                return 0;
            }
            WM_LBUTTONUP if c.drag.as_ref().is_some_and(|d| d.moving) => {
                // Sürükleme bitti: bırakılan sekme öne gelir.
                let index = c.drag.take().map_or(0, |d| d.index);
                // Bırakılan sekme imlecin olduğu yerden yuvasına kayar.
                if let Some((i, dx, dy)) = c.pointer.drag {
                    c.slides.retain(|s| s.index != i);
                    c.slides.push(Slide {
                        index: i,
                        dx,
                        dy,
                        started: std::time::Instant::now(),
                    });
                    c.start_slides();
                }
                let pointer = Pointer {
                    pressed: None,
                    hover: c.hit(lparam),
                    drag: None,
                };
                c.set_pointer(pointer);
                drop(c);
                ReleaseCapture();
                let _ = s.proxy.send_event(UserEvent::SelectTab(index));
                return 0;
            }
            WM_LBUTTONUP => {
                c.drag = None;
                let pressed = c.pointer.pressed;
                let target = c.hit(lparam);
                let pointer = Pointer {
                    pressed: None,
                    hover: target,
                    drag: None,
                };
                c.set_pointer(pointer);
                // Açık paneli kilide tıklayarak kapatmak: panel basışta
                // kapandı, bırakınca yeniden açılmasın.
                let toggled_off = target == Some(Target::Site) && c.site_just_closed();
                drop(c);
                ReleaseCapture();
                if let (Some(t), Some(p)) = (target, pressed)
                    && t == p
                    && !toggled_off
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
            WM_TIMER if wparam == SLIDE_TIMER => {
                c.tick_slides();
                return 0;
            }
            // Kullanıcı adres kutusuna yazdı: önerileri güncelle. Programın
            // kendisi metni değiştirirken arayüz meşgul olduğundan buraya
            // hiç gelinmez; yine de yalnızca kutu odaktayken dinle.
            WM_COMMAND
                if (wparam >> 16) as u32 == EN_CHANGE
                    && lparam as HWND == c.edit
                    && GetFocus() == c.edit =>
            {
                let text = edit_text(c.edit);
                // Silerken tamamlanırsa silinen harf geri gelir; yalnızca
                // imleç sondayken ve yazarken tamamla.
                let (mut start, mut end) = (0u32, 0u32);
                SendMessageW(
                    c.edit,
                    EM_GETSEL,
                    (&raw mut start) as WPARAM,
                    (&raw mut end) as LPARAM,
                );
                let at_end = start == end && end as usize == text.encode_utf16().count();
                let complete = at_end && !DELETING.get();
                let _ = s.proxy.send_event(UserEvent::AddressInput(text, complete));
            }
            // Pencere taşınınca, boyutu değişince ya da başka uygulamaya
            // geçilince liste yerinde kalmasın.
            WM_ACTIVATE if wparam & 0xffff == 0 => c.hide_suggestions(),
            WM_MOVE | WM_SIZE => {
                c.hide_suggestions();
                c.hide_site();
                c.place_ask();
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
    /// Adres kutusunda son basılan tuş silme tuşu muydu (Backspace/Delete)?
    static DELETING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Site paneli açık mı (ana pencerenin başlığı etkin kalsın diye)?
    static SITE_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
        let s = shared(refdata);
        let proxy = &s.proxy;
        if msg == WM_KEYDOWN {
            DELETING.set(wparam == VK_BACK as WPARAM || wparam == VK_DELETE as WPARAM);
        }
        match msg {
            WM_KEYDOWN if wparam == VK_RETURN as WPARAM => {
                let chosen = s.chrome.try_borrow().ok().and_then(|c| {
                    let chosen = c.chosen(c.suggest.selected);
                    c.hide_suggestions();
                    chosen
                });
                let text = chosen.unwrap_or_else(|| edit_text(hwnd));
                let _ = proxy.send_event(UserEvent::Navigate(text));
                return 0;
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as WPARAM => {
                // Önce açık listeyi kapat; ikinci Esc sayfaya döner.
                let closed = s.chrome.try_borrow().is_ok_and(|c| {
                    c.suggest.visible.get() && {
                        c.hide_suggestions();
                        true
                    }
                });
                if !closed {
                    let _ = proxy.send_event(UserEvent::FocusPage);
                }
                return 0;
            }
            WM_KEYDOWN if wparam == VK_DOWN as WPARAM || wparam == VK_UP as WPARAM => {
                let delta = if wparam == VK_DOWN as WPARAM { 1 } else { -1 };
                if s.chrome
                    .try_borrow_mut()
                    .is_ok_and(|mut c| c.move_selection(delta))
                {
                    return 0;
                }
            }
            WM_KILLFOCUS => {
                if let Ok(c) = s.chrome.try_borrow() {
                    c.hide_suggestions();
                }
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

/// Öneri listesi: fareyle satır seçimi. Odak almaz; adres kutusu odakta kalır.
unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
            WM_ERASEBKGND => return 1,
            _ => {}
        }
        let s = shared(refdata);
        let Ok(mut c) = s.chrome.try_borrow_mut() else {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_PAINT => {
                c.paint_popup();
                ValidateRect(hwnd, null());
                return 0;
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
                let hover = c.popup_hit(lparam);
                if hover != c.suggest.hover {
                    c.suggest.hover = hover;
                    c.layout_popup();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_MOUSELEAVE => {
                if c.suggest.hover.take().is_some() {
                    c.layout_popup();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_LBUTTONDOWN => {
                if let Some(row) = c.popup_hit(lparam) {
                    let text = c.chosen(row).unwrap_or_else(|| edit_text(c.edit));
                    c.hide_suggestions();
                    drop(c);
                    let _ = s.proxy.send_event(UserEvent::Navigate(text));
                }
                return 0;
            }
            _ => {}
        }
        drop(c);
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// Site bilgisi paneli: izin satırlarına tıklama, Esc ve dışarı tıklayınca
/// kapanma.
unsafe extern "system" fn site_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_ERASEBKGND {
            return 1;
        }
        let s = shared(refdata);
        let Ok(mut c) = s.chrome.try_borrow_mut() else {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_PAINT => {
                c.paint_site();
                ValidateRect(hwnd, null());
                return 0;
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
                let hover = c.site_hit(lparam);
                if hover != c.site.hover {
                    c.site.hover = hover;
                    c.layout_site();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_MOUSELEAVE => {
                if c.site.hover.take().is_some() {
                    c.layout_site();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_LBUTTONDOWN => {
                if let Some(row) = c.site_hit(lparam) {
                    drop(c);
                    let _ = s.proxy.send_event(UserEvent::SitePermission(row));
                }
                return 0;
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as WPARAM => {
                c.hide_site();
                drop(c);
                let _ = s.proxy.send_event(UserEvent::FocusPage);
                return 0;
            }
            WM_ACTIVATE if wparam & 0xffff == 0 => {
                let main = c.hwnd;
                c.hide_site();
                drop(c);
                // Başka bir uygulamaya geçildi: ana pencerenin başlığı da
                // artık etkin görünmesin.
                if lparam as HWND != main {
                    SendMessageW(main, WM_NCACTIVATE, 0, 0);
                }
                return 0;
            }
            _ => {}
        }
        drop(c);
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// İzin sorusu: düğmeler. Odak almaz.
unsafe extern "system" fn ask_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
            WM_ERASEBKGND => return 1,
            _ => {}
        }
        let s = shared(refdata);
        let Ok(mut c) = s.chrome.try_borrow_mut() else {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_PAINT => {
                c.paint_ask();
                ValidateRect(hwnd, null());
                return 0;
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
                let hover = c.ask_hit(lparam);
                if hover != c.ask.hover {
                    c.ask.hover = hover;
                    c.layout_ask();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_MOUSELEAVE => {
                if c.ask.hover.take().is_some() {
                    c.layout_ask();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_LBUTTONDOWN => {
                if let Some(target) = c.ask_hit(lparam) {
                    drop(c);
                    let _ = s.proxy.send_event(UserEvent::AskAnswer(target));
                }
                return 0;
            }
            _ => {}
        }
        drop(c);
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// Sayfada bul kutusu: çizim, düğmeler ve yazı kutusunun renkleri.
unsafe extern "system" fn find_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_ERASEBKGND {
            return 1;
        }
        let s = shared(refdata);
        let Ok(mut c) = s.chrome.try_borrow_mut() else {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_PAINT => {
                c.paint_find();
                ValidateRect(hwnd, null());
                return 0;
            }
            WM_CTLCOLOREDIT => {
                let hdc = wparam as HDC;
                SetTextColor(hdc, c.theme.palette.text.colorref());
                SetBkColor(hdc, c.theme.palette.surface.colorref());
                return c.edit_brush as LRESULT;
            }
            WM_COMMAND if (wparam >> 16) as u32 == EN_CHANGE && lparam as HWND == c.find.edit => {
                let text = edit_text(c.find.edit);
                c.find.state.has_text = !text.is_empty();
                c.find.state.active = 0;
                c.find.state.total = 0;
                c.redraw_find();
                drop(c);
                let _ = s.proxy.send_event(UserEvent::FindText(text));
                return 0;
            }
            WM_MOUSEMOVE => {
                if !c.find.tracking_leave {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    c.find.tracking_leave = true;
                }
                let hover = c.find_hit(lparam);
                if hover != c.find.state.hover {
                    c.find.state.hover = hover;
                    c.redraw_find();
                }
            }
            WM_MOUSELEAVE => {
                c.find.tracking_leave = false;
                if c.find.state.hover.take().is_some() {
                    c.redraw_find();
                }
            }
            WM_LBUTTONDOWN => {
                let target = c.find_hit(lparam);
                if target.is_some() {
                    SetCapture(hwnd);
                }
                c.find.state.pressed = target;
                c.redraw_find();
                return 0;
            }
            WM_LBUTTONUP => {
                let pressed = c.find.state.pressed.take();
                let target = c.find_hit(lparam);
                ReleaseCapture();
                let event = match (target, pressed) {
                    (Some(t), Some(p)) if t == p => match t {
                        FindTarget::Prev => Some(UserEvent::FindPrev),
                        FindTarget::Next => Some(UserEvent::FindNext),
                        FindTarget::Close => Some(UserEvent::FindClose),
                        FindTarget::Case => {
                            c.find.state.case_sensitive = !c.find.state.case_sensitive;
                            Some(UserEvent::FindText(edit_text(c.find.edit)))
                        }
                    },
                    _ => None,
                };
                c.redraw_find();
                let edit = c.find.edit;
                drop(c);
                if !matches!(event, Some(UserEvent::FindClose)) {
                    SetFocus(edit);
                }
                if let Some(event) = event {
                    let _ = s.proxy.send_event(event);
                }
                return 0;
            }
            _ => {}
        }
        drop(c);
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// Bulma kutusunun yazı alanı: Enter sonraki, Shift+Enter önceki, Esc kapatır.
unsafe extern "system" fn find_edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        let proxy = &shared(refdata).proxy;
        let down = |vk: u16| GetKeyState(vk.into()) < 0;
        match msg {
            WM_KEYDOWN if wparam == VK_RETURN as WPARAM => {
                let event = if down(VK_SHIFT) {
                    UserEvent::FindPrev
                } else {
                    UserEvent::FindNext
                };
                let _ = proxy.send_event(event);
                return 0;
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as WPARAM => {
                let _ = proxy.send_event(UserEvent::FindClose);
                return 0;
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                SWALLOW_CHAR.set(false);
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
            WM_CHAR if wparam == 0x0D || wparam == 0x1B => return 0,
            WM_CHAR | WM_SYSCHAR if SWALLOW_CHAR.replace(false) => return 0,
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}
