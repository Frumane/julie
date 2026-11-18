// Sürüm derlemesinde arka planda konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod address;
#[cfg_attr(not(windows), allow(dead_code))]
mod blocking;
#[cfg(windows)]
mod browser;
#[cfg(windows)]
mod chrome;
#[cfg_attr(not(windows), allow(dead_code))]
mod cosmetic;
#[cfg_attr(not(windows), allow(dead_code))]
mod filter_lists;
#[cfg_attr(not(windows), allow(dead_code))]
mod history_page;
#[cfg_attr(not(windows), allow(dead_code))]
mod log;
#[cfg_attr(not(windows), allow(dead_code))]
mod places;
#[cfg(windows)]
mod request_filter;
#[cfg(windows)]
mod scriptlet_injector;
#[cfg(windows)]
mod search_suggest;
#[cfg_attr(not(windows), allow(dead_code))]
mod session;
#[cfg_attr(not(windows), allow(dead_code))]
mod settings;
#[cfg_attr(not(windows), allow(dead_code))]
mod settings_page;
#[cfg_attr(not(windows), allow(dead_code))]
mod shortcuts;
#[cfg_attr(not(windows), allow(dead_code))]
mod sleep;
#[cfg_attr(not(windows), allow(dead_code))]
mod start_page;
#[cfg_attr(not(windows), allow(dead_code))]
mod suggest;

#[cfg_attr(not(windows), allow(dead_code))]
mod ui;

/// Arayüzden ve web görünümlerinden olay döngüsüne gelen olaylar.
#[derive(Debug, Clone)]
pub enum UserEvent {
    Back,
    Forward,
    Reload,
    /// Adres çubuğundan gelen metin (URL ya da arama).
    Navigate(String),
    /// Adres çubuğuna yazılıyor: önerileri güncelle. İkinci alan: imleç
    /// sonda ve son tuş silme değil, yani otomatik tamamlanabilir.
    AddressInput(String, bool),
    /// Arama motorunun önerileri geldi: motor, sorulan metin, öneriler.
    SearchSuggestions(settings::Engine, String, Vec<String>),
    NewTab,
    /// Yeni gizli pencere aç (Ctrl+Shift+N).
    NewPrivateWindow,
    /// Sayfada bul kutusunu aç (Ctrl+F).
    Find,
    /// Bulma kutusundaki metin değişti (ya da büyük/küçük harf seçeneği).
    FindText(String),
    /// Sonraki / önceki eşleşme (Enter, Shift+Enter, F3, Shift+F3).
    FindNext,
    FindPrev,
    /// Bulma kutusunu kapat (Esc, ✕).
    FindClose,
    /// Sekmedeki eşleşme sayısı değişti: (sekme, seçili eşleşme, toplam).
    FindCount(u64, i32, i32),
    CloseTab,
    /// Sekme şeridinde ✕ ya da orta tıkla belirli bir sekmeyi kapat.
    CloseTabAt(usize),
    /// Son kapatılan sekmeyi yeniden aç.
    ReopenTab,
    SelectTab(usize),
    /// Sekmeyi sürükleyerek başka bir sıraya taşı: (nereden, nereye).
    MoveTab(usize, usize),
    NextTab,
    PrevTab,
    LastTab,
    FocusAddress,
    /// Geliştirici araçlarını aç (F12).
    DevTools,
    /// Sayfayı yer imlerine ekle ya da çıkar (Ctrl+D, yıldız butonu).
    ToggleBookmark,
    /// Geçmiş sayfasını aç (Ctrl+H).
    OpenHistory,
    /// Ayarlar sayfasını aç (Ctrl+,).
    OpenSettings,
    /// Kenar çubuğundaki yer imine git.
    OpenBookmark(usize),
    /// Windows'un koyu/açık teması ya da vurgu rengi değişmiş olabilir.
    SystemThemeChanged,
    /// Sekmede ses çalmaya başladı/durdu.
    AudioChanged(u64, bool),
    /// Sayfanın ikonu (PNG) geldi.
    FaviconChanged(u64, Vec<u8>),
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
