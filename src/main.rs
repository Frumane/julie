// Sürüm derlemesinde arka planda konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod address;
#[cfg(windows)]
mod browser;
#[cfg_attr(not(windows), allow(dead_code))]
mod settings;
#[cfg_attr(not(windows), allow(dead_code))]
mod shortcuts;
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
