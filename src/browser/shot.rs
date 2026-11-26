//! Sağ tık menüsündeki "Ekran görüntüsü al": görüntüyü sayfa motoruna
//! çizdirir, İndirilenler'e kaydeder, panoya kopyalar ve bildirim gösterir.
//! Dosya ve pano işleri `crate::screenshot` içinde.

use std::cell::Cell;
use std::rc::Rc;

use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows::core::HSTRING;
use wry::WebViewExtWindows;

use super::Browser;
use crate::UserEvent;
use crate::screenshot;
use crate::ui::layout;

/// Ekran görüntüsü bildiriminin kimliği (site bildirimleriyle karışmaz).
pub(super) const SCREENSHOT_TOAST: u64 = u64::MAX - 1;

type Done = Rc<Cell<Option<Box<dyn FnOnce(Result<String, String>)>>>>;

/// Motora bir DevTools komutu gönderir; yanıt (JSON) ya da hata `then`'e gelir.
pub(super) fn call(
    page: &ICoreWebView2,
    method: &str,
    params: &str,
    then: impl FnOnce(Result<String, String>) + 'static,
) {
    let then: Done = Rc::new(Cell::new(Some(Box::new(then))));
    let later = Rc::clone(&then);
    let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
        if let Some(then) = later.take() {
            then(result.map(|_| json).map_err(|e| e.to_string()));
        }
        Ok(())
    }));
    let sent = unsafe {
        page.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &done)
    };
    if let Err(err) = sent
        && let Some(then) = then.take()
    {
        then(Err(err.to_string()));
    }
}

impl Browser {
    /// Sekmenin (yoksa açık sekmenin) görüntüsünü ister; `full`: tüm sayfa.
    pub(super) fn screenshot(&mut self, id: Option<u64>, full: bool) {
        let index = match id {
            Some(id) => self.index_of(id),
            None => Some(self.active),
        };
        let Some(page) = index
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| t.webview.as_ref())
            .map(|w| w.webview())
        else {
            return;
        };
        let proxy = self.proxy.clone();
        let finish = move |result: Result<String, String>| {
            let png = result.and_then(|json| {
                screenshot::png_from_reply(&json).ok_or_else(|| "boş yanıt".to_string())
            });
            let _ = proxy.send_event(UserEvent::ScreenshotTaken(png));
        };
        if !full {
            call(
                &page,
                "Page.captureScreenshot",
                &screenshot::visible_params(),
                finish,
            );
            return;
        }
        let core = page.clone();
        call(
            &page,
            "Page.getLayoutMetrics",
            "{}",
            move |result| match result.map(|json| screenshot::full_params(&json)) {
                Ok(Some(params)) => call(&core, "Page.captureScreenshot", &params, finish),
                Ok(None) => finish(Err("sayfanın boyu okunamadı".into())),
                Err(err) => finish(Err(err)),
            },
        );
    }

    /// Görüntü geldi: kaydeder, panoya koyar, bildirim gösterir.
    pub(super) fn screenshot_taken(&mut self, result: Result<Vec<u8>, String>) {
        let (title, body) = match result.and_then(|png| self.save_screenshot(&png)) {
            Ok(body) => ("Ekran görüntüsü alındı".to_string(), body),
            Err(err) => {
                crate::log::error(&format!("ekran görüntüsü alınamadı: {err}"));
                self.screenshot_file = None;
                (
                    "Ekran görüntüsü alınamadı".to_string(),
                    "Sayfa yüklenirken ya da çok büyükken olabilir. Biraz sonra yeniden dene."
                        .to_string(),
                )
            }
        };
        self.chrome.borrow_mut().show_toast(layout::ToastInfo {
            key: SCREENSHOT_TOAST,
            site: "Julie".to_string(),
            icon: None,
            title,
            body,
        });
    }

    /// PNG'yi İndirilenler'e yazar ve panoya kopyalar; bildirim metnini döndürür.
    fn save_screenshot(&mut self, png: &[u8]) -> Result<String, String> {
        let dir = screenshot::win::downloads_dir()
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|p| std::path::PathBuf::from(p).join("Downloads"))
            })
            .ok_or("İndirilenler klasörü bulunamadı")?;
        let name = screenshot::file_name(screenshot::win::now());
        let path = screenshot::unique_path(&dir, &name, |p| p.exists());
        std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
        let copied = match screenshot::win::copy_to_clipboard(png) {
            Ok(()) => true,
            Err(err) => {
                crate::log::error(&format!("ekran görüntüsü panoya kopyalanamadı: {err}"));
                false
            }
        };
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.screenshot_file = Some(path);
        Ok(if copied {
            format!("Panoya kopyalandı ve İndirilenler'e kaydedildi: {file}. Açmak için tıkla.")
        } else {
            format!("İndirilenler'e kaydedildi: {file}. Açmak için tıkla.")
        })
    }
}
