//! Geliştirici araçları (F12) penceresi. WebView2 bu pencereyi kendisi
//! açar ve Edge'inki gibi görünür ("DevTools - ..."). Windows'un pencere
//! bildirimleri (açıldı, başlığı değişti) dinlenir: araç penceresi
//! açılınca Julie'nin yanına yerleştirilir; başlığı ve simgesi Julie'ninki
//! olur, motor başlığı değiştirdikçe yeniden. F12, sağ tık "İncele" ve
//! diğer bütün yollarla açılan pencereler için geçerli; bekleme döngüsü yok.
//!
//! İleride (yol haritası): araçları Julie'nin penceresinin içine gömmek.

#[cfg(windows)]
use std::collections::HashSet;
#[cfg(windows)]
use std::sync::Mutex;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, GA_ROOT, GetAncestor, GetClassNameW,
    GetSystemMetrics, GetWindowRect, GetWindowTextW, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsWindow,
    LR_DEFAULTCOLOR, LoadImageW, OBJID_WINDOW, SM_CXICON, SM_CXSMICON, SMTO_ABORTIFHUNG,
    SWP_NOACTIVATE, SWP_NOZORDER, SendMessageTimeoutW, SetWindowPos, SetWindowTextW,
    WINEVENT_OUTOFCONTEXT, WM_SETICON,
};

/// Araç penceresinin başlığı.
const TITLE: &str = "Julie · Geliştirici araçları";
/// Motorun verdiği başlığın öneki.
const ENGINE_PREFIX: &str = "DevTools - ";
/// Yan yana sığmazsa araçların kaplayacağı genişlik oranı.
const SHARE: f32 = 0.4;

#[cfg(windows)]
struct Watch {
    /// İzlenen WebView2 tarayıcı işlemi.
    pid: u32,
    /// Julie'nin ana penceresi.
    julie: isize,
    /// Julie'nin simgesi (büyük, küçük).
    icons: (isize, isize),
    /// Yerleştirilmiş araç pencereleri (bir kez yerleştirilir).
    placed: HashSet<isize>,
}

#[cfg(windows)]
static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

#[cfg(windows)]
/// WebView2 tarayıcı işleminin araç pencerelerini izlemeye başlar. Ana
/// iş parçacığından çağrılmalı (bildirimler onun mesaj döngüsüne gelir).
/// Aynı işlem için ikinci çağrı bir şey yapmaz.
pub fn watch(browser_pid: u32, julie: isize) {
    let Ok(mut watch) = WATCH.lock() else {
        return;
    };
    if watch.as_ref().is_some_and(|w| w.pid == browser_pid) {
        return;
    }
    for event in [EVENT_OBJECT_SHOW, EVENT_OBJECT_NAMECHANGE] {
        unsafe {
            SetWinEventHook(
                event,
                event,
                std::ptr::null_mut(),
                Some(on_event),
                browser_pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
    }
    *watch = Some(Watch {
        pid: browser_pid,
        julie,
        icons: load_icons(),
        placed: HashSet::new(),
    });
}

#[cfg(windows)]
unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != OBJID_WINDOW || child != 0 || hwnd.is_null() {
        return;
    }
    // Yalnızca en üstteki pencereler; motorun başlığını taşıyanlar.
    if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd
        || class_of(hwnd) != "Chrome_WidgetWin_1"
        || !title_of(hwnd).starts_with(ENGINE_PREFIX)
    {
        return;
    }
    // Bildirim, başka bir pencereye mesaj gönderirken de gelebilir; kilit
    // o sırada tutuluyorsa bekleme (kilitlenir), bu bildirimi atla.
    let Ok(mut watch) = WATCH.try_lock() else {
        return;
    };
    let Some(watch) = watch.as_mut() else {
        return;
    };
    let key = hwnd as isize;
    if event == EVENT_OBJECT_SHOW && !watch.placed.contains(&key) {
        // Kapanmış pencerelerin kayıtlarını at.
        watch
            .placed
            .retain(|h| unsafe { IsWindow(*h as HWND) } != 0);
        watch.placed.insert(key);
        place(key, watch.julie);
    }
    brand(key, watch.icons);
}

#[cfg(windows)]
fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

#[cfg(windows)]
fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Motorun başlığından ("DevTools - site.com/yol") Julie'nin başlığı.
/// Julie'nin kendi sayfaları `julie://ad` olarak yazılır.
pub fn branded_title(engine: &str) -> Option<String> {
    let page = engine.strip_prefix(ENGINE_PREFIX)?.trim();
    let page = match page.strip_prefix("julie.") {
        Some(rest) => {
            let name = rest.split(['/', '?', '#']).next().unwrap_or_default();
            format!("julie://{name}")
        }
        None => page.to_string(),
    };
    Some(if page.is_empty() {
        TITLE.to_string()
    } else {
        format!("{TITLE} — {page}")
    })
}

#[cfg(windows)]
/// Başlığı ve simgeyi Julie'ninki yapar (motor değiştirdiyse yeniden).
fn brand(hwnd: isize, icons: (isize, isize)) {
    let hwnd = hwnd as HWND;
    let Some(title) = branded_title(&title_of(hwnd)) else {
        return;
    };
    let wide: Vec<u16> = title.encode_utf16().chain([0]).collect();
    unsafe { SetWindowTextW(hwnd, wide.as_ptr()) };
    for (kind, icon) in [(ICON_BIG, icons.0), (ICON_SMALL, icons.1)] {
        if icon != 0 {
            let mut out = 0usize;
            unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    WM_SETICON,
                    kind as WPARAM,
                    icon as LPARAM,
                    SMTO_ABORTIFHUNG,
                    200,
                    &mut out,
                )
            };
        }
    }
}

