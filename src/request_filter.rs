//! WebView2'nin ağ isteklerini yakalayıp reklam engelleyiciden geçirir.
//!
//! Engellenen istek ağa hiç çıkmaz: WebView2'ye boş bir 403 yanıtı veririz.
//! Böylece reklam betikleri indirilmez, çalıştırılmaz ve bellek tutmaz.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use tao::event_loop::EventLoopProxy;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL, ICoreWebView2_22,
};
use webview2_com::{WebResourceRequestedEventHandler, take_pwstr};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{HSTRING, Interface, PWSTR, w};
use wry::{WebView, WebViewExtWindows};

use crate::UserEvent;
use crate::address;
use crate::blocking::{self, Blocker, Verdict};
use crate::settings;

/// Bütün sekmelerin ortak kullandığı engelleyici durumu.
/// WebView2 olayları arayüz iş parçacığında geldiği için `Rc` yeterli.
#[derive(Clone, Default)]
pub struct Shared {
    /// Kurallar henüz yüklenmediyse `None` (ilk açılışta indirilirken).
    pub blocker: Rc<RefCell<Option<Blocker>>>,
    /// Engelleyicinin kapalı olduğu siteler.
    pub disabled: Rc<RefCell<BTreeSet<String>>>,
    /// Kurallar ya da site istisnaları değiştikçe artar; sekmelere kayıtlı
    /// betiklerin yenilenmesi gerektiğini bildirir.
    pub generation: Rc<Cell<u64>>,
}

/// Tek bir sekmenin engelleyici durumu.
#[derive(Clone, Default)]
pub struct TabState {
    /// Sekmede açık olan ana sayfanın adresi.
    pub page_url: Rc<RefCell<String>>,
    /// Bu sayfada engellenen istek sayısı.
    pub blocked: Rc<Cell<u32>>,
}

impl TabState {
    /// Ana sayfa değişirken çağrılır: sayaç sıfırlanır.
    pub fn start_page_load(&self, url: &str) {
        *self.page_url.borrow_mut() = url.to_string();
        self.blocked.set(0);
    }
}

impl Shared {
    /// Kurallar ya da istisnalar değişti.
    pub fn bump(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    /// Sayfada engelleyici kapalı mı?
    pub fn disabled_for(&self, page_url: &str) -> bool {
        address::host_of(page_url)
            .is_some_and(|host| settings::is_listed(&self.disabled.borrow(), &host))
    }

    fn check(&self, url: &str, page_url: &str, context: i32) -> Verdict {
        // Tarayıcının kendi sayfaları ve ana belge asla engellenmez.
        if address::is_start_page(url) || page_url.is_empty() {
            return Verdict::Allow;
        }
        // Ana belge: adres birebir aynı ya da (yönlendirme/normalleştirme
        // farkları için) aynı sitede bir belge isteği.
        let is_main_frame = context == 1
            && (url == page_url || address::host_of(url) == address::host_of(page_url));
        let kind = blocking::request_type(context, is_main_frame);
        if kind == "main_frame" || self.disabled_for(page_url) {
            return Verdict::Allow;
        }
        match &*self.blocker.borrow() {
            Some(blocker) => blocker.check(url, page_url, kind),
            None => Verdict::Allow,
        }
    }
}

/// Sekmenin bütün isteklerini (alt çerçeveler ve worker'lar dahil)
/// engelleyiciden geçirir.
pub fn install(
    webview: &WebView,
    shared: Shared,
    tab: TabState,
    proxy: EventLoopProxy<UserEvent>,
    id: u64,
) {
    unsafe {
        let core = webview.webview();
        let environment = webview.environment();

        // Yeni WebView2 sürümlerinde iframe ve worker isteklerini de almak
        // için kaynak türünü açıkça belirtmek gerekiyor.
        let with_kinds = core.cast::<ICoreWebView2_22>().and_then(|core22| {
            core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        });
        if with_kinds.is_err() {
            let _ =
                core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
        }

        let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            args.Request()?.Uri(&mut uri)?;
            let url = take_pwstr(uri);
            let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
            args.ResourceContext(&mut context)?;

            let page_url = tab.page_url.borrow().clone();
            let response = match shared.check(&url, &page_url, context.0) {
                Verdict::Allow => return Ok(()),
                Verdict::Block => {
                    environment.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?
                }
                Verdict::Redirect(data_url) => match decode_data_url(&data_url) {
                    Some((mime, body)) => {
                        let stream = SHCreateMemStream(Some(&body));
                        let headers = HSTRING::from(format!(
                            "Content-Type: {mime}\r\nAccess-Control-Allow-Origin: *"
                        ));
                        environment.CreateWebResourceResponse(
                            stream.as_ref(),
                            200,
                            w!("OK"),
                            &headers,
                        )?
                    }
                    None => {
                        environment.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?
                    }
                },
            };
            args.SetResponse(&response)?;
            tab.blocked.set(tab.blocked.get() + 1);
            let _ = proxy.send_event(UserEvent::BlockedCountChanged(id));
            Ok(())
        }));
        let mut token = 0i64;
        let _ = core.add_WebResourceRequested(&handler, &mut token);
    }
}

/// `data:<mime>;base64,<veri>` adresini çözer.
fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    use base64::Engine as _;
    let (header, data) = url.strip_prefix("data:")?.split_once(',')?;
    let mime = header.strip_suffix(";base64")?;
    let body = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some((mime.to_string(), body))
}
