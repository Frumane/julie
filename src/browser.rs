//! Pencere, sekmeler ve olay döngüsü.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{IconExtWindows, WindowExtWindows};
use tao::window::{Window, WindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
    COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR, COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
    COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG,
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, COREWEBVIEW2_PERMISSION_KIND,
    COREWEBVIEW2_PERMISSION_STATE, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    COREWEBVIEW2_PERMISSION_STATE_DENY, COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW,
    COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL,
    COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED, COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC,
    COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE, COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
    COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED, ICoreWebView2,
    ICoreWebView2_2, ICoreWebView2_3, ICoreWebView2_4, ICoreWebView2_8, ICoreWebView2_11,
    ICoreWebView2_13, ICoreWebView2_14, ICoreWebView2_15, ICoreWebView2_28,
    ICoreWebView2Certificate, ICoreWebView2Deferral,
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs2,
    ICoreWebView2DevToolsProtocolEventReceiver, ICoreWebView2DownloadOperation,
    ICoreWebView2Environment9, ICoreWebView2Environment15, ICoreWebView2Find,
    ICoreWebView2PermissionRequestedEventArgs, ICoreWebView2PermissionRequestedEventArgs3,
    ICoreWebView2Profile2, ICoreWebView2Profile3, ICoreWebView2Profile4, ICoreWebView2Settings8,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, BytesReceivedChangedEventHandler,
    CallDevToolsProtocolMethodCompletedHandler, ClearBrowsingDataCompletedHandler,
    ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler,
    DevToolsProtocolEventReceivedEventHandler, DownloadStartingEventHandler,
    FaviconChangedEventHandler, FindActiveMatchIndexChangedEventHandler,
    FindMatchCountChangedEventHandler, FindStartCompletedHandler, GetFaviconCompletedHandler,
    GetNonDefaultPermissionSettingsCompletedHandler, IsDocumentPlayingAudioChangedEventHandler,
    NavigationCompletedEventHandler, PermissionRequestedEventHandler,
    ServerCertificateErrorDetectedEventHandler, SetPermissionStateCompletedHandler,
    SourceChangedEventHandler, StateChangedEventHandler, TrySuspendCompletedHandler,
    ZoomFactorChangedEventHandler, take_pwstr,
};
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::core::{BOOL, HSTRING, Interface, PWSTR};
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use wry::http::{Response, StatusCode};
use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
};

use crate::UserEvent;
use crate::address::{self, DOWNLOADS_URL, HISTORY_URL, SETTINGS_URL, START_URL};
use crate::blocking::{self, Blocker};
use crate::cert_page;
use crate::chrome::Chrome;
use crate::cosmetic;
use crate::devtools;
use crate::downloads::{self, Action as DownloadAction, State as DownloadState};
use crate::error_page;
use crate::history_page;
use crate::places::{self, Places};
use crate::privacy::{self, CertAction, Decision};
use crate::request_filter::{self, TabState};
use crate::scriptlet_injector;
use crate::session::{SavedTab, Session};
use crate::settings::{self, Engine, Settings};
use crate::settings_page;
use crate::shortcuts::{self, Modifiers};
use crate::site_info;
use crate::sleep::{self, Action, Power};
use crate::start_page::{self, Message};
use crate::suggest;
use crate::ui::layout::{self, AskTarget, BookmarkInfo, Model, SuggestionRow, TabInfo};
use crate::ui::theme::{Appearance, ColorMode};

/// WebView2 tarayıcı sürecine verilen argümanlar. Aynı profili kullanan
/// bütün WebView'lerde birebir aynı olmalı, yoksa WebView2 oluşturulamaz.
///
/// - `--process-per-site`: Aynı sitenin sekmeleri tek renderer sürecini
///   paylaşır (farklı siteler yine ayrı süreçlerde, güvenlik aynı kalır).
///   Aynı siteden çok sekme açıldığında belleği belirgin düşürür.
/// - `msWebOOUI`, `msPdfOOUI`: Edge'in metin seçince çıkan mini menüsü.
/// - `MediaRouter`, `DialMediaRouteProvider`: Ağda Chromecast arayan arka
///   plan taraması (CPU ve ağ harcar, tarayıcımızda yayın özelliği yok).
/// - `AutofillServerCommunication`: Form alanı bilgilerinin sunucuya
///   gönderilmesi (otomatik doldurmanın kendisi çalışmaya devam eder).
/// - `SpareRendererForSitePerProcess`: Yeni sekme hızlı açılsın diye
///   boşta bekletilen yedek sayfa süreci (bellek harcar, kazancı birkaç ms).
///
/// - `--force-webrtc-ip-handling-policy`: Görüntülü görüşme (WebRTC)
///   sitelere bilgisayarın yerel ağ adresini sızdırmasın; yalnızca
///   internete çıkılan arayüz kullanılır. (LibreWolf ve Brave'de de var.)
///   Ayar değildir: değişirse açık gizli pencereyle argümanlar uyuşmaz.
///
/// SmartScreen burada kapatılmıyor; sekme başına ayardan yönetiliyor.
const BROWSER_ARGS: &str = "--process-per-site \
    --force-webrtc-ip-handling-policy=default_public_interface_only \
    --disable-features=msWebOOUI,msPdfOOUI,MediaRouter,DialMediaRouteProvider,AutofillServerCommunication,SpareRendererForSitePerProcess";

/// Donanım hızlandırma kapalıysa eklenen argüman. Argümanlar aynı veri
/// klasörünü kullanan bütün pencerelerde aynı olmalı; ayar bu yüzden
/// yeniden başlatınca geçerli olur.
fn browser_args(gpu_off: bool) -> String {
    if gpu_off {
        format!("{BROWSER_ARGS} --disable-gpu")
    } else {
        BROWSER_ARGS.to_string()
    }
}

struct Tab {
    id: u64,
    /// Sekme bellekten atıldıysa (uyutulduysa) `None`.
    webview: Option<WebView>,
    title: String,
    url: String,
    filter: TabState,
    power: Power,
    /// Sekme ne zamandan beri arka planda (öndeyse `None`).
    hidden_since: Option<Instant>,
    /// Geçmişe en son kaydedilen adres (aynı adres art arda kaydedilmesin).
    recorded_url: String,
    /// Sekmede ses çalıyor mu (sekme şeridindeki hoparlör ikonu).
    audio: bool,
    /// Geçerli sayfada genel gizleme kurallarından muaf seçiciler.
    cosmetic_exceptions: HashSet<String>,
    /// Geçerli sayfada genel gizleme kuralları kapalı mı (`$generichide`)?
    generichide: bool,
    /// Hata ya da uyarı sayfasından geri dönerken geçmişte kaç adım
    /// gidilecek: başarısız sayfa geçmişe girdiyse 2 (yoksa geri dönünce
    /// yeniden hata alıp sayfada takılı kalınır).
    error_back: u8,
    /// Sekmenin yanında açık geliştirici araçları (F12).
    devtools: Option<DevTools>,
    /// Yakınlaştırma (1.0 = %100).
    zoom: f64,
    /// Sekmenin sesi kapatıldı (sekme menüsünden).
    muted: bool,
}

/// Sekme menüsündeki komutlar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabCommand {
    NewTab,
    Reload,
    Duplicate,
    Mute,
    Close,
    CloseOthers,
    CloseRight,
    Reopen,
}

/// Bir sekmenin geliştirici araçları paneli: araçların kendi WebView'i ve
/// incelenen sayfadaki olay dinleyicileri.
struct DevTools {
    webview: WebView,
    receivers: Vec<(ICoreWebView2DevToolsProtocolEventReceiver, i64)>,
}

impl Drop for DevTools {
    fn drop(&mut self) {
        for (receiver, token) in &self.receivers {
            unsafe {
                let _ = receiver.remove_DevToolsProtocolEventReceived(*token);
            }
        }
    }
}

struct Browser {
    window: Window,
    chrome: Rc<RefCell<Chrome>>,
    context: WebContext,
    proxy: EventLoopProxy<UserEvent>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    /// Ctrl+Shift+T için son kapatılan sekmelerin adresleri.
    closed: Vec<String>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    /// Bu süreç donanım hızlandırma kapalı mı çalışıyor (ayar değişse de
    /// yeniden başlatana kadar aynı kalır).
    gpu_off: bool,
    /// Başlangıç sayfasını üreten işleyici WebView2'nin iş parçacığından
    /// çağrılabildiği için seçili motor burada paylaşılır.
    start_state: Arc<Mutex<start_page::State>>,
    filter: request_filter::Shared,
    /// Filtre listeleri indirilemediyse `true` (buton bunu gösterir).
    filters_error: bool,
    /// `None`: sekmeler hiç uyutulmaz (ayar).
    sleep_timing: Option<sleep::Timing>,
    /// Açık site panelinin izinleri.
    site: Option<SitePermissions>,
    /// Yanıt bekleyen izin istekleri (geliş sırasıyla). WebView2'nin olay
    /// işleyicisi de ekler; ikisi de arayüz iş parçacığında çalışır.
    asks: Rc<RefCell<Vec<PendingAsk>>>,
    session_path: Option<PathBuf>,
    /// Geçmiş ve yer imleri. Açılamazsa (disk hatası) `None`; tarayıcı
    /// yine çalışır, yalnızca bu özellikler devre dışı kalır.
    places: Option<Arc<Mutex<Places>>>,
    /// Son kayıttan beri sekmelerde değişiklik oldu mu?
    session_dirty: bool,
    /// Gecikmeli oturum kaydı istendi, henüz gelmedi.
    save_pending: bool,
    /// Arayüzde gösterilen durum (her olayda yeniden hesaplanmasın diye).
    bookmarks: Vec<BookmarkInfo>,
    can_back: bool,
    can_forward: bool,
    star: (bool, bool),
    shield: (String, bool),
    /// Veritabanından ikonu yüklenmiş (ya da yüklenmeye çalışılmış) siteler.
    favicons_loaded: RefCell<HashSet<String>>,
    /// Son sekme kapandı ve ayara göre tarayıcı da kapanmalı.
    quit: bool,
    /// Arama önerisi iş parçacığına kanal (ilk istekte başlatılır).
    remote_tx: Option<std::sync::mpsc::Sender<(Engine, String)>>,
    /// Arama motorundan gelmiş öneriler: (motor, küçük harfli metin).
    remote_cache: std::collections::HashMap<(Engine, String), Vec<String>>,
    /// Adres çubuğunda önerisi gösterilen metin ve tamamlanabilir mi.
    address_query: Option<(String, bool)>,
    /// Başlangıç sayfasının arama kutusundaki metin: (sekme kimliği, metin).
    page_query: Option<(u64, String)>,
    /// Gizli pencere: InPrivate sayfalar; geçmişe, site ikonlarına ve
    /// oturuma hiçbir şey yazılmaz.
    private: bool,
    /// HTTPS önce ve izleme parametresi temizleme (bütün sekmeler).
    guard: Arc<Mutex<privacy::Shield>>,
    /// Sekmede en son geçersiz bulunan sertifika zinciri (PEM), sekme
    /// kimliğine göre.
    certs: HashMap<u64, Vec<String>>,
    /// Bu oturumdaki indirmeler (indirmeler sayfası da okur).
    downloads: Arc<Mutex<downloads::List>>,
    /// Motorun indirme nesneleri (duraklat, devam, iptal) ve indirmeyi
    /// başlatan sekme, indirme kimliğine göre.
    download_ops: DownloadOps,
    /// İndirme ilerlemesinin en son gösterildiği an (seyrek göstermek için).
    downloads_shown: Instant,
    /// Kapatılan ama içinde indirme süren sekmelerin motoru. Motor
    /// kapanırsa indirme durur; indirme bitene kadar gizli bekletilir.
    parked: Vec<(u64, WebView)>,
    /// Araçlar açılırken bekleyen "İncele": sekme ve öğe kimliği.
    pending_inspect: Option<(u64, i64)>,
    /// Açık sekme menüsü: hangi sekme için ve satırların komutları.
    tab_menu: Option<(u64, Vec<Option<TabCommand>>)>,
}

type DownloadOps = Rc<RefCell<HashMap<u64, (u64, ICoreWebView2DownloadOperation)>>>;

fn app_title(private: bool) -> &'static str {
    if private { "Julie (Gizli)" } else { "Julie" }
}

/// Gizli pencere olarak başlatma argümanı (`julie.exe --private`).
const PRIVATE_ARG: &str = "--private";

/// Gizli pencere her zaman koyu görünür: hangi pencerede olduğun belli olsun.
fn shown_appearance(appearance: Appearance, private: bool) -> Appearance {
    if private {
        Appearance {
            mode: ColorMode::Dark,
            ..appearance
        }
    } else {
        appearance
    }
}

/// Aynı programı gizli pencere olarak başlatır. Ayrı bir süreçtir; WebView2
/// verileri bellekte tutulur ve pencere kapanınca silinir.
fn open_private_window() {
    let started = std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).arg(PRIVATE_ARG).spawn());
    if let Err(err) = started {
        crate::log::error(&format!("gizli pencere açılamadı: {err}"));
    }
}

pub fn run() {
    let private = std::env::args().any(|a| a == PRIVATE_ARG);
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(app_title(private))
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        // Uygulama ikonu julie.exe'ye gömülü (kaynak 1, bkz. build.rs).
        .with_window_icon(tao::window::Icon::from_resource(1, None).ok())
        .build(&event_loop)
        .expect("pencere oluşturulamadı");

    let app_dir = settings::app_dir();
    let settings_path = app_dir.as_ref().map(|dir| dir.join("settings.ini"));
    // Gizli pencere oturum kaydetmez ve açılışta eski sekmeleri açmaz.
    let session_path = app_dir
        .as_ref()
        .filter(|_| !private)
        .map(|dir| dir.join("session.txt"));
    let places = app_dir
        .as_ref()
        .and_then(|dir| open_places(&dir.join("places.sqlite")));
    let settings = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();

    let size = window.inner_size();
    let chrome = Chrome::create(
        window.hwnd() as _,
        proxy.clone(),
        shown_appearance(settings.appearance, private),
        window.scale_factor() as f32,
        (size.width, size.height),
    );
    let system_accent = chrome.borrow().system_accent();

    let filter = request_filter::Shared::default();
    *filter.disabled.borrow_mut() = settings.blocking_disabled.clone();
    if let Some(dir) = &app_dir {
        load_filters(&filter, &blocking::cache_path(dir), &proxy);
    }

    let sleep_timing = sleep::Timing::for_setting(settings.tab_sleep);
    let mut browser = Browser {
        window,
        chrome,
        context: WebContext::new(app_dir.map(|dir| dir.join("WebView2"))),
        proxy,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
        closed: Vec::new(),
        start_state: Arc::new(Mutex::new(start_page::State {
            engine: settings.engine,
            smartscreen: !settings.smartscreen_off,
            appearance: settings.appearance,
            system_accent,
            close_with_last_tab: settings.close_with_last_tab,
            search_suggestions: !settings.search_suggestions_off,
            autocomplete: !settings.autocomplete_off,
            private,
            privacy: settings.privacy,
            tab_sleep: settings.tab_sleep,
            gpu: !settings.gpu_off,
            gpu_running: !settings.gpu_off,
        })),
        certs: HashMap::new(),
        downloads: Arc::new(Mutex::new(downloads::List::default())),
        download_ops: Rc::new(RefCell::new(HashMap::new())),
        downloads_shown: Instant::now(),
        parked: Vec::new(),
        pending_inspect: None,
        tab_menu: None,
        guard: Arc::new(Mutex::new(privacy::Shield::new(
            !settings.privacy.https_first_off,
            !settings.privacy.strip_tracking_off,
        ))),
        gpu_off: settings.gpu_off,
        settings,
        settings_path,
        filter,
        filters_error: false,
        sleep_timing,
        site: None,
        asks: Rc::default(),
        session_path,
        places,
        session_dirty: false,
        save_pending: false,
        bookmarks: Vec::new(),
        can_back: false,
        can_forward: false,
        star: (false, false),
        shield: (String::new(), false),
        favicons_loaded: RefCell::new(HashSet::new()),
        quit: false,
        remote_tx: None,
        remote_cache: std::collections::HashMap::new(),
        address_query: None,
        page_query: None,
        private,
    };
    browser.reload_bookmarks();
    browser.layout();
    browser.restore_session();
    let check_every = browser
        .sleep_timing
        .map_or(sleep::Timing::NORMAL.check_every, |t| t.check_every);
    start_ticker(browser.proxy.clone(), check_every);
    browser.chrome.borrow().focus_address();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    browser.save_session();
                    browser.clear_on_close();
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    browser.layout()
                }
                _ => {}
            },
            Event::UserEvent(event) => {
                browser.handle(event);
                if browser.quit {
                    browser.save_session();
                    browser.clear_on_close();
                    *control_flow = ControlFlow::Exit;
                } else {
                    browser.schedule_save();
                }
            }
            _ => {}
        }
    });
}

