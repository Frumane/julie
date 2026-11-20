//! Geliştirici araçları (F12) penceresi. WebView2 bu pencereyi kendisi
//! açar ve Edge'inki gibi görünür ("DevTools - ..."). Pencere açılınca
//! bulunur; başlığı ve simgesi Julie'ninki olur, Julie'nin yanına
//! yerleştirilir. Pencere WebView2'nin işleminde olduğundan değişiklikler
//! dışarıdan yapılır ve motor başlığı değiştirdikçe yeniden uygulanır.
//!
//! İleride (yol haritası): araçları Julie'nin penceresinin içine gömmek.

#[cfg(windows)]
use std::collections::HashSet;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetSystemMetrics, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsWindow, IsWindowVisible,
    LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SMTO_ABORTIFHUNG, SWP_NOACTIVATE,
    SWP_NOZORDER, SendMessageTimeoutW, SetWindowPos, SetWindowTextW, WM_SETICON,
};

/// Yeni sekmenin değil, Julie'nin pencere başlığı öneki.
const TITLE: &str = "Julie · Geliştirici araçları";
/// Motorun verdiği başlığın öneki.
const ENGINE_PREFIX: &str = "DevTools - ";
/// Yan yana sığmazsa araçların kaplayacağı genişlik oranı.
const SHARE: f32 = 0.4;

#[cfg(windows)]
/// Zaten sahiplenilmiş araç pencereleri (birden çok sekmede F12).
static CLAIMED: Mutex<Option<HashSet<isize>>> = Mutex::new(None);

#[cfg(windows)]
/// Motorun araç penceresini açmasından sonra çağrılır: pencereyi arayan
/// ve kapanana kadar Julie'nin gibi tutan bir iş parçacığı başlatır.
/// `browser_pid`: WebView2 tarayıcı işlemi; `julie`: ana pencere.
pub fn adopt(browser_pid: u32, julie: isize) {
    let _ = std::thread::Builder::new()
        .name("devtools".into())
        .spawn(move || watch(browser_pid, julie));
}

#[cfg(windows)]
fn watch(pid: u32, julie: isize) {
    // Pencere birkaç yüz milisaniyede açılır; gelmezse (zaten açıktı ve
    // yalnızca öne geldi) vazgeç.
    let started = Instant::now();
    let hwnd = loop {
        if let Some(hwnd) = find_new(pid) {
            break hwnd;
        }
        if started.elapsed() > Duration::from_secs(5) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    place(hwnd, julie);
    let icons = load_icons();
    loop {
        if unsafe { IsWindow(hwnd as HWND) } == 0 {
            break;
        }
        brand(hwnd, icons);
        std::thread::sleep(Duration::from_millis(250));
    }
    if let Ok(mut claimed) = CLAIMED.lock() {
        claimed.get_or_insert_with(HashSet::new).remove(&hwnd);
    }
}

#[cfg(windows)]
/// Tarayıcı işleminin henüz sahiplenilmemiş, görünür araç penceresi.
fn find_new(pid: u32) -> Option<isize> {
    struct Search {
        pid: u32,
        found: Vec<isize>,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
        let search = unsafe { &mut *(data as *mut Search) };
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        if owner == search.pid
            && unsafe { IsWindowVisible(hwnd) } != 0
            && class_of(hwnd) == "Chrome_WidgetWin_1"
            && title_of(hwnd).starts_with(ENGINE_PREFIX)
        {
            search.found.push(hwnd as isize);
        }
        1
    }
    let mut search = Search {
        pid,
        found: Vec::new(),
    };
    unsafe { EnumWindows(Some(each), &mut search as *mut Search as LPARAM) };
    let mut claimed = CLAIMED.lock().ok()?;
    let claimed = claimed.get_or_insert_with(HashSet::new);
    let hwnd = search.found.into_iter().find(|h| !claimed.contains(h))?;
    claimed.insert(hwnd);
    Some(hwnd)
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
pub fn branded_title(engine: &str) -> Option<String> {
    let page = engine.strip_prefix(ENGINE_PREFIX)?.trim();
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
