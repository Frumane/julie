// Sürüm derlemesinde arka planda konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod address;
#[cfg_attr(not(windows), allow(dead_code))]
mod blocking;
#[cfg(windows)]
mod browser;
#[cfg_attr(not(windows), allow(dead_code))]
mod cert_page;
#[cfg(windows)]
mod chrome;
#[cfg_attr(not(windows), allow(dead_code))]
mod cosmetic;
#[cfg_attr(not(windows), allow(dead_code))]
mod default_browser;
#[cfg_attr(not(windows), allow(dead_code))]
mod devtools;
#[cfg_attr(not(windows), allow(dead_code))]
mod downloads;
#[cfg_attr(not(windows), allow(dead_code))]
mod error_page;
#[cfg_attr(not(windows), allow(dead_code))]
mod extensions;
#[cfg(windows)]
mod file_icon;
#[cfg_attr(not(windows), allow(dead_code))]
mod filter_lists;
#[cfg_attr(not(windows), allow(dead_code))]
mod history_page;
#[cfg_attr(not(windows), allow(dead_code))]
mod log;
#[cfg_attr(not(windows), allow(dead_code))]
mod pdf_content;
#[cfg_attr(not(windows), allow(dead_code))]
mod pdf_viewer;
#[cfg_attr(not(windows), allow(dead_code))]
mod places;
#[cfg_attr(not(windows), allow(dead_code))]
mod privacy;
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
mod site_info;
#[cfg_attr(not(windows), allow(dead_code))]
mod sleep;
#[cfg_attr(not(windows), allow(dead_code))]
mod start_page;
#[cfg_attr(not(windows), allow(dead_code))]
mod suggest;
#[cfg_attr(not(windows), allow(dead_code))]
mod tab_groups;

#[cfg_attr(not(windows), allow(dead_code))]
mod ui;