/// Önbellekteki derlenmiş kuralları yükler. Önbellek yoksa ya da eskiyse
/// listeleri arka planda indirir; hazır olunca `FiltersReady` gelir.
fn load_filters(
    filter: &request_filter::Shared,
    cache: &std::path::Path,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let cached = std::fs::read(cache)
        .ok()
        .and_then(|b| Blocker::deserialize(&b));
    let have_cache = cached.is_some();
    *filter.blocker.borrow_mut() = cached;

    if !have_cache || blocking::needs_update(cache) {
        let proxy = proxy.clone();
        let cache = cache.to_path_buf();
        std::thread::spawn(move || match blocking::update(&cache) {
            Ok(bytes) => {
                let _ = proxy.send_event(UserEvent::FiltersReady(bytes));
            }
            Err(err) => {
                let _ = proxy.send_event(UserEvent::FiltersFailed(err));
            }
        });
    }
}

/// Uyutma kontrolü için olay döngüsüne periyodik `Tick` gönderir.
fn start_ticker(proxy: EventLoopProxy<UserEvent>, every: std::time::Duration) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(every);
            // Olay döngüsü kapandıysa iş parçacığı da biter.
            if proxy.send_event(UserEvent::Tick).is_err() {
                break;
            }
        }
    });
}

/// SmartScreen zararlı site korumasını sekmede açar/kapatır.
fn set_reputation_checking(webview: &WebView, on: bool) {
    unsafe {
        let _ = webview
            .webview()
            .Settings()
            .and_then(|settings| settings.cast::<ICoreWebView2Settings8>())
            .and_then(|settings| settings.SetIsReputationCheckingRequired(on));
    }
}

/// Edge'in kendi hata sayfası kapalı: sayfa açılamayınca Julie'nin hata
/// sayfası gelene kadar Edge'inki bir an görünmesin.
fn disable_builtin_error_page(webview: &WebView) {
    unsafe {
        let _ = webview
            .webview()
            .Settings()
            .and_then(|settings| settings.SetIsBuiltInErrorPageEnabled(false));
    }
}

/// Sekmede ses ya da video çalıyor mu?
fn is_playing_audio(webview: &WebView) -> bool {
    unsafe {
        webview
            .webview()
            .cast::<ICoreWebView2_8>()
            .and_then(|core| {
                let mut playing = BOOL::default();
                core.IsDocumentPlayingAudio(&mut playing)?;
                Ok(playing.as_bool())
            })
            .unwrap_or(false)
    }
}

/// WebView2'den sayfayı dondurmasını ister (gizli olmalı).
fn try_suspend(webview: &WebView) -> bool {
    unsafe {
        webview
            .webview()
            .cast::<ICoreWebView2_3>()
            .and_then(|core| {
                core.TrySuspend(&TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(()))))
            })
            .is_ok()
    }
}

/// Ctrl+Shift+T ile geri açılabilecek en fazla sekme sayısı.
const MAX_CLOSED: usize = 25;

/// Geçmiş veritabanını açar ve süresi dolmuş kayıtları siler.
fn open_places(path: &std::path::Path) -> Option<Arc<Mutex<Places>>> {
    match Places::open(path) {
        Ok(p) => {
            let cutoff = places::now() - places::HISTORY_DAYS * 24 * 60 * 60;
            if let Err(err) = p.expire(cutoff) {
                crate::log::error(&format!("eski geçmiş silinemedi: {err}"));
            }
            Some(Arc::new(Mutex::new(p)))
        }
        Err(err) => {
            crate::log::error(&format!("geçmiş veritabanı açılamadı: {err}"));
            None
        }
    }
}

/// Tarayıcının kendi sayfalarının (başlangıç, ayarlar, geçmiş) ikon anahtarı.
/// İki nokta içerdiği için hiçbir alan adıyla çakışmaz.
const JULIE_ICON_KEY: &str = "julie:";

/// Julie logosunun küçük boyutlu hâli; kendi sayfalarımızın sekme ikonu.
const JULIE_ICON_PNG: &[u8] = include_bytes!("../assets/logo-32.png");

/// Sekmede ve yer imlerinde gösterilecek ikonun anahtarı.
fn icon_key(url: &str) -> Option<String> {
    if address::is_internal_page(url) {
        return Some(JULIE_ICON_KEY.to_string());
    }
    favicon_key(url)
}

/// Site ikonlarının anahtarı: web sayfalarında `www.` olmadan alan adı.
fn favicon_key(url: &str) -> Option<String> {
    if !places::is_recordable(url) {
        return None;
    }
    address::host_of(url)
}

/// Sayfanın ikonu değişince PNG olarak alır.
fn listen_for_favicon(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let Ok(core) = webview.webview().cast::<ICoreWebView2_15>() else {
        return;
    };
    let handler = FaviconChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(core) = sender.and_then(|s| s.cast::<ICoreWebView2_15>().ok()) else {
            return Ok(());
        };
        let proxy = proxy.clone();
        let done = GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
            if let (Ok(()), Some(stream)) = (result, stream) {
                let png = read_stream(&stream, places::MAX_FAVICON_BYTES);
                if !png.is_empty() {
                    let _ = proxy.send_event(UserEvent::FaviconChanged(id, png));
                }
            }
            Ok(())
        }));
        unsafe { core.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &done) }
    }));
    let mut token = 0i64;
    unsafe {
        let _ = core.add_FaviconChanged(&handler, &mut token);
    }
}

/// Akıştaki baytları en fazla `limit` kadar okur; daha büyükse boş döner.
fn read_stream(stream: &IStream, limit: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut read = 0u32;
        let hr = unsafe { stream.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut read)) };
        if hr.is_err() || read == 0 {
            break;
        }
        out.extend_from_slice(&buf[..read as usize]);
        if out.len() > limit {
            return Vec::new();
        }
    }
    out
}

/// Sekmede ses çalmaya başlayınca ya da durunca haber verir.
fn listen_for_audio(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let Ok(core) = webview.webview().cast::<ICoreWebView2_8>() else {
        return;
    };
    let handler = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(core) = sender.and_then(|s| s.cast::<ICoreWebView2_8>().ok()) {
            let mut playing = BOOL::default();
            unsafe { core.IsDocumentPlayingAudio(&mut playing)? };
            let _ = proxy.send_event(UserEvent::AudioChanged(id, playing.as_bool()));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = core.add_IsDocumentPlayingAudioChanged(&handler, &mut token);
    }
}

/// Sekmenin sesini kapatır ya da açar.
fn set_muted(webview: Option<&WebView>, muted: bool) {
    if let Some(core) = webview.and_then(|w| w.webview().cast::<ICoreWebView2_8>().ok()) {
        unsafe {
            let _ = core.SetIsMuted(muted);
        }
    }
}

/// Sayfanın sağ tık menüsünün sonuna "İncele" ekler.
fn listen_for_context_menu(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let Ok(core) = webview.webview().cast::<ICoreWebView2_11>() else {
        return;
    };
    let Some(env) = webview
        .webview()
        .cast::<ICoreWebView2_2>()
        .ok()
        .and_then(|w| unsafe { w.Environment() }.ok())
        .and_then(|e| e.cast::<ICoreWebView2Environment9>().ok())
    else {
        return;
    };
    let handler = ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        unsafe {
            let items = args.MenuItems()?;
            let mut count = 0u32;
            items.Count(&mut count)?;
            let separator = env.CreateContextMenuItem(
                &HSTRING::new(),
                None::<&IStream>,
                COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR,
            )?;
            let inspect = env.CreateContextMenuItem(
                &HSTRING::from("İncele"),
                None::<&IStream>,
                COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
            )?;
            let proxy = proxy.clone();
            let selected = CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                let _ = proxy.send_event(UserEvent::InspectElement(id));
                Ok(())
            }));
            let mut token = 0i64;
            inspect.add_CustomItemSelected(&selected, &mut token)?;
            if count > 0 {
                items.InsertValueAtIndex(count, &separator)?;
                count += 1;
            }
            items.InsertValueAtIndex(count, &inspect)?;
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = core.add_ContextMenuRequested(&handler, &mut token);
    }
}

/// Sayfa yakınlaştırılınca (Ctrl + +/-, Ctrl + tekerlek) haber verir.
fn listen_for_zoom(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let handler = ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(controller) = sender {
            let mut zoom = 1.0;
            unsafe { controller.ZoomFactor(&mut zoom)? };
            let _ = proxy.send_event(UserEvent::ZoomChanged(id, zoom));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview
            .controller()
            .add_ZoomFactorChanged(&handler, &mut token);
    }
}

/// Sayfanın adresi her değiştiğinde haber verir. Sayfa yüklemesinden farklı
/// olarak tek sayfalık uygulamaların (YouTube'da video değiştirmek gibi)
/// yeniden yükleme olmadan yaptığı adres değişikliklerini de yakalar.
fn listen_for_source_changes(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(core) = sender {
            let mut uri = PWSTR::null();
            unsafe { core.Source(&mut uri)? };
            let _ = proxy.send_event(UserEvent::UrlChanged(id, take_pwstr(uri)));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview.webview().add_SourceChanged(&handler, &mut token);
    }
}

/// Sayfa açılamazsa hata sayfasına, şifreliye çevrilen gezinti başarısız
/// olursa uyarı sayfasına geç. `cert_events`: sertifika hataları ayrıca
/// yakalanıyor (`listen_for_cert_errors`). `cert_page`: sertifika olayı bu
/// gezintiyi zaten bir uyarıya yönlendirdi; iptalin ardından gelen
/// "bağlantı kesildi" sonucu o uyarının üstüne başka sayfa açmasın.
fn listen_for_navigation_result(
    webview: &WebView,
    guard: Arc<Mutex<privacy::Shield>>,
    proxy: EventLoopProxy<UserEvent>,
    id: u64,
    cert_events: bool,
    cert_page: Arc<AtomicBool>,
) {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut success = BOOL::default();
        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
        unsafe {
            args.IsSuccess(&mut success)?;
            args.WebErrorStatus(&mut status)?;
        }
        let cancelled = status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED;
        let kind = error_page::kind_for_status(status.0);
        let mut guard = guard.lock().unwrap();
        let target = match guard.finished(id, success.as_bool(), cancelled) {
            // Site hiç yoksa ya da internet yoksa sorun şifreleme değil.
            Some(http) if matches!(kind, Some("dns" | "offline")) => {
                kind.map(|k| error_page::error_url(k, status.0, &http))
            }
            Some(http) => Some(privacy::warning_url(&http)),
            None if success.as_bool() || cancelled => None,
            None => kind
                .filter(|k| !(*k == "cert" && cert_events))
                .and_then(|k| {
                    let url = guard.last_started(id)?;
                    let web = url.starts_with("http://") || url.starts_with("https://");
                    (web && !address::is_internal_page(url))
                        .then(|| error_page::error_url(k, status.0, url))
                }),
        };
        drop(guard);
        let handled = cert_page.swap(false, Ordering::SeqCst);
        if let Some(target) = target.filter(|_| !handled || success.as_bool()) {
            let _ = proxy.send_event(UserEvent::Redirect(id, target));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview
            .webview()
            .add_NavigationCompleted(&handler, &mut token);
    }
}

/// Geçersiz sertifikalı sayfada WebView2'nin kendi uyarısı yerine Julie'nin
/// hata sayfasını göster; kullanıcı devam etmeyi seçtiyse izin ver.
/// Çalışma zamanı desteklemiyorsa `false`.
fn listen_for_cert_errors(
    webview: &WebView,
    guard: Arc<Mutex<privacy::Shield>>,
    proxy: EventLoopProxy<UserEvent>,
    id: u64,
    cert_page: Arc<AtomicBool>,
) -> bool {
    let Ok(core) = webview.webview().cast::<ICoreWebView2_14>() else {
        return false;
    };
    let handler = ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut uri = PWSTR::null();
        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
        unsafe {
            args.RequestUri(&mut uri)?;
            args.ErrorStatus(&mut status)?;
        }
        let uri = take_pwstr(uri);
        let mut guard = guard.lock().unwrap();
        // Olay sayfanın içindeki dosyalar için de gelir; yalnızca açılan
        // sayfanın kendisi hata sayfasına götürür (diğerleri engellenir).
        let page = guard
            .last_started(id)
            .and_then(site_info::origin_of)
            .zip(site_info::origin_of(&uri))
            .is_some_and(|(a, b)| site_info::same_origin(&a, &b));
        let action = guard.cert_error(id, &uri);
        drop(guard);
        let (allow, target) = match action {
            CertAction::Allow => (true, None),
            _ if !page => return Ok(()),
            CertAction::Warn(http) => (false, Some(privacy::warning_url(&http))),
            CertAction::Error => {
                let cert = unsafe { args.ServerCertificate() }.ok();
                let info = cert.as_ref().and_then(cert_info);
                if let Some(chain) = cert.as_ref().and_then(cert_chain) {
                    let _ = proxy.send_event(UserEvent::CertSeen(id, chain));
                }
                (
                    false,
                    Some(error_page::cert_url(status.0, &uri, info.as_ref())),
                )
            }
        };
        unsafe {
            args.SetAction(if allow {
                COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW
            } else {
                COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL
            })?;
        }
        if let Some(target) = target {
            cert_page.store(true, Ordering::SeqCst);
            let _ = proxy.send_event(UserEvent::Redirect(id, target));
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { core.add_ServerCertificateErrorDetected(&handler, &mut token) }.is_ok()
}

/// Ayrıntılar'da gösterilecek sertifika bilgisi.
fn cert_info(cert: &ICoreWebView2Certificate) -> Option<error_page::CertInfo> {
    let mut subject = PWSTR::null();
    let mut issuer = PWSTR::null();
    let (mut from, mut to) = (0f64, 0f64);
    unsafe {
        cert.Subject(&mut subject).ok()?;
        cert.Issuer(&mut issuer).ok()?;
        cert.ValidFrom(&mut from).ok()?;
        cert.ValidTo(&mut to).ok()?;
    }
    Some(error_page::CertInfo {
        subject: take_pwstr(subject),
        issuer: take_pwstr(issuer),
        valid_from: from as i64,
        valid_to: to as i64,
    })
}

/// Sertifika zinciri (PEM metinleri): önce sitenin kendi sertifikası,
/// sonra onu verenler, en sonda kök.
fn cert_chain(cert: &ICoreWebView2Certificate) -> Option<Vec<String>> {
    let mut pem = PWSTR::null();
    unsafe { cert.ToPemEncoding(&mut pem) }.ok()?;
    let leaf = take_pwstr(pem);
    if leaf.is_empty() {
        return None;
    }
    let mut chain = vec![leaf];
    if let Ok(issuers) = unsafe { cert.PemEncodedIssuerCertificateChain() } {
        let mut count = 0u32;
        let _ = unsafe { issuers.Count(&mut count) };
        for i in 0..count.min(10) {
            let mut value = PWSTR::null();
            if unsafe { issuers.GetValueAtIndex(i, &mut value) }.is_ok() {
                let pem = take_pwstr(value);
                // Liste sitenin sertifikasıyla başlayabilir; iki kez gösterme.
                if !pem.is_empty() && !chain.iter().any(|c| c.trim() == pem.trim()) {
                    chain.push(pem);
                }
            }
        }
    }
    Some(chain)
}

/// Profilde (bütün sekmelerde ortak) kayıtlı site izinlerini ister; sonuç
/// `UserEvent::SitePermissions` olarak gelir.
fn request_permissions(
    webview: &WebView,
    proxy: EventLoopProxy<UserEvent>,
    id: u64,
    origin: String,
) -> windows::core::Result<()> {
    let profile = unsafe { webview.webview().cast::<ICoreWebView2_13>()?.Profile()? }
        .cast::<ICoreWebView2Profile4>()?;
    let done =
        GetNonDefaultPermissionSettingsCompletedHandler::create(Box::new(move |result, list| {
            let mut set = Vec::new();
            if let (Ok(()), Some(list)) = (result, list) {
                let mut count = 0u32;
                unsafe { list.Count(&mut count)? };
                for i in 0..count {
                    let item = unsafe { list.GetValueAtIndex(i)? };
                    let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                    let mut state = COREWEBVIEW2_PERMISSION_STATE::default();
                    let mut item_origin = PWSTR::null();
                    unsafe {
                        item.PermissionKind(&mut kind)?;
                        item.PermissionState(&mut state)?;
                        item.PermissionOrigin(&mut item_origin)?;
                    }
                    if site_info::same_origin(&take_pwstr(item_origin), &origin) {
                        set.push((kind.0, state.0));
                    }
                }
            }
            let _ = proxy.send_event(UserEvent::SitePermissions(id, origin, set));
            Ok(())
        }));
    unsafe { profile.GetNonDefaultPermissionSettings(&done) }
}

/// Sitenin bir iznini profilde değiştirir.
fn set_permission(
    webview: &WebView,
    origin: &str,
    kind: i32,
    state: i32,
) -> windows::core::Result<()> {
    let profile = unsafe { webview.webview().cast::<ICoreWebView2_13>()?.Profile()? }
        .cast::<ICoreWebView2Profile4>()?;
    let done = SetPermissionStateCompletedHandler::create(Box::new(|result| {
        if let Err(err) = result {
            crate::log::error(&format!("izin değiştirilemedi: {err}"));
        }
        Ok(())
    }));
    unsafe {
        profile.SetPermissionState(
            COREWEBVIEW2_PERMISSION_KIND(kind),
            &HSTRING::from(origin),
            COREWEBVIEW2_PERMISSION_STATE(state),
            &done,
        )
    }
}

/// Edge'in izleme önleme seviyesi. Profil bütün sekmelerde ortaktır.
fn set_tracking_prevention(webview: &WebView, level: privacy::TrackingLevel) {
    use privacy::TrackingLevel as L;
    let value = match level {
        L::Off => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
        L::Basic => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC,
        L::Balanced => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
        L::Strict => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
    };
    let result = (|| unsafe {
        webview
            .webview()
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile3>()?
            .SetPreferredTrackingPreventionLevel(value)
    })();
    if let Err(err) = result {
        crate::log::error(&format!("izleme önleme ayarlanamadı: {err}"));
    }
}

/// Sayfalara saat dilimini UTC gösterir (`on` değilse gerçek saat dilimi).
/// Ayar sekmenin ömrü boyunca geçerlidir; açık sayfa yenileyince görür.
/// `then`, komut bitince (başarılı ya da değil) bir kez çalışır.
fn set_timezone(webview: &WebView, on: bool, then: Option<Box<dyn FnOnce()>>) {
    let params = if on {
        r#"{"timezoneId":"UTC"}"#
    } else {
        r#"{"timezoneId":""}"#
    };
    let then = Rc::new(RefCell::new(then));
    let later = Rc::clone(&then);
    let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
        if let Err(err) = result {
            crate::log::error(&format!("saat dilimi ayarlanamadı: {err}"));
        }
        if let Some(then) = later.borrow_mut().take() {
            then();
        }
        Ok(())
    }));
    let sent = unsafe {
        webview.webview().CallDevToolsProtocolMethod(
            &HSTRING::from("Emulation.setTimezoneOverride"),
            &HSTRING::from(params),
            &done,
        )
    };
    if let Err(err) = sent {
        crate::log::error(&format!("saat dilimi ayarlanamadı: {err}"));
        if let Some(then) = then.borrow_mut().take() {
            then();
        }
    }
}

