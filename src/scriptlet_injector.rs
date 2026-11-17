//! uBlock betiklerini (scriptlet) sayfanın kendi kodundan önce çalıştırır.
//!
//! YouTube reklamlarını engelleyen betikler, sayfa reklam verisini okumadan
//! önce `fetch`, `XMLHttpRequest`, `JSON.parse` gibi işlevleri sarmalamak
//! zorundadır. Bu yüzden WebView2'nin "belge oluşturulurken çalıştır"
//! özelliğini kullanıyoruz.
//!
//! Betikler site başına kaydedilir: bir site için kaydedilen betik o sekmede
//! sonraki bütün ziyaretlerde hazır bulunur. Betiğin içinde site adı kontrol
//! edilir; başka sitede çalışmaz.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, NavigationStartingEventHandler, take_pwstr,
};
use windows::core::{HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::address::exact_host;
use crate::cosmetic::wrap_for_host;
use crate::request_filter::Shared;

/// Bir sekmede en fazla bu kadar sitenin betiği kayıtlı tutulur.
const MAX_SITES: usize = 32;

struct Registration {
    host: String,
    /// WebView2'nin verdiği kimlik; betik yoksa ya da henüz gelmediyse boş.
    id: Rc<RefCell<Option<String>>>,
}

#[derive(Default)]
struct State {
    sites: RefCell<VecDeque<Registration>>,
    /// Kayıtlar engelleyicinin hangi sürümüne göre yapıldı.
    generation: Cell<u64>,
}

/// Sekmede ana sayfa her değiştiğinde gerekli betikleri kaydeder.
pub fn install(webview: &WebView, shared: Shared) {
    let state = Rc::new(State::default());
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(core), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let url = take_pwstr(uri);
        prepare(&core, &state, &shared, &url);
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = webview
            .webview()
            .add_NavigationStarting(&handler, &mut token);
    }
}

fn prepare(core: &ICoreWebView2, state: &State, shared: &Shared, url: &str) {
    // Kurallar güncellendiyse ya da bir sitede engelleyici açılıp
    // kapatıldıysa eski kayıtların hepsi geçersizdir.
    let generation = shared.generation.get();
    if state.generation.get() != generation {
        let mut sites = state.sites.borrow_mut();
        while let Some(site) = sites.pop_front() {
            remove(core, &site);
        }
        state.generation.set(generation);
    }

    let Some(host) = exact_host(url) else { return };
    if state.sites.borrow().iter().any(|s| s.host == host) {
        return;
    }
    let id = Rc::new(RefCell::new(None));
    let script = if shared.disabled_for(url) {
        String::new()
    } else {
        shared
            .blocker
            .borrow()
            .as_ref()
            .map(|b| b.scriptlets(url))
            .unwrap_or_default()
    };
    if !script.is_empty() {
        let wrapped = wrap_for_host(&host, &script);
        let id_slot = Rc::clone(&id);
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, new_id| {
                if result.is_ok() {
                    *id_slot.borrow_mut() = Some(new_id);
                }
                Ok(())
            },
        ));
        unsafe {
            let _ = core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(wrapped), &done);
        }
    }

    // Betiği olmayan siteler de kaydedilir; her ziyarette yeniden hesaplanmaz.
    let mut sites = state.sites.borrow_mut();
    sites.push_back(Registration { host, id });
    if sites.len() > MAX_SITES
        && let Some(old) = sites.pop_front()
    {
        remove(core, &old);
    }
}

fn remove(core: &ICoreWebView2, site: &Registration) {
    if let Some(id) = site.id.borrow().as_ref() {
        unsafe {
            let _ = core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id.as_str()));
        }
    }
}
