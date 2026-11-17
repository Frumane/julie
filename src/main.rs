// Sürüm derlemesinde arka planda konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod address;
#[cfg_attr(not(windows), allow(dead_code))]
mod blocking;
#[cfg(windows)]
mod browser;
#[cfg_attr(not(windows), allow(dead_code))]
mod cosmetic;
#[cfg_attr(not(windows), allow(dead_code))]
mod log;
#[cfg(windows)]
mod request_filter;
#[cfg_attr(not(windows), allow(dead_code))]
mod settings;
#[cfg_attr(not(windows), allow(dead_code))]
mod shortcuts;
#[cfg_attr(not(windows), allow(dead_code))]
mod sleep;
#[cfg_attr(not(windows), allow(dead_code))]
mod start_page;
#[cfg(windows)]
mod toolbar;

/// Arayüzden ve web görünümlerinden olay döngüsüne gelen olaylar.
#[derive(Debug, Clone)]
pub enum UserEvent {
    Back,
    Forward,
    Reload,
    /// Adres çubuğundan gelen metin (URL ya da arama).
    Navigate(String),
    NewTab,
    CloseTab,
    /// Son kapatılan sekmeyi yeniden aç.
    ReopenTab,
    SelectTab(usize),
    NextTab,
    PrevTab,
    LastTab,
    FocusAddress,
    FocusPage,
    /// Bir sayfanın `window.open` / `target=_blank` isteği.
    OpenInNewTab(String),
    /// Başlangıç sayfasından (sekme kimliği ile) gelen mesaj.
    StartPage(u64, start_page::Message),
    TitleChanged(u64, String),
    /// Sekmede engellenen istek sayısı değişti.
    BlockedCountChanged(u64),
    /// Engelleyiciyi geçerli sitede aç/kapat.
    ToggleBlocking,
    /// Arka planda indirilip derlenen filtre kuralları hazır.
    FiltersReady(Vec<u8>),
    /// Filtre listeleri indirilemedi ya da derlenemedi.
    FiltersFailed(String),
    /// Periyodik kontrol: arka plandaki sekmeleri uyutma zamanı geldi mi?
    Tick,
    /// Sayfadaki kozmetik filtre betiğinden gelen mesaj (sekme, sayfa adresi).
    Cosmetic(u64, String, cosmetic::Message),
    UrlChanged(u64, String),
}

#[cfg(windows)]
fn main() {
    browser::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Julie şimdilik yalnızca Windows'ta çalışıyor (WebView2).");
    std::process::exit(1);
}