/// Sitenin sertifika zincirini motordan ister (`Network.getCertificate`);
/// gelince `UserEvent::OpenCertificate` gönderir.
fn request_certificate(webview: &WebView, proxy: EventLoopProxy<UserEvent>, origin: &str) {
    let params = serde_json::json!({ "origin": origin }).to_string();
    let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
        if let Err(err) = result {
            crate::log::error(&format!("sertifika alınamadı: {err}"));
            return Ok(());
        }
        let chain = cert_page::chain_from_devtools(&json);
        if chain.is_empty() {
            crate::log::error("sertifika alınamadı: boş yanıt");
        } else {
            let _ = proxy.send_event(UserEvent::OpenCertificate(chain));
        }
        Ok(())
    }));
    let sent = unsafe {
        webview.webview().CallDevToolsProtocolMethod(
            &HSTRING::from("Network.getCertificate"),
            &HSTRING::from(params),
            &done,
        )
    };
    if let Err(err) = sent {
        crate::log::error(&format!("sertifika alınamadı: {err}"));
    }
}

/// Profildeki bütün çerezleri ve site verilerini siler; bitene kadar bekler.
/// Geçmiş, oturum ve ayarlar Julie'nin kendi dosyalarında olduğundan kalır.
fn clear_site_data(webview: &WebView) -> webview2_com::Result<()> {
    let profile = unsafe { webview.webview().cast::<ICoreWebView2_13>()?.Profile()? }
        .cast::<ICoreWebView2Profile2>()?;
    ClearBrowsingDataCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            profile
                .ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(|result| result),
    )
}

// ------------------------------------------------------------ sayfada bul

/// Sekmenin bulma oturumu (WebView2 1.0.2849+ çalışma zamanında var).
fn find_of(webview: &WebView) -> Option<ICoreWebView2Find> {
    let core = webview.webview().cast::<ICoreWebView2_28>().ok()?;
    unsafe { core.Find() }.ok()
}

/// Metni sayfada arar. WebView2'nin kendi bulma kutusu gizlenir; bütün
/// eşleşmeler vurgulanır, ilk eşleşmeye gidilir.
fn start_find(webview: &WebView, text: &str, case_sensitive: bool) -> windows::core::Result<()> {
    let core = webview.webview();
    let find = core.cast::<ICoreWebView2_28>()?;
    let environment: ICoreWebView2Environment15 =
        unsafe { core.cast::<ICoreWebView2_2>()?.Environment()? }.cast()?;
    unsafe {
        let options = environment.CreateFindOptions()?;
        options.SetFindTerm(&windows::core::HSTRING::from(text))?;
        options.SetIsCaseSensitive(case_sensitive)?;
        options.SetShouldHighlightAllMatches(true)?;
        options.SetSuppressDefaultFindDialog(true)?;
        let done = FindStartCompletedHandler::create(Box::new(|_| Ok(())));
        find.Find()?.Start(&options, &done)
    }
}

fn stop_find(webview: &WebView) {
    if let Some(find) = find_of(webview) {
        let _ = unsafe { find.Stop() };
    }
}

/// Eşleşme sayısı ya da seçili eşleşme değişince bulma kutusuna bildirir.
fn listen_for_find(webview: &WebView, proxy: EventLoopProxy<UserEvent>, id: u64) {
    let Some(find) = find_of(webview) else {
        return;
    };
    let report = move |sender: Option<ICoreWebView2Find>| {
        if let Some(find) = sender {
            let (mut active, mut total) = (0i32, 0i32);
            unsafe {
                find.ActiveMatchIndex(&mut active)?;
                find.MatchCount(&mut total)?;
            }
            let _ = proxy.send_event(UserEvent::FindCount(id, active, total));
        }
        Ok(())
    };
    let count_report = report.clone();
    let count =
        FindMatchCountChangedEventHandler::create(Box::new(move |sender, _| count_report(sender)));
    let active =
        FindActiveMatchIndexChangedEventHandler::create(Box::new(move |sender, _| report(sender)));
    let mut token = 0i64;
    unsafe {
        let _ = find.add_MatchCountChanged(&count, &mut token);
        let _ = find.add_ActiveMatchIndexChanged(&active, &mut token);
    }
}

/// Sayfa odaktayken basılan kısayolları yakalar. Bizim kısayolumuzsa
/// WebView2'nin kendi davranışını durdurur ve komutu olay döngüsüne yollar;
/// değilse tuş sayfaya (ya da WebView2'nin yerleşik kısayollarına) gider.
/// `devtools`: geliştirici araçlarının paneli; orada Ctrl+F ve F3 araçların
/// kendi aramasına kalır.
fn listen_for_shortcuts(webview: &WebView, proxy: EventLoopProxy<UserEvent>, devtools: bool) {
    // Eski WebView2 sürümlerinde bulma API'si yok: o zaman Ctrl+F ve F3
    // yakalanmaz, WebView2'nin kendi bulma kutusu açılır.
    let own_find = !devtools && find_of(webview).is_some();
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        unsafe {
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                return Ok(());
            }
            let mut key = 0u32;
            args.VirtualKey(&mut key)?;
            let down = |vk: u16| GetKeyState(vk.into()) < 0;
            let modifiers = Modifiers {
                ctrl: down(VK_CONTROL.0),
                shift: down(VK_SHIFT.0),
                alt: down(VK_MENU.0),
            };
            if let Some(event) = shortcuts::lookup(key, modifiers)
                && (own_find
                    || !matches!(
                        event,
                        UserEvent::Find | UserEvent::FindNext | UserEvent::FindPrev
                    ))
            {
                args.SetHandled(true)?;
                let _ = proxy.send_event(event);
            }
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview
            .controller()
            .add_AcceleratorKeyPressed(&handler, &mut token);
    }
}

/// wry sayfa penceresini kardeşlerini kırpmadan oluşturuyor; o zaman sayfa,
/// üstünde duran bulma kutusunun üzerine çiziliyor ve kutu görünmüyor.
fn clip_siblings(webview: &WebView) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_CLIPSIBLINGS,
    };
    let hwnd = webview.hwnd().0;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_CLIPSIBLINGS as isize);
    }
}

/// İndirmeleri Edge'in indirme kutusu yerine Julie'nin listesine alır.
fn listen_for_downloads(
    webview: &WebView,
    tab: u64,
    list: Arc<Mutex<downloads::List>>,
    ops: DownloadOps,
    proxy: EventLoopProxy<UserEvent>,
) {
    let Ok(core) = webview.webview().cast::<ICoreWebView2_4>() else {
        return;
    };
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        unsafe {
            // Edge'in kendi indirme kutusu açılmasın.
            args.SetHandled(true)?;
            let op = args.DownloadOperation()?;
            let mut path = PWSTR::null();
            args.ResultFilePath(&mut path)?;
            let path = take_pwstr(path);
            let mut uri = PWSTR::null();
            op.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let mut total = 0i64;
            op.TotalBytesToReceive(&mut total)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let id = list
                .lock()
                .unwrap()
                .add(&uri, &path, u64::try_from(total).ok(), now);
            if let Some(d) = list.lock().unwrap().get_mut(id) {
                d.icon = crate::file_icon::icon(&d.name(), None);
            }
            ops.borrow_mut().insert(id, (tab, op.clone()));

            let bytes_list = Arc::clone(&list);
            let bytes_proxy = proxy.clone();
            let bytes = BytesReceivedChangedEventHandler::create(Box::new(move |op, _| {
                if let Some(op) = op {
                    let (mut got, mut total) = (0i64, 0i64);
                    op.BytesReceived(&mut got)?;
                    op.TotalBytesToReceive(&mut total)?;
                    bytes_list.lock().unwrap().set_received(
                        id,
                        got.max(0) as u64,
                        u64::try_from(total).ok(),
                        Instant::now(),
                    );
                    let _ = bytes_proxy.send_event(UserEvent::DownloadsChanged(false));
                }
                Ok(())
            }));
            let mut token = 0i64;
            op.add_BytesReceivedChanged(&bytes, &mut token)?;

            let state_list = Arc::clone(&list);
            let state_proxy = proxy.clone();
            let changed = StateChangedEventHandler::create(Box::new(move |op, _| {
                if let Some(op) = op {
                    update_download(&op, id, &state_list)?;
                    // "Bitince aç" seçiliyse dosya kendiliğinden açılır.
                    let open = {
                        let mut list = state_list.lock().unwrap();
                        list.get_mut(id)
                            .filter(|d| d.state == DownloadState::Done && d.open_when_done)
                            .map(|d| {
                                d.open_when_done = false;
                                d.path.clone()
                            })
                    };
                    if let Some(path) = open {
                        open_file(&path);
                    }
                    let _ = state_proxy.send_event(UserEvent::DownloadsChanged(true));
                }
                Ok(())
            }));
            op.add_StateChanged(&changed, &mut token)?;
        }
        let _ = proxy.send_event(UserEvent::DownloadsChanged(true));
        let _ = proxy.send_event(UserEvent::DownloadStarted(tab));
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = core.add_DownloadStarting(&handler, &mut token);
    }
}

/// Motorun indirme nesnesinden listedeki kaydı günceller.
fn update_download(
    op: &ICoreWebView2DownloadOperation,
    id: u64,
    list: &Mutex<downloads::List>,
) -> windows::core::Result<()> {
    unsafe {
        let mut state = Default::default();
        op.State(&mut state)?;
        let mut reason = Default::default();
        op.InterruptReason(&mut reason)?;
        let mut can_resume = BOOL::default();
        op.CanResume(&mut can_resume)?;
        let mut path = PWSTR::null();
        op.ResultFilePath(&mut path)?;
        let path = take_pwstr(path);
        let mut got = 0i64;
        op.BytesReceived(&mut got)?;
        // Duraklatmak ve iptal etmek de "kesinti" sayılır; nedeninden ayrılır.
        let state = match state {
            COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS => DownloadState::Active,
            COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => DownloadState::Done,
            _ => match reason.0 {
                28 => DownloadState::Paused,
                26 => DownloadState::Cancelled,
                _ => DownloadState::Failed,
            },
        };
        let mut list = list.lock().unwrap();
        list.set_state(id, state);
        if let Some(d) = list.get_mut(id) {
            d.reason = if state == DownloadState::Failed {
                downloads::reason_text(reason.0).to_string()
            } else {
                String::new()
            };
            d.can_resume = can_resume.as_bool();
            if !path.is_empty() {
                d.path = path;
            }
            d.received = got.max(0) as u64;
            if state == DownloadState::Done {
                d.total = Some(d.total.unwrap_or(0).max(d.received));
                // Programların kendi simgesi var; dosya inince ondan alınır.
                if let Some(icon) = crate::file_icon::icon(&d.name(), Some(&d.path)) {
                    d.icon = Some(icon);
                }
            }
        }
    }
    Ok(())
}

/// İndirilen dosyayı Windows'un o türe atadığı programla açar.
fn open_file(path: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = path.encode_utf16().chain([0]).collect();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Dosyanın klasörünü Gezgin'de, dosya seçili olarak açar.
fn show_in_folder(path: &str) {
    use std::os::windows::process::CommandExt;
    let result = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn();
    if let Err(err) = result {
        crate::log::error(&format!("klasör açılamadı: {err}"));
    }
}

/// Pencereyi verilen yere (fiziksel piksel) koyar ve köşelerini yuvarlar.
fn place_rounded(webview: &WebView, (x, y, w, h): (i32, i32, u32, u32), radius: i32) {
    let _ = webview.set_bounds(Rect {
        position: PhysicalPosition::new(x, y).into(),
        size: PhysicalSize::new(w, h).into(),
    });
    unsafe {
        let region = CreateRoundRectRgn(0, 0, w as i32 + 1, h as i32 + 1, 2 * radius, 2 * radius);
        // Pencere bölgeyi sahiplenir; silmek gerekmez.
        SetWindowRgn(webview.hwnd().0, region, 1);
    }
}

/// wry adresi `julie://devtools/` ya da `https://julie.devtools/` olarak verebilir.
fn is_devtools_host(host: Option<&str>) -> bool {
    matches!(host, Some("devtools" | "julie.devtools"))
}

/// Geliştirici araçlarının dosyaları (programın içindeki paketten).
fn serve_devtools(uri: &wry::http::Uri) -> Response<Cow<'static, [u8]>> {
    let found = is_devtools_host(uri.host())
        .then(|| devtools::file(uri.path()))
        .flatten();
    let Some((body, mime)) = found else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Cow::Borrowed(&[][..]))
            .expect("geçerli yanıt");
    };
    // Araçların sayfası kendi güvenlik politikasını taşır.
    Response::builder()
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(body))
        .expect("geçerli yanıt")
}

/// Sayfanın motor olaylarını araçlara iletir; dinleyicileri döndürür
/// (araçlar kapanınca kaldırılır).
fn forward_devtools_events(
    page: &ICoreWebView2,
    tools: &ICoreWebView2,
) -> Vec<(ICoreWebView2DevToolsProtocolEventReceiver, i64)> {
    let mut receivers = Vec::new();
    for name in devtools::events() {
        let Ok(receiver) = (unsafe { page.GetDevToolsProtocolEventReceiver(&HSTRING::from(name)) })
        else {
            continue;
        };
        let tools = tools.clone();
        let method = name.clone();
        let handler =
            DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                unsafe {
                    let mut json = PWSTR::null();
                    args.ParameterObjectAsJson(&mut json)?;
                    let params = take_pwstr(json);
                    let session = args
                        .cast::<ICoreWebView2DevToolsProtocolEventReceivedEventArgs2>()
                        .ok()
                        .and_then(|a| {
                            let mut id = PWSTR::null();
                            a.SessionId(&mut id).ok()?;
                            Some(take_pwstr(id))
                        });
                    let message = devtools::event(&method, &params, session.as_deref());
                    let _ = tools.PostWebMessageAsString(&HSTRING::from(message));
                }
                Ok(())
            }));
        let mut token = 0i64;
        if unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }.is_ok() {
            receivers.push((receiver, token));
        }
    }
    receivers
}