#[cfg(windows)]
/// Julie'nin simgesi (büyük, küçük).
fn load_icons() -> (isize, isize) {
    let load = |size: i32| unsafe {
        LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            1 as _,
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        ) as isize
    };
    unsafe {
        (
            load(GetSystemMetrics(SM_CXICON)),
            load(GetSystemMetrics(SM_CXSMICON)),
        )
    }
}

#[cfg(windows)]
/// Araçları Julie'nin yanına koyar: sağda yer varsa yanına, yoksa
/// ekranın sağ tarafına (Julie'nin üstüne).
fn place(hwnd: isize, julie: isize) {
    let julie = julie as HWND;
    let mut window = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: window,
        rcWork: window,
        dwFlags: 0,
    };
    unsafe {
        if GetWindowRect(julie, &mut window) == 0
            || GetMonitorInfoW(
                MonitorFromWindow(julie, MONITOR_DEFAULTTONEAREST),
                &mut info,
            ) == 0
        {
            return;
        }
    }
    let (x, y, w, h) = placement(
        (window.left, window.top, window.right, window.bottom),
        (
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        ),
    );
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

/// Araç penceresinin yeri (x, y, genişlik, yükseklik). `julie` ve `work`
/// (sol, üst, sağ, alt): Julie'nin penceresi ve ekranın kullanılabilir alanı.
pub fn placement(julie: (i32, i32, i32, i32), work: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    const MIN: i32 = 480;
    let top = julie.1.max(work.1);
    let height = (julie.3.min(work.3) - top).max(400);
    let room = work.2 - julie.2;
    if room >= MIN {
        return (julie.2, top, room.min(760), height);
    }
    let width = (((work.2 - work.0) as f32 * SHARE) as i32).max(MIN);
    (work.2 - width, top, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            branded_title("DevTools - www.ornek.com/yol").as_deref(),
            Some("Julie · Geliştirici araçları — www.ornek.com/yol")
        );
        assert_eq!(branded_title("DevTools - ").as_deref(), Some(TITLE));
        assert_eq!(
            branded_title("DevTools - julie.error/?kind=cert&code=X").as_deref(),
            Some("Julie · Geliştirici araçları — julie://error")
        );
        // Zaten değiştirilmiş başlığa dokunulmaz.
        assert_eq!(branded_title(&format!("{TITLE} — a.com")), None);
    }

    #[test]
    fn placement_beside_or_over() {
        let work = (0, 0, 1920, 1040);
        // Sağda yer var: yanına.
        assert_eq!(placement((0, 0, 1000, 1040), work), (1000, 0, 760, 1040));
        // Ekranı kaplıyor: sağ %40.
        assert_eq!(placement((0, 0, 1920, 1040), work), (1152, 0, 768, 1040));
    }
}