/// Arayüzden ve web görünümlerinden olay döngüsüne gelen olaylar.
#[derive(Debug, Clone)]
pub enum UserEvent {
    Back,
    Forward,
    Reload,
    /// Öndeki sekmenin yüklenmesini durdur.
    Stop,
    /// Julie açıkken Windows'tan (başka bir programdan) gelen adresler:
    /// yeni sekmelerde aç ve pencereyi öne getir.
    OpenExternal(Vec<String>),
    /// Sekme yüklenmeye başladı / yüklenmesi bitti.
    Loading(u64, bool),
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
    /// Geliştirici araçlarını aç ya da kapat (F12).
    DevTools,
    /// Sekmenin geliştirici araçlarından gelen mesaj (protokol komutu).
    DevToolsMessage(u64, String),
    /// Sayfanın sağ tık menüsünde "İncele" seçildi.
    InspectElement(u64),
    /// İncelenecek öğe bulundu (sekme, öğe kimliği).
    InspectNode(u64, i64),
    /// Sekmenin araçları sayfayı yükledi (bekleyen "İncele" gösterilebilir).
    DevToolsReady(u64),
    /// Araç panelinin ayracı sürükleniyor: imlecin x'i (fiziksel piksel),
    /// bırakıldı mı.
    DevToolsResize(i32, bool),
    /// Sayfayı yer imlerine ekle ya da çıkar (Ctrl+D, yıldız butonu).
    ToggleBookmark,
    /// Geçmiş sayfasını aç (Ctrl+H).
    OpenHistory,
    /// İndirmeler sayfasını aç (Ctrl+J, indirme düğmesi).
    OpenDownloads,
    /// İndirme listesi değişti; `true`: hemen göster (durum değişti),
    /// `false`: yalnızca ilerleme (seyrek gösterilir).
    DownloadsChanged(bool),
    /// İndirme düğmesine basıldı: paneli aç ya da kapat.
    DownloadsPanel,
    /// İndirme panelinde tıklanan yer (`layout::PanelClick::code`) ve çift
    /// tıklama mı.
    DownloadsPanelClick(usize, bool),
    /// Sekmeye sağ tıklandı: sekme menüsünü aç.
    TabMenu(usize),
    /// Grup başlığına tıklandı: grubu daralt/genişlet (model sırası).
    ToggleGroup(usize),
    /// Grup başlığına sağ tık: grup menüsü.
    GroupMenu(usize),
    /// Grup menüsündeki ad kutusuna yazıldı.
    GroupName(String),
    /// Sağ tık menüsünde seçilen satır.
    MenuChoice(usize),
    /// Sekmede indirme başladı (sekme boş kaldıysa kapanır).
    DownloadStarted(u64),
    /// Edge'in PDF görüntüleyicisinin kaydet düğmesi: PDF asıl adresinden
    /// yeniden indirilsin (sekme kimliği).
    SavePdf(u64),
    /// Edge'in görüntüleyicisindeki PDF'in sayfadan gelen parçası
    /// (sekme, sayfa adresi, `julie-pdf:` sonrası).
    PdfPiece(u64, String, String),
    /// Ayarlar sayfasını aç (Ctrl+,).
    OpenSettings,
    /// Mağaza sayfasında "Julie'ye ekle" (eklenti kimliği).
    ExtensionOffer(String),
    /// Eklenti indirilip açıldı: onay bekliyor (ya da hata).
    ExtensionStaged(Result<extensions::Pending, String>),
    /// Güncelleme denetimi bitti: (kimlik, yeni sürüm) ve kullanıcı mı istedi.
    ExtensionUpdates(Result<Vec<(String, String)>, String>, bool),
    /// Eklenti listesi okundu: sayfalara gönder.
    ExtensionsChanged,
    /// Eklenti değişti: listeyi profilden yeniden oku.
    ExtensionsRefresh,
    /// Kenar çubuğundaki yer imine git.
    OpenBookmark(usize),
    /// Windows'un koyu/açık teması ya da vurgu rengi değişmiş olabilir.
    SystemThemeChanged,
    /// Sekmede ses çalmaya başladı/durdu.
    AudioChanged(u64, bool),
    /// Sekmedeki bir öğe tam ekrana geçti / çıktı (video, PDF sunumu).
    FullScreen(u64, bool),
    /// Sekmenin yakınlaştırması değişti (1.0 = %100).
    ZoomChanged(u64, f64),
    /// Adres çubuğundaki yüzdeye basıldı: %100'e dön.
    ZoomReset,
    /// Sayfanın ikonu (PNG) geldi.
    FaviconChanged(u64, Vec<u8>),
    FocusPage,
    /// Bir sayfanın `window.open` / `target=_blank` isteği.
    OpenInNewTab(String),
    /// Sekmedeki site yeni pencere açmak istiyor: sekme ve isteğin kimliği.
    NewWindow(u64, u64),
    /// Sitenin açtığı pencere kendini kapatmak istiyor.
    WindowClose(u64),
    /// Site bildirim gösteriyor: sekme, bildirimin kimliği, site, başlık, metin.
    Notify(u64, u64, String, String, String),
    /// Bildirime tıklandı / bildirim kapatıldı (çarpı ya da süre doldu).
    ToastClick(u64),
    ToastClose(u64),
    /// Site bildirimini geri çekti.
    NotificationGone(u64),
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
    /// Sekmedeki sayfa yüklendi (sekme, adres).
    PageLoaded(u64, String),
    /// Sekmeyi başka adrese götür (şifreliye çevirme, takip parametresi
    /// temizleme, uyarı sayfası).
    Redirect(u64, String),
    /// Sekmede geçersiz bulunan sertifika zinciri (PEM, yapraktan köke);
    /// "Sertifikayı görüntüle" için.
    CertSeen(u64, Vec<String>),
    /// Kilit panelinden istenen sertifika zinciri geldi: yeni sekmede göster.
    OpenCertificate(Vec<String>),
    /// Bekleyen oturum kaydının zamanı geldi.
    SaveSession,
    /// Sekmedeki bir site izin istedi (soru `Browser::asks` listesinde).
    PermissionAsked(u64),
    /// İzin sorusu yanıtlandı.
    AskAnswer(ui::layout::AskTarget),
    /// Kilide tıklandı: site bilgisi panelini aç.
    SiteInfo,
    /// Sitenin profilde kayıtlı izinleri geldi: (sekme, köken, (tür, durum)).
    SitePermissions(u64, String, Vec<(i32, i32)>),
    /// Paneldeki bir izin satırına tıklandı (satır sırası).
    SitePermission(usize),
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