/// `julie://` isteklerini karşılar. Şimdilik tek sayfa var: başlangıç sayfası.
fn serve(
    uri: &wry::http::Uri,
    state: start_page::State,
    places: Option<&Mutex<Places>>,
    downloads: &Mutex<downloads::List>,
) -> Response<Cow<'static, [u8]>> {
    // WebView2 adresi `julie://history/` ya da `http://julie.history/`
    // biçiminde verebilir; ikisinde de sayfa adı ana bilgisayar kısmıdır.
    let host = uri.host().unwrap_or_default();
    let name = host.strip_prefix("julie.").unwrap_or(host);
    let html = match (name, uri.path()) {
        ("start", "/") => {
            let places = places.and_then(|p| p.lock().ok());
            let bookmarks = places
                .as_ref()
                .and_then(|p| p.bookmarks().ok())
                .unwrap_or_default();
            start_page::render(state, &bookmarks, |url| {
                places.as_ref()?.favicon(&favicon_key(url)?).ok().flatten()
            })
        }
        ("history", "/") => {
            let query = history_page::query_from(uri.query());
            let entries = places
                .and_then(|p| p.lock().ok()?.history(&query, history_page::LIMIT).ok())
                .unwrap_or_default();
            history_page::render(&query, &entries)
        }
        ("settings", "/") => settings_page::render(&state),
        ("downloads", "/") => downloads::render(&downloads.lock().unwrap()),
        ("insecure", "/") => privacy::render_warning(uri.query()),
        ("error", "/") => error_page::render(uri.query()),
        ("fly", "/") => error_page::render_game(),
        ("certificate", "/") => cert_page::render(uri.query()),
        _ => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Cow::Borrowed(&[][..]))
                .expect("geçerli yanıt");
        }
    };
    Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .header(
            "Content-Security-Policy",
            start_page::CONTENT_SECURITY_POLICY,
        )
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(html.into_bytes()))
        .expect("geçerli yanıt")
}

impl Browser {
    fn handle(&mut self, event: UserEvent) {
        match event {
            // Butona tıklanınca odak butonda kalmasın, sayfaya dönsün.
            UserEvent::Back => {
                if is_problem_page(&self.current().url) {
                    self.page_back(self.active);
                } else {
                    let _ = self.view().go_back();
                }
                let _ = self.view().focus();
            }
            UserEvent::Forward => {
                let webview = self.view();
                let _ = webview.go_forward();
                let _ = webview.focus();
            }
            UserEvent::Reload => {
                // Hata sayfasında yenilemek, açılamayan sayfayı yeniden dener.
                let url = &self.current().url;
                if is_problem_page(url) {
                    let original = site_info::shown_url(url);
                    self.replace_page(self.active, &original);
                } else if self.view().url().map_or(true, |u| is_blank(&u)) && !is_blank(url) {
                    // Sekme açılışta henüz ilk sayfasını yüklemedi: WebView'de
                    // yalnızca boş sayfa var; yenilemek onu yeniler ve asıl
                    // yüklemeyi iptal eder. Asıl adresi yeniden aç.
                    let url = url.clone();
                    let _ = self.view().load_url(&url);
                } else {
                    let _ = self.view().reload();
                }
                let _ = self.view().focus();
            }
            UserEvent::FocusAddress => self.chrome.borrow().focus_address(),
            UserEvent::DevTools => self.toggle_devtools(),
            UserEvent::DevToolsMessage(id, text) => self.devtools_message(id, &text),
            UserEvent::TabMenu(index) => self.open_tab_menu(index),
            UserEvent::MenuChoice(row) => {
                if let Some((id, commands)) = self.tab_menu.take()
                    && let Some(Some(command)) = commands.get(row)
                {
                    self.tab_command(id, *command);
                }
            }
            UserEvent::InspectElement(id) => self.inspect_element(id),
            UserEvent::InspectNode(id, node) => self.inspect_node(id, node),
            UserEvent::DevToolsReady(id) => {
                if self.pending_inspect.is_some_and(|(tab, _)| tab == id)
                    && let Some((_, node)) = self.pending_inspect.take()
                {
                    self.post_inspect(id, node);
                }
            }
            UserEvent::DevToolsResize(x, done) => self.resize_devtools(x, done),
            UserEvent::ReopenTab => {
                if let Some(url) = self.closed.pop() {
                    self.open_tab(&url);
                }
            }
            UserEvent::NextTab => self.activate((self.active + 1) % self.tabs.len()),
            UserEvent::PrevTab => {
                self.activate((self.active + self.tabs.len() - 1) % self.tabs.len())
            }
            UserEvent::LastTab => self.activate(self.tabs.len() - 1),
            UserEvent::Navigate(text) => self.navigate(self.active, &text),
            UserEvent::AddressInput(text, complete) => {
                self.address_query = Some((text, complete));
                self.refresh_address_suggestions();
            }
            UserEvent::SearchSuggestions(engine, query, list) => {
                self.remote_arrived(engine, query, list)
            }
            UserEvent::FocusPage => {
                self.show_address(&self.current().url);
                let _ = self.view().focus();
            }
            UserEvent::NewPrivateWindow => open_private_window(),
            UserEvent::CertSeen(id, chain) => {
                self.certs.insert(id, chain);
            }
            UserEvent::Redirect(id, url) => {
                if let Some(index) = self.index_of(id) {
                    let tab = &mut self.tabs[index];
                    if is_problem_page(&url) {
                        // Açılamayan sayfa geçmişe girdiyse (sekmenin adresi
                        // ona geçtiyse) geri dönerken o da atlanır.
                        let failed = site_info::shown_url(&url);
                        tab.error_back = if site_info::same_page(&tab.url, &failed) {
                            2
                        } else {
                            1
                        };
                    }
                    if let Some(webview) = &tab.webview {
                        let _ = webview.load_url(&url);
                    }
                }
            }
            UserEvent::PermissionAsked(id) => {
                if self.current().id == id {
                    self.show_ask();
                }
            }
            UserEvent::AskAnswer(target) => self.answer_ask(target),
            UserEvent::SiteInfo => self.open_site_panel(),
            UserEvent::SitePermissions(id, origin, set) => {
                if self.current().id == id {
                    self.site = Some((id, origin, set));
                    self.show_site_panel();
                }
            }
            UserEvent::SitePermission(layout::SITE_CERT_ROW) => {
                self.chrome.borrow_mut().hide_site();
                self.view_site_certificate();
            }
            UserEvent::SitePermission(row) => self.cycle_permission(row),
            UserEvent::OpenCertificate(chain) => {
                if !chain.is_empty() {
                    self.open_tab(&cert_page::url(&chain));
                }
            }
            UserEvent::Find => self.open_find(),
            UserEvent::FindText(text) => self.find_text(&text),
            UserEvent::FindNext => self.find_step(true),
            UserEvent::FindPrev => self.find_step(false),
            UserEvent::FindClose => self.close_find(true),
            UserEvent::FindCount(id, active, total) => {
                if self.tabs.get(self.active).is_some_and(|t| t.id == id) {
                    self.chrome.borrow_mut().set_find_count(active, total);
                }
            }
            UserEvent::NewTab => {
                self.open_tab(START_URL);
                self.chrome.borrow().focus_address();
            }
            UserEvent::StartPage(id, message) => match message {
                Message::Go(text) => {
                    if let Some(index) = self.index_of(id) {
                        self.navigate(index, &text);
                    }
                }
                Message::SetEngine(engine) => self.set_engine(engine),
                Message::SetSmartScreen(on) => self.set_smartscreen(on),
                Message::SetHardwareAcceleration(on) => {
                    self.settings.gpu_off = !on;
                    self.start_state.lock().unwrap().gpu = on;
                    self.save_settings();
                }
                Message::SetCloseWithLastTab(on) => {
                    self.settings.close_with_last_tab = on;
                    self.start_state.lock().unwrap().close_with_last_tab = on;
                    self.save_settings();
                }
                Message::SetSearchSuggestions(on) => {
                    self.settings.search_suggestions_off = !on;
                    self.start_state.lock().unwrap().search_suggestions = on;
                    self.save_settings();
                }
                Message::SetAutocomplete(on) => {
                    self.settings.autocomplete_off = !on;
                    self.start_state.lock().unwrap().autocomplete = on;
                    self.save_settings();
                }
                Message::OpenPrivateWindow => open_private_window(),
                Message::Suggest(text) => {
                    self.page_query = Some((id, text));
                    self.refresh_page_suggestions();
                }
                Message::RemoveBookmark(url) => {
                    self.with_places(|p| p.remove_bookmark(&url));
                    self.reload_bookmarks();
                    self.update_star();
                }
                Message::DeleteHistory(url) => {
                    self.with_places(|p| p.delete_from_history(&url));
                }
                Message::ClearHistory => {
                    self.with_places(|p| p.clear_history());
                }
                Message::Appearance(key, value) => self.change_appearance(&key, &value),
                Message::Privacy(key, value) => self.change_privacy(&key, &value),
                Message::SetTabSleep(after) => {
                    self.settings.tab_sleep = after;
                    self.sleep_timing = sleep::Timing::for_setting(after);
                    self.start_state.lock().unwrap().tab_sleep = after;
                    self.save_settings();
                }
                Message::AllowHttp(url) => {
                    self.guard.lock().unwrap().allow_http(&url);
                    if let Some(index) = self.index_of(id) {
                        self.navigate(index, &url);
                    }
                }
                Message::PageBack => {
                    if let Some(index) = self.index_of(id) {
                        self.page_back(index);
                    }
                }
                Message::Retry(url) => {
                    if let Some(index) = self.index_of(id) {
                        self.replace_page(index, &url);
                    }
                }
                Message::AllowCert(url) => {
                    self.guard.lock().unwrap().allow_cert(&url);
                    if let Some(index) = self.index_of(id) {
                        self.replace_page(index, &url);
                    }
                }
                Message::Download(action, download) => self.download_action(action, download),
                Message::ClearDownloads => {
                    let mut list = self.downloads.lock().unwrap();
                    list.clear_finished();
                    self.download_ops
                        .borrow_mut()
                        .retain(|d, _| list.get(*d).is_some());
                    drop(list);
                    self.show_downloads(true);
                }
                Message::ViewCert => {
                    if let Some(chain) = self.certs.get(&id) {
                        let url = cert_page::url(chain);
                        self.open_tab(&url);
                    }
                }
            },
            UserEvent::ToggleBookmark => self.toggle_bookmark(),
            UserEvent::OpenHistory => self.open_internal("history", HISTORY_URL),
            UserEvent::OpenDownloads => {
                self.downloads.lock().unwrap().mark_seen();
                self.open_internal("downloads", DOWNLOADS_URL);
                self.sync_chrome();
            }
            UserEvent::DownloadsPanel => {
                if self.chrome.borrow().downloads_visible() {
                    self.chrome.borrow_mut().hide_downloads();
                } else {
                    self.downloads.lock().unwrap().mark_seen();
                    let rows = self.download_rows();
                    self.chrome.borrow_mut().show_downloads(rows);
                    self.sync_chrome();
                }
            }
            UserEvent::DownloadsPanelClick(code, double) => self.download_panel_click(code, double),
            UserEvent::DownloadsChanged(now) => self.show_downloads(now),
            UserEvent::DownloadStarted(id) => self.after_download_started(id),
            UserEvent::OpenSettings => self.open_internal("settings", SETTINGS_URL),
            UserEvent::OpenBookmark(i) => {
                if let Some(url) = self.bookmarks.get(i).map(|b| b.url.clone()) {
                    let _ = self.view().load_url(&url);
                    let _ = self.view().focus();
                }
            }
            UserEvent::CloseTabAt(i) => {
                if i < self.tabs.len() {
                    self.close_tab(i);
                }
            }
            UserEvent::SystemThemeChanged => {
                let changed = self.chrome.borrow_mut().refresh_system();
                if changed {
                    let accent = self.chrome.borrow().system_accent();
                    self.start_state.lock().unwrap().system_accent = accent;
                    self.after_theme_change();
                }
            }
            UserEvent::FaviconChanged(id, png) => {
                let url = self.index_of(id).and_then(|i| {
                    let tab = &self.tabs[i];
                    tab.webview
                        .as_ref()
                        .and_then(|w| w.url().ok())
                        .or(Some(tab.url.clone()))
                });
                if let Some(key) = url.as_deref().and_then(favicon_key) {
                    if !self.private {
                        self.with_places(|p| p.set_favicon(&key, &png, places::now()));
                    }
                    self.favicons_loaded.borrow_mut().insert(key.clone());
                    self.chrome.borrow_mut().set_favicon(&key, png);
                }
            }
            UserEvent::ZoomChanged(id, zoom) => {
                if let Some(index) = self.index_of(id) {
                    self.tabs[index].zoom = zoom;
                    if index == self.active {
                        self.sync_chrome();
                    }
                }
            }
            UserEvent::ZoomReset => {
                if let Some(webview) = &self.current().webview {
                    unsafe {
                        let _ = webview.controller().SetZoomFactor(1.0);
                    }
                    let _ = webview.focus();
                }
            }
            UserEvent::AudioChanged(id, playing) => {
                if let Some(index) = self.index_of(id) {
                    self.tabs[index].audio = playing;
                    self.sync_chrome();
                }
            }
            UserEvent::OpenInNewTab(url) => self.open_tab(&url),
            UserEvent::CloseTab => self.close_tab(self.active),
            UserEvent::SelectTab(index) => self.activate(index),
            UserEvent::MoveTab(from, to) => self.move_tab(from, to),
            UserEvent::TitleChanged(id, title) => {
                if let Some(index) = self.index_of(id) {
                    if index == self.active {
                        self.set_window_title(&title);
                    }
                    let url = self.tabs[index].url.clone();
                    if places::is_recordable(&url) && !title.is_empty() && !self.private {
                        self.with_places(|p| p.set_title(&url, &title));
                    }
                    self.tabs[index].title = title;
                    self.session_dirty = true;
                    self.sync_chrome();
                }
            }
            UserEvent::BlockedCountChanged(id) => {
                if self.index_of(id) == Some(self.active) {
                    self.update_shield();
                }
            }
            UserEvent::ToggleBlocking => self.toggle_blocking(),
            UserEvent::FiltersReady(bytes) => {
                match Blocker::deserialize(&bytes) {
                    Some(blocker) => {
                        *self.filter.blocker.borrow_mut() = Some(blocker);
                        self.filter.bump();
                    }
                    None => self.filters_failed("derlenen kurallar okunamadı".into()),
                }
                self.update_shield();
            }
            UserEvent::FiltersFailed(err) => self.filters_failed(err),
            UserEvent::SaveSession => {
                self.save_pending = false;
                if self.session_dirty {
                    self.save_session();
                }
            }
            UserEvent::Tick => {
                self.put_tabs_to_sleep();
                self.sync_chrome();
                if self.session_dirty {
                    self.save_session();
                }
            }
            UserEvent::Cosmetic(id, url, message) => self.apply_cosmetics(id, &url, message),
            // Yeni WebView ilk sayfası yüklenmeden önce boş sayfa bildirebilir;
            // sekmenin (oturumdan gelen) asıl adresi bununla silinmesin.
            UserEvent::UrlChanged(id, url)
                if is_blank(&url)
                    && self
                        .index_of(id)
                        .is_some_and(|i| !is_blank(&self.tabs[i].url)) => {}
            UserEvent::UrlChanged(id, url) => {
                if let Some(index) = self.index_of(id) {
                    if index == self.active {
                        self.show_address(&url);
                        if self.tabs[index].url != url {
                            self.chrome.borrow_mut().hide_site();
                        }
                    }
                    // Başka siteye geçildi: eski sayfanın soruları düşer.
                    let page = site_info::origin_of(&url).unwrap_or_default();
                    drop_asks(&self.asks, id, Some(&page));
                    if index == self.active {
                        self.show_ask();
                    }
                    if places::is_recordable(&url)
                        && self.tabs[index].recorded_url != url
                        && !self.private
                    {
                        let time = places::now();
                        self.with_places(|p| p.record_visit(&url, time));
                        self.tabs[index].recorded_url = url.clone();
                    }
                    self.tabs[index].url = url;
                    self.session_dirty = true;
                    if index == self.active {
                        self.update_navigation();
                        self.update_star();
                    }
                }
            }
        }
    }

    /// Adres çubuğu metnini çözümleyip sekmede açar.
    fn navigate(&self, index: usize, text: &str) {
        let search = self.settings.engine_or_fallback().search_prefix();
        let url = address::resolve(text, search);
        if index == self.active {
            self.show_address(&url);
        }
        if let Some(webview) = &self.tabs[index].webview {
            let _ = webview.load_url(&url);
            let _ = webview.focus();
        }
    }

    fn set_engine(&mut self, engine: Engine) {
        self.settings.engine = Some(engine);
        self.start_state.lock().unwrap().engine = Some(engine);
        self.save_settings();
    }

    fn set_smartscreen(&mut self, on: bool) {
        self.settings.smartscreen_off = !on;
        self.start_state.lock().unwrap().smartscreen = on;
        // Açık sekmelere hemen uygula; yeniden başlatma gerekmez.
        for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            set_reputation_checking(webview, on);
        }
        self.save_settings();
    }

    fn change_privacy(&mut self, key: &str, value: &str) {
        if !self.settings.privacy.set(key, value) {
            return;
        }
        let p = self.settings.privacy;
        self.start_state.lock().unwrap().privacy = p;
        {
            let mut guard = self.guard.lock().unwrap();
            guard.https_first = !p.https_first_off;
            guard.strip_tracking = !p.strip_tracking_off;
        }
        if key == "tracking"
            && let Some(webview) = self.tabs.iter().find_map(|t| t.webview.as_ref())
        {
            set_tracking_prevention(webview, p.tracking);
        }
        if key == "timezone" {
            for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
                set_timezone(webview, p.hide_timezone, None);
            }
        }
        self.save_settings();
    }

    /// "Kapanınca sil" açıksa site verilerini temizler (gizli pencerede
    /// veriler zaten bellekte, gerek yok).
    pub fn clear_on_close(&self) {
        if self.private || !self.settings.privacy.clear_on_close {
            return;
        }
        if let Some(webview) = self.tabs.iter().find_map(|t| t.webview.as_ref())
            && let Err(err) = clear_site_data(webview)
        {
            crate::log::error(&format!("site verileri silinemedi: {err}"));
        }
    }

    fn save_settings(&self) {
        if let Some(path) = &self.settings_path
            && let Err(err) = self.settings.save(path)
        {
            crate::log::error(&format!("ayarlar kaydedilemedi: {err}"));
        }
    }

    /// Geçerli sitede engelleyiciyi açar/kapatır ve sayfayı yeniler.
    fn toggle_blocking(&mut self) {
        let page_url = self.current().filter.page_url.borrow().clone();
        if address::is_start_page(&page_url) {
            return;
        }
        let Some(host) = address::host_of(&page_url) else {
            return;
        };
        let list = &mut self.settings.blocking_disabled;
        // Alt alan adında açarken üst alan adı kaydını da kaldır.
        if self.filter.disabled_for(&page_url) {
            list.retain(|entry| !settings::covers(entry, &host));
        } else {
            list.insert(host);
        }
        *self.filter.disabled.borrow_mut() = list.clone();
        self.filter.bump();
        self.save_settings();
        self.recreate_current_tab();
    }

    /// Öndeki sekmenin WebView'ini kapatıp aynı adreste sıfırdan açar.
    ///
    /// Engelleyici açılıp kapatıldığında yenilemek yetmez: sayfa yenilenirken
    /// WebView2 önceden kaydedilmiş betikleri (scriptlet) hâlâ çalıştırabilir.
    /// Yeni WebView'de eski betik olamaz. Bedeli: sekmenin geri/ileri geçmişi
    /// sıfırlanır.
    fn recreate_current_tab(&mut self) {
        let index = self.active;
        if let Some(url) = self.tabs[index].webview.as_ref().and_then(|w| w.url().ok()) {
            self.tabs[index].url = url;
        }
        // WebView'i bırakmak WebView2 sayfasını kapatır; `activate` uyuyan
        // sekmeyi uyandırır gibi yeniden oluşturur.
        drop_asks(&self.asks, self.tabs[index].id, None);
        // Araçlar eski sayfaya bağlı; sayfayla birlikte kapanır.
        self.tabs[index].devtools = None;
        self.tabs[index].webview = None;
        self.tabs[index].power = Power::Discarded;
        self.activate(index);
        self.layout();
    }

    /// Geçmiş/yer imi veritabanında bir işlem yapar; hata olursa günlüğe yazar.
    fn with_places<T>(&self, f: impl FnOnce(&Places) -> rusqlite::Result<T>) -> Option<T> {
        let places = self.places.as_ref()?.lock().ok()?;
        f(&places)
            .map_err(|err| crate::log::error(&format!("geçmiş veritabanı: {err}")))
            .ok()
    }

    /// Öndeki sekmenin en güncel adresi.
    fn current_url(&self) -> String {
        self.view()
            .url()
            .unwrap_or_else(|_| self.current().url.clone())
    }

    /// Öndeki sayfayı yer imlerine ekler ya da çıkarır (Ctrl+D, yıldız).
    fn toggle_bookmark(&mut self) {
        let url = self.current_url();
        if !places::is_recordable(&url) {
            return;
        }
        let title = self.current().title.clone();
        let bookmarked = self.with_places(|p| p.is_bookmarked(&url)).unwrap_or(false);
        if bookmarked {
            self.with_places(|p| p.remove_bookmark(&url));
        } else {
            self.with_places(|p| p.add_bookmark(&url, &title, places::now()));
        }
        self.reload_bookmarks();
        self.update_star();
    }

    /// Yıldız butonunu öndeki sayfaya göre günceller.
    fn update_star(&mut self) {
        let url = self.current_url();
        let enabled = self.places.is_some() && places::is_recordable(&url);
        let bookmarked = enabled && self.with_places(|p| p.is_bookmarked(&url)).unwrap_or(false);
        self.star = (bookmarked, enabled);
        self.sync_chrome();
    }

    /// Tarayıcının iç sayfası (geçmiş, ayarlar) açıksa ona geçer ve
    /// yeniler; değilse yeni sekmede açar.
    fn open_internal(&mut self, name: &str, url: &str) {
        let existing = self
            .tabs
            .iter()
            .position(|t| address::internal_page(&t.url) == Some(name));
        match existing {
            Some(index) => {
                self.activate(index);
                let _ = self.view().reload();
            }
            None => self.open_tab(url),
        }
    }

    /// Yer imlerini veritabanından yeniden okur (kenar çubuğu için).
    fn reload_bookmarks(&mut self) {
        self.bookmarks = self
            .with_places(|p| p.bookmarks())
            .unwrap_or_default()
            .into_iter()
            .map(|e| BookmarkInfo {
                icon: icon_key(&e.url),
                title: e.title,
                url: e.url,
            })
            .collect();
        self.sync_chrome();
    }

    /// Ayarlar sayfasından gelen görünüm değişikliği.
    fn change_appearance(&mut self, key: &str, value: &str) {
        let mut a = self.settings.appearance;
        if !a.set(key, value) || a == self.settings.appearance {
            return;
        }
        self.settings.appearance = a;
        self.save_settings();
        self.start_state.lock().unwrap().appearance = a;
        self.chrome
            .borrow_mut()
            .set_appearance(shown_appearance(a, self.private));
        self.after_theme_change();
    }

    /// Tema değişince sayfaların yeri ve renk düzeni de değişebilir.
    fn after_theme_change(&mut self) {
        self.layout();
        let theme = self.web_theme();
        for webview in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            let _ = webview.set_theme(theme);
        }
        // Açık geliştirici araçları da Julie'nin yeni renklerini alsın.
        let message = HSTRING::from(devtools::theme_message(self.chrome.borrow().theme()));
        for tools in self.tabs.iter().filter_map(|t| t.devtools.as_ref()) {
            let _ = tools.webview.set_theme(theme);
            unsafe {
                let _ = tools.webview.webview().PostWebMessageAsString(&message);
            }
        }
    }

    /// Sayfaların (iç sayfalar ve sitelerin `prefers-color-scheme` sorgusu)
    /// kullanacağı renk düzeni.
    fn web_theme(&self) -> wry::Theme {
        if self.chrome.borrow().theme().dark {
            wry::Theme::Dark
        } else {
            wry::Theme::Light
        }
    }

    /// Arayüzün içeriğini (sekmeler, butonlar, rozetler) günceller.
    fn sync_chrome(&self) {
        // Henüz sekme yokken (açılışta) çizilecek bir şey yok.
        if self.tabs.is_empty() {
            return;
        }
        let asleep = |t: &Tab| t.webview.is_none() || t.power == Power::Suspended;
        let model = Model {
            tabs: self
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| TabInfo {
                    icon: icon_key(&t.url),
                    title: if address::is_start_page(&t.url) && t.title.is_empty() {
                        "Yeni sekme".to_string()
                    } else {
                        t.title.clone()
                    },
                    active: i == self.active,
                    audio: t.audio,
                    muted: t.muted,
                    sleeping: i != self.active && asleep(t),
                })
                .collect(),
            bookmarks: self.bookmarks.clone(),
            can_back: self.can_back,
            can_forward: self.can_forward,
            bookmarked: self.star.0,
            star_enabled: self.star.1,
            shield: self.shield.0.clone(),
            shield_off: self.shield.1,
            tab_scroll: 0.0,
            tab_shift: Vec::new(),
            security: {
                let url = &self.current().url;
                site_info::security_of(url, self.guard.lock().unwrap().cert_bypassed(url))
            },
            downloads: self.downloads.lock().unwrap().badge(),
            zoom: {
                let percent = (self.current().zoom * 100.0).round() as u16;
                (percent != 100).then_some(percent)
            },
        };
        self.load_favicons(&model);
        self.chrome.borrow_mut().set_model(model);
    }

    /// Arayüzde görünen sitelerin ikonlarını veritabanından bir kez yükler.
    fn load_favicons(&self, model: &Model) {
        let keys = model
            .tabs
            .iter()
            .filter_map(|t| t.icon.as_ref())
            .chain(model.bookmarks.iter().filter_map(|b| b.icon.as_ref()));
        self.ensure_favicons(keys);
    }

    /// Verilen sitelerin ikonlarını (henüz yüklenmediyse) arayüze verir.
    fn ensure_favicons<'a>(&self, keys: impl Iterator<Item = &'a String>) {
        for key in keys {
            if !self.favicons_loaded.borrow_mut().insert(key.clone()) {
                continue;
            }
            if key == JULIE_ICON_KEY {
                self.chrome
                    .borrow_mut()
                    .set_favicon(key, JULIE_ICON_PNG.to_vec());
                continue;
            }
            if let Some(png) = self.with_places(|p| p.favicon(key)).flatten() {
                self.chrome.borrow_mut().set_favicon(key, png);
            }
        }
    }

    /// Yazılan metin için öneri satırları ve (istenirse) otomatik tamamlama.
    fn suggestion_items(
        &mut self,
        text: &str,
        can_complete: bool,
    ) -> (Vec<suggest::Item>, Option<String>) {
        let engine = self.settings.engine_or_fallback();
        // Yazılan adresle aynı sayfa listeden düşebileceği için bir fazla iste.
        let matches = self
            .with_places(|p| p.suggest(text, suggest::MAX_MATCHES + 1, places::now()))
            .unwrap_or_default();
        let completed = if can_complete && !self.settings.autocomplete_off {
            suggest::completion(text, &matches)
        } else {
            None
        };
        let remote = self.remote_suggestions(engine, text);
        let items = suggest::build(text, completed.as_deref(), engine, matches, &remote);
        (items, completed)
    }

    /// Arama motorunun önbellekteki önerileri. Yoksa istenir ve gelene
    /// kadar bir harf eksiğinin önerileri gösterilir (liste titremesin).
    fn remote_suggestions(&mut self, engine: Engine, text: &str) -> Vec<String> {
        // Gizli pencerede yazılanlar arama motoruna gönderilmez (Firefox gibi).
        if self.settings.search_suggestions_off
            || self.private
            || !suggest::may_ask_engine(text, engine)
        {
            return Vec::new();
        }
        let query = text.trim().to_lowercase();
        if let Some(list) = self.remote_cache.get(&(engine, query.clone())) {
            return list.clone();
        }
        let proxy = &self.proxy;
        let tx = self
            .remote_tx
            .get_or_insert_with(|| crate::search_suggest::spawn(proxy.clone()));
        let _ = tx.send((engine, text.trim().to_string()));
        // Gelene kadar daha kısa bir metnin önerilerinden yazılanla
        // başlayanlar gösterilir ("hav" → "hava durumu"; "haberler" değil).
        let mut shorter = query.as_str();
        while let Some((cut, _)) = shorter.char_indices().last() {
            shorter = &shorter[..cut];
            if let Some(list) = self.remote_cache.get(&(engine, shorter.to_string())) {
                return list
                    .iter()
                    .filter(|p| p.to_lowercase().starts_with(&query))
                    .cloned()
                    .collect();
            }
        }
        Vec::new()
    }

    /// Arama motorunun önerileri geldi: hâlâ aynı metin yazılıysa listeyi yenile.
    fn remote_arrived(&mut self, engine: Engine, query: String, list: Vec<String>) {
        if self.remote_cache.len() > 200 {
            self.remote_cache.clear();
        }
        let key = query.trim().to_lowercase();
        self.remote_cache.insert((engine, key.clone()), list);
        if engine != self.settings.engine_or_fallback() {
            return;
        }
        let same = |text: &str| text.trim().to_lowercase() == key;
        if self
            .address_query
            .as_ref()
            .is_some_and(|(text, _)| same(text))
            && self.chrome.borrow().suggestions_visible()
        {
            self.refresh_address_suggestions();
        }
        if self.page_query.as_ref().is_some_and(|(_, text)| same(text)) {
            self.refresh_page_suggestions();
        }
    }

    /// Adres çubuğunun öneri listesini (ve otomatik tamamlamayı) günceller.
    fn refresh_address_suggestions(&mut self) {
        let Some((text, complete)) = self.address_query.clone() else {
            return;
        };
        if text.trim().is_empty() {
            self.chrome.borrow().hide_suggestions();
            return;
        }
        let (items, completed) = self.suggestion_items(&text, complete);
        if let Some(completed) = &completed {
            self.chrome.borrow().complete_address(&text, completed);
        }
        self.ensure_favicons(items.iter().filter_map(|i| i.icon.as_ref()));
        let (rows, targets) = items
            .into_iter()
            .map(|i| {
                let row = SuggestionRow {
                    search: i.kind == suggest::Kind::Search,
                    icon: i.icon,
                    title: i.title,
                    detail: i.detail,
                };
                (row, i.target)
            })
            .unzip();
        self.chrome.borrow_mut().show_suggestions(rows, targets);
    }

    /// Başlangıç sayfasının arama kutusuna önerileri gönderir; sayfa
    /// listeyi kutunun altında kendisi çizer.
    fn refresh_page_suggestions(&mut self) {
        use base64::Engine as _;
        let Some((id, text)) = self.page_query.clone() else {
            return;
        };
        let Some(index) = self.index_of(id) else {
            return;
        };
        let (items, _) = self.suggestion_items(&text, false);
        let rows: Vec<_> = items
            .iter()
            .map(|i| {
                let icon = i
                    .icon
                    .as_ref()
                    .and_then(|key| self.with_places(|p| p.favicon(key)).flatten())
                    .map(|png| {
                        format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(png)
                        )
                    });
                serde_json::json!({
                    "search": i.kind == suggest::Kind::Search,
                    "title": i.title,
                    "detail": i.detail,
                    "target": i.target,
                    "icon": icon,
                })
            })
            .collect();
        let script = format!(
            "window.julieSuggestions && window.julieSuggestions({}, {})",
            serde_json::Value::from(text),
            serde_json::Value::from(rows)
        );
        if let Some(webview) = &self.tabs[index].webview {
            let _ = webview.evaluate_script(&script);
        }
    }

    /// Geri/ileri butonlarının durumunu öndeki sekmeden okur.
    fn update_navigation(&mut self) {
        let webview = self.view();
        let back = webview.can_go_back().unwrap_or(false);
        let forward = webview.can_go_forward().unwrap_or(false);
        self.can_back = back;
        self.can_forward = forward;
    }

    fn filters_failed(&mut self, err: String) {
        crate::log::error(&format!("filtre listeleri güncellenemedi: {err}"));
        self.filters_error = true;
        self.update_shield();
    }

    /// Sayfadaki betiğin isteğine göre gizlenecek reklam kutularını bulur
    /// ve sayfaya CSS olarak uygular.
    fn apply_cosmetics(&mut self, id: u64, url: &str, message: cosmetic::Message) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if address::is_start_page(url) || self.filter.disabled_for(url) {
            return;
        }
        let blocker = self.filter.blocker.borrow();
        let Some(blocker) = blocker.as_ref() else {
            return;
        };
        let tab = &mut self.tabs[index];
        let selectors = match message {
            cosmetic::Message::Start => {
                let page = blocker.page_cosmetics(url);
                tab.cosmetic_exceptions = page.exceptions;
                tab.generichide = page.generichide;
                page.hide_selectors
            }
            cosmetic::Message::Seen { classes, ids } => {
                if tab.generichide {
                    return;
                }
                let classes: Vec<&str> = classes.iter().map(String::as_str).collect();
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                blocker.generic_selectors(&classes, &ids, &tab.cosmetic_exceptions)
            }
        };
        let css = cosmetic::hide_css(&selectors);
        if let (false, Some(webview)) = (css.is_empty(), &tab.webview) {
            let _ = webview.evaluate_script(&cosmetic::apply_script(&css));
        }
    }

    /// Engelleyici rozetini geçerli sekmeye göre günceller.
    fn update_shield(&mut self) {
        let tab = self.current();
        let page_url = tab.filter.page_url.borrow().clone();
        let blocked = tab.filter.blocked.get();
        self.shield =
            if address::is_internal_page(&page_url) || address::host_of(&page_url).is_none() {
                ("0".to_string(), false)
            } else if self.filter.disabled_for(&page_url) {
                ("Kapalı".to_string(), true)
            } else if self.filter.blocker.borrow().is_none() {
                let text = if self.filters_error {
                    "Liste yok"
                } else {
                    "…"
                };
                (text.to_string(), true)
            } else {
                (blocked.to_string(), false)
            };
        self.sync_chrome();
    }

    /// Adres çubuğunu günceller; başlangıç sayfasında kutu boş kalır.
    fn show_address(&self, url: &str) {
        let shown = site_info::shown_url(url);
        self.chrome
            .borrow()
            .set_address(if address::is_start_page(url) {
                ""
            } else {
                &shown
            });
    }

    /// Hata ya da uyarı sayfasından geri dön: açılamayan sayfayı da atla;
    /// geride sayfa yoksa başlangıç sayfasına git.
    fn page_back(&self, index: usize) {
        let tab = &self.tabs[index];
        let Some(webview) = &tab.webview else {
            return;
        };
        if is_problem_page(&tab.url) {
            let n = tab.error_back.max(1);
            let start = serde_json::to_string(START_URL).unwrap_or_default();
            let _ = webview.evaluate_script(&format!(
                "history.length > {n} ? history.go(-{n}) : location.replace({start})"
            ));
        } else if webview.can_go_back().unwrap_or(false) {
            let _ = webview.go_back();
        } else {
            let _ = webview.load_url(START_URL);
        }
    }

    /// Hata sayfasının yerine (geçmişe yeni kayıt eklemeden) `url`'yi aç.
    fn replace_page(&self, index: usize, url: &str) {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return;
        }
        if let (Some(webview), Ok(json)) = (&self.tabs[index].webview, serde_json::to_string(url)) {
            let _ = webview.evaluate_script(&format!("location.replace({json})"));
        }
    }

    /// Öndeki sekmenin ilk bekleyen sorusunu gösterir; yoksa soruyu kapatır.
    fn show_ask(&mut self) {
        let id = self.current().id;
        let info = self.asks.borrow().iter().find(|a| a.tab == id).map(|a| {
            let (text, icon) =
                site_info::ask_text(a.kind).unwrap_or(("", crate::ui::scene::Icon::Globe));
            layout::AskInfo {
                host: address::exact_host(&a.origin).unwrap_or_else(|| a.origin.clone()),
                text: text.to_string(),
                icon,
            }
        });
        let mut chrome = self.chrome.borrow_mut();
        match info {
            Some(info) => chrome.show_ask(info),
            None => chrome.hide_ask(),
        }
    }

    /// İzin sorusu yanıtlandı. Aynı sitenin aynı izin için bekleyen diğer
    /// istekleri de aynı yanıtı alır.
    fn answer_ask(&mut self, target: AskTarget) {
        let id = self.current().id;
        let answered: Vec<PendingAsk> = {
            let mut list = self.asks.borrow_mut();
            let Some(first) = list.iter().position(|a| a.tab == id) else {
                drop(list);
                self.show_ask();
                return;
            };
            let (origin, kind) = (list[first].origin.clone(), list[first].kind);
            let (same, rest) = std::mem::take(&mut *list)
                .into_iter()
                .partition(|a| a.tab == id && a.kind == kind && a.origin == origin);
            *list = rest;
            same
        };
        let (allow, save) = match target {
            AskTarget::Allow => (true, true),
            AskTarget::Block => (false, true),
            AskTarget::Close => (false, false),
        };
        for ask in answered {
            ask.finish(allow, save);
        }
        self.show_ask();
        let _ = self.view().focus();
    }

    /// Kilide tıklandı: sitenin izinlerini sor, gelince paneli aç.
    fn open_site_panel(&mut self) {
        let tab = self.current();
        let Some(origin) = site_info::origin_of(&tab.url) else {
            // Tarayıcının kendi sayfası: izin yok, panel hemen açılır.
            self.site = None;
            self.show_site_panel();
            return;
        };
        let id = tab.id;
        let proxy = self.proxy.clone();
        if let Err(err) = request_permissions(self.view(), proxy, id, origin.clone()) {
            // Eski çalışma zamanı: izinler okunamıyor; paneli izinsiz aç.
            crate::log::error(&format!("izinler okunamadı: {err}"));
            self.site = None;
            self.show_site_panel();
        }
    }

    fn site_panel_info(&self) -> layout::SiteInfo {
        let url = &self.current().url;
        let shown = site_info::shown_url(url);
        let host = if address::is_internal_page(&shown) {
            "Julie".to_string()
        } else {
            address::exact_host(&shown).unwrap_or_else(|| shown.clone())
        };
        let permissions = match &self.site {
            Some((id, _, set)) if *id == self.current().id => site_info::permission_rows(set)
                .into_iter()
                .map(|(_, name, state)| (name.to_string(), state))
                .collect(),
            _ => Vec::new(),
        };
        let security = site_info::security_of(url, self.guard.lock().unwrap().cert_bypassed(url));
        layout::SiteInfo {
            host,
            certificate: matches!(
                security,
                layout::Security::Secure | layout::Security::Danger
            ),
            security,
            permissions,
        }
    }

    fn show_site_panel(&mut self) {
        let info = self.site_panel_info();
        let mut chrome = self.chrome.borrow_mut();
        if chrome.site_visible() {
            chrome.update_site(info);
        } else {
            chrome.show_site(info);
        }
    }

    /// Kilit panelinde "Sertifikayı görüntüle": öndeki sitenin sertifika
    /// zincirini yeni sekmede göster. Hata sayfasındaysa (ya da sertifika
    /// hatasına rağmen girildiyse) yakalanan zincir; değilse motordan istenir.
    fn view_site_certificate(&mut self) {
        let tab = self.current();
        let id = tab.id;
        let shown = site_info::shown_url(&tab.url);
        if let Some(chain) = self.certs.get(&id).filter(|_| {
            site_info::security_of(&tab.url, self.guard.lock().unwrap().cert_bypassed(&tab.url))
                == layout::Security::Danger
        }) {
            let url = cert_page::url(chain);
            self.open_tab(&url);
            return;
        }
        let Some(origin) = site_info::origin_of(&shown) else {
            return;
        };
        request_certificate(self.view(), self.proxy.clone(), &origin);
    }

    /// Paneldeki izin satırına tıklandı: Sor → İzin ver → Engelle → Sor.
    fn cycle_permission(&mut self, row: usize) {
        let current = self.current().id;
        let Some((id, origin, set)) = &mut self.site else {
            return;
        };
        if *id != current {
            return;
        }
        let Some(&(kind, _, state)) = site_info::permission_rows(set).get(row) else {
            return;
        };
        let raw = site_info::raw_state(site_info::next_state(state));
        let origin = origin.clone();
        set.retain(|(k, _)| *k != kind);
        if raw != site_info::STATE_DEFAULT {
            set.push((kind, raw));
        }
        if let Err(err) = set_permission(self.view(), &origin, kind, raw) {
            crate::log::error(&format!("izin değiştirilemedi: {err}"));
        }
        self.show_site_panel();
    }

    fn current(&self) -> &Tab {
        &self.tabs[self.active]
    }

    /// Öndeki sekmenin WebView'i. Öndeki sekme hiçbir zaman uyumaz.
    fn view(&self) -> &WebView {
        self.current()
            .webview
            .as_ref()
            .expect("öndeki sekme uyanık olmalı")
    }

    /// Uzun süredir arka planda olan sekmeleri dondurur ya da bellekten atar.
    fn put_tabs_to_sleep(&mut self) {
        let Some(timing) = self.sleep_timing else {
            return;
        };
        let now = Instant::now();
        let downloading = self.downloading_tabs();
        let asks = self.asks.borrow();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == self.active {
                continue;
            }
            let (Some(webview), Some(since)) = (&tab.webview, tab.hidden_since) else {
                continue;
            };
            // Yanıt bekleyen sorusu olan ya da araçları açık sekme uyumaz.
            if tab.devtools.is_some()
                || asks.iter().any(|a| a.tab == tab.id)
                || downloading.contains(&tab.id)
            {
                continue;
            }
            let action = sleep::decide(
                &timing,
                tab.power,
                now - since,
                is_playing_audio(webview),
                address::is_start_page(&tab.url),
            );
            match action {
                Some(Action::Suspend) => {
                    if try_suspend(webview) {
                        tab.power = Power::Suspended;
                    }
                }
                Some(Action::Discard) => {
                    // Tek sayfalık uygulamalar adresi sayfa yüklenmeden de
                    // değiştirebilir; en güncel adresi WebView'den al.
                    if let Ok(url) = webview.url() {
                        tab.url = url;
                    }
                    // WebView'in bırakılması WebView2 sayfasını ve belleğini serbest bırakır.
                    tab.webview = None;
                    tab.power = Power::Discarded;
                }
                None => {}
            }
        }
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Sayfa alanı (fiziksel piksel): arayüzün sayfaya ayırdığı bölge.
    fn content_bounds(&self) -> Rect {
        let (x, y, w, h) = self.chrome.borrow().content_px();
        Rect {
            position: PhysicalPosition::new(x, y).into(),
            size: PhysicalSize::new(w, h).into(),
        }
    }

    /// Sayfayı arayüzün çerçevesine yerleştirir ve köşelerini yuvarlar.
    fn place_webview(&self, webview: &WebView) {
        let (x, y, w, h) = self.chrome.borrow().content_px();
        place_rounded(
            webview,
            (x, y, w, h),
            self.chrome.borrow().content_radius_px(),
        );
    }

    /// Araçları açık sekme: sayfa solda, araçlar sağda.
    fn place_split(&self, page: &WebView, tools: &WebView) {
        let (x, y, w, h) = self.chrome.borrow().content_px();
        let r = self.chrome.borrow().content_radius_px();
        let (page_w, tools_x, tools_w) = self.split(x, w);
        place_rounded(page, (x, y, page_w, h), r);
        place_rounded(tools, (tools_x, y, tools_w, h), r);
    }

    fn layout(&self) {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        self.chrome
            .borrow_mut()
            .set_size(size.width, size.height, scale);
        // Gizli sekmeler de yeniden boyutlanır ki geçişte sayfa zıplamasın.
        for tab in &self.tabs {
            match (&tab.webview, &tab.devtools) {
                (Some(page), Some(tools)) => self.place_split(page, &tools.webview),
                (Some(page), None) => self.place_webview(page),
                _ => {}
            }
        }
        self.update_side_panel();
    }

    /// Bulma kutusu öndeki sekmenin araç panelinin üstüne binmesin.
    fn update_side_panel(&self) {
        let open = self
            .tabs
            .get(self.active)
            .is_some_and(|t| t.devtools.is_some());
        let (panel, gap) = if open {
            let (x, _, w, _) = self.chrome.borrow().content_px();
            let (page_w, _, panel) = self.split(x, w);
            (panel, w - page_w - panel)
        } else {
            (0, 0)
        };
        self.chrome.borrow_mut().set_side_panel(panel, gap);
    }

    /// Sayfa alanını sayfa ve araç paneli olarak böler (kullanıcının
    /// ayarladığı genişlikle).
    fn split(&self, x: i32, width: u32) -> (u32, i32, u32) {
        let share = self
            .settings
            .devtools_width
            .unwrap_or(devtools::DEFAULT_WIDTH);
        devtools::split(x, width, self.window.scale_factor() as f32, share)
    }

    /// Ayraç sürükleniyor: paneli imlecin yerine göre genişletir ya da
    /// daraltır. Bırakılınca genişlik ayarlara kaydedilir.
    fn resize_devtools(&mut self, mouse_x: i32, done: bool) {
        let (x, _, w, _) = self.chrome.borrow().content_px();
        let share = devtools::share_at(x, w, self.window.scale_factor() as f32, mouse_x);
        if self.settings.devtools_width != Some(share) {
            self.settings.devtools_width = Some(share);
            self.layout();
        }
        if done {
            self.save_settings();
        }
    }

    /// Süren bir indirme başlatmış sekmeler (bunlar uyutulmaz).
    fn downloading_tabs(&self) -> HashSet<u64> {
        let list = self.downloads.lock().unwrap();
        self.download_ops
            .borrow()
            .iter()
            .filter(|(id, _)| list.get(**id).is_some_and(|d| d.state.running()))
            .map(|(_, (owner, _))| *owner)
            .collect()
    }

    /// İndirme düğmesini ve açık indirmeler sayfalarını günceller. Yalnızca
    /// ilerleme değiştiyse saniyede birkaç kez.
    fn show_downloads(&mut self, now: bool) {
        if !now && self.downloads_shown.elapsed() < std::time::Duration::from_millis(250) {
            return;
        }
        self.downloads_shown = Instant::now();
        if self.chrome.borrow().downloads_visible() {
            let rows = self.download_rows();
            self.chrome.borrow_mut().update_downloads(rows);
        }
        if now && !self.parked.is_empty() {
            let running = self.downloading_tabs();
            self.parked.retain(|(id, _)| running.contains(id));
        }
        self.sync_chrome();
        let json = HSTRING::from(self.downloads.lock().unwrap().json().to_string());
        for tab in &self.tabs {
            if address::internal_page(&tab.url) == Some("downloads")
                && let Some(webview) = &tab.webview
            {
                unsafe {
                    let _ = webview.webview().PostWebMessageAsString(&json);
                }
            }
        }
    }

    /// İndirme bir sayfa açmaz: yalnızca indirme için açılmış boş sekme
    /// kapanır; sayfası olan sekmede adres çubuğu sayfanın adresine döner.
    fn after_download_started(&mut self, id: u64) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let Some(source) = self.tabs[index].webview.as_ref().and_then(|w| w.url().ok()) else {
            return;
        };
        if is_blank(&source) {
            if self.tabs.len() == 1 {
                // Tek sekme: pencere kapanmasın, yeni sekme sayfası açılsın.
                self.tabs[index].url = START_URL.to_string();
                if let Some(webview) = &self.tabs[index].webview {
                    let _ = webview.load_url(START_URL);
                }
                self.show_address(START_URL);
            } else {
                // İndirme adresi "kapatılan sekmeler"e girmesin: yeniden
                // açılınca dosya bir daha iner.
                self.tabs[index].url = START_URL.to_string();
                self.close_tab(index);
            }
            return;
        }
        self.tabs[index].url = source.clone();
        self.session_dirty = true;
        if index == self.active {
            self.show_address(&source);
            self.update_navigation();
            self.update_star();
        }
    }

    /// İndirme panelinin satırları; Windows simgeleri arayüze verilir.
    fn download_rows(&self) -> Vec<layout::DownloadRow> {
        let list = self.downloads.lock().unwrap();
        let rows = list.rows(5);
        let mut chrome = self.chrome.borrow_mut();
        for row in &rows {
            if let Some((key, png)) = list.get(row.id).and_then(|d| d.icon.as_ref()) {
                chrome.set_favicon(key, png.as_ref().clone());
            }
        }
        rows
    }

    /// İndirme panelinde tıklanan yer.
    fn download_panel_click(&mut self, code: usize, double: bool) {
        let click = layout::PanelClick::from_code(code);
        let state = |id| self.downloads.lock().unwrap().get(id).map(|d| d.state);
        match click {
            // Süren indirmeye çift tıklama: bitince aç.
            layout::PanelClick::Row(id) if double => {
                if state(id).is_some_and(DownloadState::running) {
                    self.download_action(DownloadAction::AutoOpen, id);
                }
            }
            layout::PanelClick::All => {
                self.chrome.borrow_mut().hide_downloads();
                self.handle(UserEvent::OpenDownloads);
            }
            layout::PanelClick::Row(id) => {
                if state(id) == Some(DownloadState::Done) {
                    self.chrome.borrow_mut().hide_downloads();
                    self.download_action(DownloadAction::Open, id);
                }
            }
            layout::PanelClick::Primary(id) => match state(id) {
                Some(DownloadState::Active) => self.download_action(DownloadAction::Pause, id),
                Some(DownloadState::Paused | DownloadState::Failed) => {
                    self.download_action(DownloadAction::Resume, id)
                }
                Some(DownloadState::Done) => {
                    self.chrome.borrow_mut().hide_downloads();
                    self.download_action(DownloadAction::Show, id);
                }
                _ => {}
            },
            layout::PanelClick::Cancel(id) => self.download_action(DownloadAction::Cancel, id),
        }
    }

    /// İndirmeler sayfasındaki düğmeler.
    fn download_action(&mut self, action: DownloadAction, id: u64) {
        let op = self
            .download_ops
            .borrow()
            .get(&id)
            .map(|(_, op)| op.clone());
        let done_path = self
            .downloads
            .lock()
            .unwrap()
            .get(id)
            .filter(|d| d.state == DownloadState::Done)
            .map(|d| d.path.clone());
        let result = unsafe {
            match (action, &op) {
                (DownloadAction::Open, _) => {
                    if let Some(path) = done_path {
                        open_file(&path);
                    }
                    Ok(())
                }
                (DownloadAction::Show, _) => {
                    if let Some(path) = done_path {
                        show_in_folder(&path);
                    }
                    Ok(())
                }
                (DownloadAction::AutoOpen, _) => {
                    self.downloads.lock().unwrap().toggle_open_when_done(id);
                    Ok(())
                }
                (DownloadAction::Pause, Some(op)) => op.Pause(),
                (DownloadAction::Resume, Some(op)) => op.Resume(),
                (DownloadAction::Cancel, Some(op)) => op.Cancel(),
                (DownloadAction::Remove, _) => {
                    let mut list = self.downloads.lock().unwrap();
                    list.remove(id);
                    if list.get(id).is_none() {
                        self.download_ops.borrow_mut().remove(&id);
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        };
        if let Err(err) = result {
            crate::log::error(&format!("indirme komutu çalışmadı: {err}"));
        }
        self.show_downloads(true);
    }

    /// F12: öndeki sekmenin geliştirici araçlarını açar ya da kapatır.
    fn toggle_devtools(&mut self) {
        let index = self.active;
        if let Some(tools) = self.tabs[index].devtools.take() {
            if let Some(page) = &self.tabs[index].webview {
                self.restore_after_devtools(page);
                let _ = page.focus();
            }
            drop(tools);
        } else {
            match self.open_devtools(index) {
                Ok(tools) => {
                    let _ = tools.webview.focus();
                    self.tabs[index].devtools = Some(tools);
                }
                Err(err) => crate::log::error(&format!("geliştirici araçları açılamadı: {err}")),
            }
        }
        self.layout();
    }

    /// Araçlar kapanınca sayfayı eski hâline getirir (durdurulmuş betik,
    /// cihaz taklidi vb.). Saat dilimi ayarı yeniden uygulanır.
    fn restore_after_devtools(&self, page: &WebView) {
        for (method, params) in devtools::CLEANUP {
            let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
            unsafe {
                let _ = page.webview().CallDevToolsProtocolMethod(
                    &HSTRING::from(*method),
                    &HSTRING::from(*params),
                    &done,
                );
            }
        }
        set_timezone(page, self.settings.privacy.hide_timezone, None);
    }

    fn open_devtools(&mut self, index: usize) -> wry::Result<DevTools> {
        let id = self.tabs[index].id;
        let page = self.tabs[index]
            .webview
            .as_ref()
            .expect("öndeki sekme uyanık olmalı")
            .webview();
        let ipc_proxy = self.proxy.clone();
        let window_proxy = self.proxy.clone();
        let bounds = self.content_bounds();
        let theme = self.web_theme();
        let theme_now = *self.chrome.borrow().theme();
        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(devtools::url(id))
            .with_bounds(bounds)
            .with_additional_browser_args(browser_args(self.gpu_off))
            .with_incognito(self.private)
            .with_devtools(false)
            .with_theme(theme)
            // Araçlar güvenli bağlam ister (ör. `crypto.randomUUID`).
            .with_https_scheme(true)
            .with_custom_protocol("julie".into(), |_, request| serve_devtools(request.uri()))
            .with_initialization_script(devtools::init_script(&theme_now))
            .with_ipc_handler(move |request| {
                if is_devtools_host(request.uri().host()) {
                    let text = request.body().clone();
                    let _ = ipc_proxy.send_event(UserEvent::DevToolsMessage(id, text));
                }
            })
            // Araçlardaki bağlantılar yeni sekmede açılır.
            .with_new_window_req_handler(move |url, _features| {
                let _ = window_proxy.send_event(UserEvent::OpenInNewTab(url));
                NewWindowResponse::Deny
            })
            .build_as_child(&self.window)?;
        listen_for_shortcuts(&webview, self.proxy.clone(), true);
        clip_siblings(&webview);
        self.chrome.borrow().raise_find();
        let receivers = forward_devtools_events(&page, &webview.webview());
        Ok(DevTools { webview, receivers })
    }

    /// Araçlardan gelen komutu sekmenin motoruna iletir; yanıtı araçlara
    /// geri gönderir.
    fn devtools_message(&mut self, id: u64, text: &str) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if text == devtools::CLOSE {
            if index == self.active && self.tabs[index].devtools.is_some() {
                self.toggle_devtools();
            }
            return;
        }
        let tab = &self.tabs[index];
        let (Some(page), Some(tools)) = (&tab.webview, &tab.devtools) else {
            return;
        };
        let Some(command) = devtools::Command::parse(text) else {
            return;
        };
        let tools = tools.webview.webview();
        // Araçlar sayfanın öğe ağacını istedi: bekleyen "İncele" artık gösterilebilir.
        let ready = (command.method == "DOM.getDocument").then(|| self.proxy.clone());
        let method = HSTRING::from(command.method.as_str());
        let params = HSTRING::from(command.params.as_str());
        let session = command.session.clone();
        let done =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                let reply = match &result {
                    Ok(()) => command.reply(Ok(&json)),
                    Err(err) => command.reply(Err((&json, &err.message()))),
                };
                unsafe {
                    let _ = tools.PostWebMessageAsString(&HSTRING::from(reply));
                }
                if let Some(proxy) = &ready {
                    let _ = proxy.send_event(UserEvent::DevToolsReady(id));
                }
                Ok(())
            }));
        let core = page.webview();
        unsafe {
            let _ = match session {
                None => core.CallDevToolsProtocolMethod(&method, &params, &done),
                Some(session) => match core.cast::<ICoreWebView2_11>() {
                    Ok(core) => core.CallDevToolsProtocolMethodForSession(
                        &HSTRING::from(session),
                        &method,
                        &params,
                        &done,
                    ),
                    Err(err) => Err(err),
                },
            };
        }
    }

    /// Sekmeye sağ tık: sekmenin durumuna göre menü.
    fn open_tab_menu(&mut self, index: usize) {
        use layout::MenuEntry;
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let last = index + 1 == self.tabs.len();
        let rows: Vec<(MenuEntry, Option<TabCommand>)> = vec![
            (
                MenuEntry::item("Yeni sekme", Some("Ctrl+T"), true),
                Some(TabCommand::NewTab),
            ),
            (MenuEntry::Separator, None),
            (
                MenuEntry::item("Yeniden yükle", Some("Ctrl+R"), true),
                Some(TabCommand::Reload),
            ),
            (
                MenuEntry::item("Çoğalt", None, true),
                Some(TabCommand::Duplicate),
            ),
            (
                MenuEntry::item(
                    if tab.muted {
                        "Sekmenin sesini aç"
                    } else {
                        "Sekmenin sesini kapat"
                    },
                    None,
                    true,
                ),
                Some(TabCommand::Mute),
            ),
            (MenuEntry::Separator, None),
            (
                MenuEntry::item("Kapat", Some("Ctrl+W"), true),
                Some(TabCommand::Close),
            ),
            (
                MenuEntry::item("Diğer sekmeleri kapat", None, self.tabs.len() > 1),
                Some(TabCommand::CloseOthers),
            ),
            (
                MenuEntry::item("Sağdaki sekmeleri kapat", None, !last),
                Some(TabCommand::CloseRight),
            ),
            (MenuEntry::Separator, None),
            (
                MenuEntry::item(
                    "Kapatılan sekmeyi yeniden aç",
                    Some("Ctrl+Shift+T"),
                    !self.closed.is_empty(),
                ),
                Some(TabCommand::Reopen),
            ),
        ];
        let (entries, commands): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        self.tab_menu = Some((tab.id, commands));
        self.chrome.borrow_mut().show_menu(entries);
    }

    fn tab_command(&mut self, id: u64, command: TabCommand) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        match command {
            TabCommand::NewTab => self.handle(UserEvent::NewTab),
            TabCommand::Reload => {
                if index != self.active {
                    self.activate(index);
                }
                self.handle(UserEvent::Reload);
            }
            TabCommand::Duplicate => {
                let url = self.tabs[index].url.clone();
                let title = self.tabs[index].title.clone();
                let new = self.add_tab(&url, &title);
                self.move_tab(new, index + 1);
                self.activate(index + 1);
            }
            TabCommand::Mute => {
                let tab = &mut self.tabs[index];
                tab.muted = !tab.muted;
                set_muted(tab.webview.as_ref(), tab.muted);
                self.sync_chrome();
            }
            TabCommand::Close => self.close_tab(index),
            TabCommand::CloseOthers => {
                self.activate(index);
                let others: Vec<u64> = self
                    .tabs
                    .iter()
                    .map(|t| t.id)
                    .filter(|t| *t != id)
                    .collect();
                for other in others {
                    if let Some(i) = self.index_of(other) {
                        self.close_tab(i);
                    }
                }
            }
            TabCommand::CloseRight => {
                let right: Vec<u64> = self.tabs[index + 1..].iter().map(|t| t.id).collect();
                for other in right {
                    if let Some(i) = self.index_of(other) {
                        self.close_tab(i);
                    }
                }
            }
            TabCommand::Reopen => self.handle(UserEvent::ReopenTab),
        }
    }

    /// "İncele": sağ tıklanan öğenin kimliğini sayfanın motorundan ister.
    fn inspect_element(&mut self, id: u64) {
        let Some(page) = self
            .index_of(id)
            .and_then(|i| self.tabs[i].webview.as_ref())
            .map(|w| w.webview())
        else {
            return;
        };
        let proxy = self.proxy.clone();
        let core = page.clone();
        let evaluated =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                let Some(object) = result.ok().and_then(|_| devtools::object_id(&json)) else {
                    return Ok(());
                };
                let proxy = proxy.clone();
                let release = core.clone();
                let described =
                    CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |r, json| {
                        if let Some(node) = r.ok().and_then(|_| devtools::backend_node_id(&json)) {
                            let _ = proxy.send_event(UserEvent::InspectNode(id, node));
                        }
                        let done =
                            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| {
                                Ok(())
                            }));
                        unsafe {
                            let _ = release.CallDevToolsProtocolMethod(
                                &HSTRING::from("Runtime.releaseObjectGroup"),
                                &HSTRING::from(r#"{"objectGroup":"julie-inspect"}"#),
                                &done,
                            );
                        }
                        Ok(())
                    }));
                let params = serde_json::json!({ "objectId": object }).to_string();
                unsafe {
                    let _ = core.CallDevToolsProtocolMethod(
                        &HSTRING::from("DOM.describeNode"),
                        &HSTRING::from(params),
                        &described,
                    );
                }
                Ok(())
            }));
        unsafe {
            let _ = page.CallDevToolsProtocolMethod(
                &HSTRING::from("Runtime.evaluate"),
                &HSTRING::from(devtools::INSPECT_TARGET),
                &evaluated,
            );
        }
    }

    /// Öğeyi araçlarda gösterir; araçlar kapalıysa açar ve hazır olunca gösterir.
    fn inspect_node(&mut self, id: u64, node: i64) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if self.tabs[index].devtools.is_some() {
            self.post_inspect(id, node);
            return;
        }
        if index != self.active {
            return;
        }
        self.pending_inspect = Some((id, node));
        self.toggle_devtools();
        // Araçlar öğe ağacını hiç istemezse (başka panel açıksa) yine de göster.
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let _ = proxy.send_event(UserEvent::DevToolsReady(id));
        });
    }

    fn post_inspect(&self, id: u64, node: i64) {
        let Some(tools) = self
            .index_of(id)
            .and_then(|i| self.tabs[i].devtools.as_ref())
        else {
            return;
        };
        let message = HSTRING::from(devtools::inspect_message(node));
        unsafe {
            let _ = tools.webview.webview().PostWebMessageAsString(&message);
        }
        let _ = tools.webview.focus();
    }

    fn open_tab(&mut self, url: &str) {
        let index = self.add_tab(url, "");
        self.activate(index);
    }

    /// Sekme şeridinin sonuna, henüz yüklenmemiş (uyuyan) bir sekme ekler.
    /// Sayfa, sekme öne geldiğinde `activate` içinde yüklenir.
    fn add_tab(&mut self, url: &str, title: &str) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let index = self.tabs.len();
        self.tabs.push(Tab {
            id,
            webview: None,
            title: title.to_string(),
            url: url.to_string(),
            filter: TabState::default(),
            power: Power::Discarded,
            hidden_since: None,
            recorded_url: String::new(),
            audio: false,
            cosmetic_exceptions: HashSet::new(),
            generichide: false,
            error_back: 1,
            devtools: None,
            zoom: 1.0,
            muted: false,
        });
        self.sync_chrome();
        index
    }

    /// Önceki oturumun sekmelerini geri getirir. Yalnızca öndeki sekme
    /// yüklenir; diğerleri tıklanınca yüklenir (açılış hızlı, bellek az).
    fn restore_session(&mut self) {
        let session = self
            .session_path
            .as_deref()
            .map(Session::load)
            .unwrap_or_default();
        if session.tabs.is_empty() {
            self.open_tab(START_URL);
            return;
        }
        for tab in &session.tabs {
            self.add_tab(&tab.url, &tab.title);
        }
        self.activate(session.active);
    }

    /// Sekmeler değiştiyse kısa bir süre sonra oturumu kaydet. Tarayıcı
    /// kapanmadan sonlandırılırsa (Görev Yöneticisi, çökme, elektrik
    /// kesintisi) son değişiklikler kaybolmasın. Art arda gelen değişiklikler
    /// tek kayıtta toplanır; boştayken hiçbir şey çalışmaz.
    fn schedule_save(&mut self) {
        if !self.session_dirty || self.save_pending || self.session_path.is_none() {
            return;
        }
        self.save_pending = true;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let _ = proxy.send_event(UserEvent::SaveSession);
        });
    }

    /// Açık sekmeleri diske yazar.
    fn save_session(&mut self) {
        let Some(path) = &self.session_path else {
            return;
        };
        let tabs = self
            .tabs
            .iter()
            .map(|tab| SavedTab {
                // Tek sayfalık uygulamalar adresi sayfa yüklenmeden de
                // değiştirebilir; açık sekmede en güncel adresi WebView'den al.
                url: tab
                    .webview
                    .as_ref()
                    .and_then(|w| w.url().ok())
                    // Sayfa henüz yüklenmediyse WebView'de boş sayfa var;
                    // sekmenin asıl adresi kaydedilsin, yoksa sekme kaybolur.
                    .filter(|url| !is_blank(url))
                    .map(|url| site_info::shown_url(&url))
                    .unwrap_or_else(|| site_info::shown_url(&tab.url)),
                title: tab.title.clone(),
            })
            .collect();
        let session = Session {
            tabs,
            active: self.active,
        };
        match session.save(path) {
            Ok(()) => self.session_dirty = false,
            Err(err) => crate::log::error(&format!("oturum kaydedilemedi: {err}")),
        }
    }

    /// Sekme için WebView2 oluşturur. Yeni sekmede ve uyuyan sekme
    /// uyandırılırken kullanılır.
    fn create_webview(&mut self, id: u64, url: &str, filter: &TabState) -> WebView {
        match self.try_create_webview(id, url, filter) {
            Ok(webview) => webview,
            Err(err) => {
                // Aynı veri klasörünü kullanan öteki Julie penceresi (ör.
                // ayar değiştikten sonra açılan gizli pencere) öteki
                // donanım hızlandırma ayarıyla çalışıyorsa WebView2
                // argümanları reddeder: onunkine uy.
                crate::log::error(&format!(
                    "WebView2 oluşturulamadı ({err}); donanım hızlandırma ayarı tersiyle deneniyor"
                ));
                self.gpu_off = !self.gpu_off;
                self.start_state.lock().unwrap().gpu_running = !self.gpu_off;
                self.try_create_webview(id, url, filter)
                    .expect("WebView2 oluşturulamadı (WebView2 Runtime yüklü mü?)")
            }
        }
    }

    fn try_create_webview(
        &mut self,
        id: u64,
        url: &str,
        filter: &TabState,
    ) -> wry::Result<WebView> {
        let title_proxy = self.proxy.clone();
        let load_proxy = self.proxy.clone();
        let window_proxy = self.proxy.clone();
        let ipc_proxy = self.proxy.clone();
        let nav_proxy = self.proxy.clone();
        let nav_filter = filter.clone();
        let nav_guard = Arc::clone(&self.guard);
        let start_state = Arc::clone(&self.start_state);
        let places = self.places.clone();
        let serve_downloads = Arc::clone(&self.downloads);
        let bounds = self.content_bounds();
        let web_theme = self.web_theme();

        // Saat dilimi gizlenecekse sayfa, ayar uygulandıktan sonra açılır;
        // yoksa ilk sayfa gerçek saat dilimini görebilir.
        let hide_timezone = self.settings.privacy.hide_timezone;
        let builder = WebViewBuilder::new_with_web_context(&mut self.context);
        let builder = if hide_timezone {
            builder
        } else {
            builder.with_url(url)
        };
        let webview = builder
            .with_bounds(bounds)
            .with_additional_browser_args(browser_args(self.gpu_off))
            // Gizli pencere: çerezler, önbellek ve site verileri bellekte.
            .with_incognito(self.private)
            // Ctrl + tekerlek / Ctrl + artı-eksi ile yakınlaştırma.
            .with_hotkeys_zoom(true)
            // Edge'in kendi araç penceresi (Copilot vb.) kapalı; F12
            // Julie'nin gömülü araçlarını açar (`toggle_devtools`).
            .with_devtools(false)
            .with_theme(web_theme)
            .with_custom_protocol("julie".into(), move |_, request| {
                serve(
                    request.uri(),
                    *start_state.lock().unwrap(),
                    places.as_deref(),
                    &serve_downloads,
                )
            })
            // `window.ipc` her sayfada vardır; tarayıcı komutlarını yalnızca
            // tarayıcının kendi sayfalarından kabul et.
            .with_ipc_handler(move |request| {
                let page = request.uri().to_string();
                let body = request.body();
                if address::is_internal_page(&page)
                    && let Some(message) = Message::parse(body)
                {
                    let _ = ipc_proxy.send_event(UserEvent::StartPage(id, message));
                } else if let Some(message) = cosmetic::Message::parse(body) {
                    let _ = ipc_proxy.send_event(UserEvent::Cosmetic(id, page, message));
                }
            })
            // Reklamlardan kalan boş kutuları gizleyen betik.
            .with_initialization_script(cosmetic::SCRIPT)
            .with_initialization_script(devtools::INSPECT_SCRIPT)
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(UserEvent::TitleChanged(id, title));
            })
            // Ana sayfa değişiyor: engelleme sayacını sıfırla.
            .with_navigation_handler(move |url| {
                if address::is_engine_page(&url) {
                    let blocked = error_page::blocked_url(&url);
                    let _ = nav_proxy.send_event(UserEvent::Redirect(id, blocked));
                    return false;
                }
                let target = match nav_guard.lock().unwrap().decide(id, &url) {
                    Decision::Allow => None,
                    Decision::Redirect(to) => Some(to),
                    Decision::Warn(http) => Some(privacy::warning_url(&http)),
                };
                if let Some(to) = target {
                    let _ = nav_proxy.send_event(UserEvent::Redirect(id, to));
                    return false;
                }
                nav_filter.start_page_load(&url);
                let _ = nav_proxy.send_event(UserEvent::BlockedCountChanged(id));
                true
            })
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, PageLoadEvent::Started) {
                    let _ = load_proxy.send_event(UserEvent::UrlChanged(id, url));
                }
            })
            // Yeni pencere açmak yerine yeni sekme aç.
            .with_new_window_req_handler(move |url, _features| {
                let _ = window_proxy.send_event(UserEvent::OpenInNewTab(url));
                NewWindowResponse::Deny
            })
            .build_as_child(&self.window)?;
        if hide_timezone {
            let proxy = self.proxy.clone();
            let url = url.to_string();
            set_timezone(
                &webview,
                true,
                Some(Box::new(move || {
                    let _ = proxy.send_event(UserEvent::Redirect(id, url));
                })),
            );
        }
        listen_for_shortcuts(&webview, self.proxy.clone(), false);
        listen_for_source_changes(&webview, self.proxy.clone(), id);
        listen_for_audio(&webview, self.proxy.clone(), id);
        listen_for_zoom(&webview, self.proxy.clone(), id);
        listen_for_context_menu(&webview, self.proxy.clone(), id);
        listen_for_find(&webview, self.proxy.clone(), id);
        listen_for_permissions(&webview, Rc::clone(&self.asks), self.proxy.clone(), id);
        let cert_page = Arc::new(AtomicBool::new(false));
        let cert_events = listen_for_cert_errors(
            &webview,
            Arc::clone(&self.guard),
            self.proxy.clone(),
            id,
            Arc::clone(&cert_page),
        );
        listen_for_navigation_result(
            &webview,
            Arc::clone(&self.guard),
            self.proxy.clone(),
            id,
            cert_events,
            cert_page,
        );
        set_tracking_prevention(&webview, self.settings.privacy.tracking);
        clip_siblings(&webview);
        // Yeni sayfa penceresi açık bulma kutusunun üstüne çıkmasın.
        self.chrome.borrow().raise_find();
        listen_for_favicon(&webview, self.proxy.clone(), id);
        listen_for_downloads(
            &webview,
            id,
            Arc::clone(&self.downloads),
            Rc::clone(&self.download_ops),
            self.proxy.clone(),
        );
        self.place_webview(&webview);
        set_reputation_checking(&webview, !self.settings.smartscreen_off);
        disable_builtin_error_page(&webview);
        scriptlet_injector::install(&webview, self.filter.clone());
        request_filter::install(
            &webview,
            self.filter.clone(),
            filter.clone(),
            self.proxy.clone(),
            id,
        );
        Ok(webview)
    }

    /// Sekmeyi kapatır. Son sekme kapanınca, ayar açıksa tarayıcı da
    /// kapanır; değilse yerine yeni bir başlangıç sekmesi açılır.
    fn close_tab(&mut self, index: usize) {
        let last = self.tabs.len() == 1;
        if last && self.settings.close_with_last_tab {
            self.quit = true;
            return;
        }
        if last {
            if address::is_start_page(&self.tabs[0].url) {
                // Zaten boş bir yeni sekme: kapatmak yerine adres çubuğuna odaklan.
                self.chrome.borrow().focus_address();
                return;
            }
            self.add_tab(START_URL, "");
        }
        // WebView drop edildiğinde WebView2 sayfası ve belleği serbest kalır.
        let mut tab = self.tabs.remove(index);
        if self.downloading_tabs().contains(&tab.id)
            && let Some(webview) = tab.webview.take()
        {
            let _ = webview.set_visible(false);
            self.parked.push((tab.id, webview));
        }
        self.guard.lock().unwrap().forget_tab(tab.id);
        drop_asks(&self.asks, tab.id, None);
        if !address::is_start_page(&tab.url) {
            self.closed.push(tab.url);
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        self.session_dirty = true;
        if index != self.active {
            // Arka plandaki bir sekme kapandı: öndeki sekme aynı kalır,
            // yalnızca sırası kayabilir.
            if index < self.active {
                self.active -= 1;
            }
            self.sync_chrome();
            return;
        }
        let next = if index >= self.tabs.len() {
            self.tabs.len() - 1
        } else {
            index
        };
        // `activate` eski sekmeyi gizlemeye çalışmasın diye geçerli bir indeks ver.
        self.active = next;
        self.activate(next);
        if last {
            self.chrome.borrow().focus_address();
        }
    }

    /// Sayfada bul kutusunu açar; içinde metin kaldıysa yeniden arar.
    fn open_find(&mut self) {
        let Some(webview) = self.tabs.get(self.active).and_then(|t| t.webview.as_ref()) else {
            return;
        };
        if find_of(webview).is_none() {
            return;
        }
        let text = self.chrome.borrow_mut().open_find();
        if !text.is_empty() {
            self.find_text(&text);
        }
    }

    fn find_text(&mut self, text: &str) {
        let case_sensitive = self.chrome.borrow().find_case();
        let Some(webview) = self.tabs.get(self.active).and_then(|t| t.webview.as_ref()) else {
            return;
        };
        if text.is_empty() {
            stop_find(webview);
            self.chrome.borrow_mut().set_find_count(0, 0);
            return;
        }
        if let Err(err) = start_find(webview, text, case_sensitive) {
            crate::log::error(&format!("sayfada bulunamadı: {err}"));
        }
    }

    /// Sonraki/önceki eşleşme. Kutu kapalıysa (F3) önce açılır.
    fn find_step(&mut self, forward: bool) {
        if !self.chrome.borrow().find_visible() {
            self.open_find();
            return;
        }
        let Some(find) = self
            .tabs
            .get(self.active)
            .and_then(|t| t.webview.as_ref())
            .and_then(find_of)
        else {
            return;
        };
        let _ = unsafe {
            if forward {
                find.FindNext()
            } else {
                find.FindPrevious()
            }
        };
    }

    /// Bulma kutusunu kapatır ve vurgulamaları kaldırır.
    fn close_find(&mut self, focus_page: bool) {
        if let Some(webview) = self.tabs.get(self.active).and_then(|t| t.webview.as_ref()) {
            stop_find(webview);
            if focus_page {
                let _ = webview.focus();
            }
        }
        self.chrome.borrow_mut().close_find();
    }

    /// Sekmeyi `from` sırasından `to` sırasına taşır; öndeki sekme aynı kalır.
    fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            // Arayüz yine de yeni modeli beklediği için gönder.
            self.sync_chrome();
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = layout::follow_move(self.active, from, to);
        self.session_dirty = true;
        self.sync_chrome();
    }

    fn activate(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // Bulma kutusu öndeki sekmeye ait; sekme değişince kapanır.
        if index != self.active && self.chrome.borrow().find_visible() {
            self.close_find(false);
        }
        // Arka plandaki sekmeleri gizle ve bellek kullanımını düşük seviyeye çek.
        // WebView2 gizli sekmelerde zamanlayıcıları ve çizimi zaten yavaşlatır.
        self.session_dirty = true;
        let now = Instant::now();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == index || tab.hidden_since.is_some() {
                continue;
            }
            tab.hidden_since = Some(now);
            if let Some(webview) = &tab.webview {
                let _ = webview.set_visible(false);
                let _ = webview.set_memory_usage_level(MemoryUsageLevel::Low);
            }
            if let Some(tools) = &tab.devtools {
                let _ = tools.webview.set_visible(false);
            }
        }
        self.active = index;

        // Bellekten atılmış sekmeyi kaldığı adresten yeniden yükle.
        if self.tabs[index].webview.is_none() {
            let (id, url, filter) = {
                let tab = &self.tabs[index];
                (tab.id, tab.url.clone(), tab.filter.clone())
            };
            self.tabs[index].webview = Some(self.create_webview(id, &url, &filter));
            self.tabs[index].zoom = 1.0;
            if self.tabs[index].muted {
                set_muted(self.tabs[index].webview.as_ref(), true);
            }
        }

        let tab = &mut self.tabs[index];
        tab.hidden_since = None;
        tab.power = Power::Awake;
        let tab = &self.tabs[index];
        let webview = tab.webview.as_ref().expect("az önce uyandırıldı");
        // Görünür yapmak dondurulmuş sayfayı da otomatik çözer.
        let _ = webview.set_memory_usage_level(MemoryUsageLevel::Normal);
        let _ = webview.set_visible(true);
        if let Some(tools) = &tab.devtools {
            let _ = tools.webview.set_visible(true);
        }
        let _ = webview.focus();
        self.update_side_panel();
        self.show_address(&tab.url);
        let title = tab.title.clone();
        self.set_window_title(&title);
        self.update_navigation();
        self.update_shield();
        self.update_star();
        self.show_ask();
    }

    fn set_window_title(&self, page_title: &str) {
        let app = app_title(self.private);
        if page_title.trim().is_empty() {
            self.window.set_title(app);
        } else {
            self.window.set_title(&format!("{page_title} — {app}"));
        }
    }
}

/// Kullanıcıya sorulan, WebView2'nin yanıt beklediği izin isteği.
struct PendingAsk {
    tab: u64,
    /// İzni isteyen köken (sayfanın içindeki bir çerçeve de olabilir).
    origin: String,
    /// İstek geldiğinde sekmede açık sayfanın kökeni: sayfadan çıkılınca
    /// soru düşer.
    page: Option<String>,
    kind: i32,
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
}

impl PendingAsk {
    /// WebView2'ye yanıtı verir. `save`: karar profilde hatırlansın.
    fn finish(self, allow: bool, save: bool) {
        let state = if allow {
            COREWEBVIEW2_PERMISSION_STATE_ALLOW
        } else {
            COREWEBVIEW2_PERMISSION_STATE_DENY
        };
        unsafe {
            let _ = self.args.SetState(state);
            if let Ok(args) = self
                .args
                .cast::<ICoreWebView2PermissionRequestedEventArgs3>()
            {
                let _ = args.SetSavesInProfile(save);
            }
            let _ = self.deferral.Complete();
        }
    }
}

/// Sekmenin bekleyen sorularını (hatırlamadan) reddeder. `page` verilirse
/// yalnızca başka sayfada sorulmuş olanlar düşer.
fn drop_asks(asks: &RefCell<Vec<PendingAsk>>, tab: u64, page: Option<&str>) {
    let gone: Vec<PendingAsk> = {
        let mut list = asks.borrow_mut();
        let (gone, keep) = std::mem::take(&mut *list).into_iter().partition(|a| {
            a.tab == tab
                && page.is_none_or(|p| {
                    a.page
                        .as_deref()
                        .is_none_or(|q| !site_info::same_origin(p, q))
                })
        });
        *list = keep;
        gone
    };
    for ask in gone {
        ask.finish(false, false);
    }
}

/// Site izin isteyince WebView2'nin kendi sorusu yerine Julie'ninkini
/// göster: istek bekletilir, yanıt gelince tamamlanır.
fn listen_for_permissions(
    webview: &WebView,
    asks: Rc<RefCell<Vec<PendingAsk>>>,
    proxy: EventLoopProxy<UserEvent>,
    id: u64,
) {
    let handler = PermissionRequestedEventHandler::create(Box::new(move |sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
        let mut uri = PWSTR::null();
        unsafe {
            args.PermissionKind(&mut kind)?;
            args.Uri(&mut uri)?;
        }
        let uri = take_pwstr(uri);
        let Some(origin) = site_info::origin_of(&uri) else {
            return Ok(());
        };
        if site_info::ask_text(kind.0).is_none() {
            return Ok(());
        }
        let page = sender.and_then(|core| {
            let mut source = PWSTR::null();
            unsafe { core.Source(&mut source) }.ok()?;
            site_info::origin_of(&take_pwstr(source))
        });
        let deferral = unsafe { args.GetDeferral()? };
        asks.borrow_mut().push(PendingAsk {
            tab: id,
            origin,
            page,
            kind: kind.0,
            args,
            deferral,
        });
        let _ = proxy.send_event(UserEvent::PermissionAsked(id));
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview
            .webview()
            .add_PermissionRequested(&handler, &mut token);
    }
}

/// Site panelinin sekmesi, kökeni ve kayıtlı izinleri (tür, durum).
type SitePermissions = (u64, String, Vec<(i32, i32)>);

/// Hata ya da şifresiz bağlantı uyarısı sayfası mı?
fn is_problem_page(url: &str) -> bool {
    matches!(address::internal_page(url), Some("error" | "insecure"))
}

/// WebView'in ilk (boş) sayfası mı?
fn is_blank(url: &str) -> bool {
    url.is_empty() || url == "about:blank"
}
