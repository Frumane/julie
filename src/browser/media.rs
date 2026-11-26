//! Resim içinde resim: sekmedeki videoyu motorun küçük penceresinde
//! oynatır. Betik ve sonucun okunması `crate::pip` içinde.

use wry::WebViewExtWindows;

use super::Browser;
use super::shot::call;
use crate::UserEvent;
use crate::pip::{self, Outcome};
use crate::ui::layout;

/// Resim içinde resim bildiriminin kimliği.
const PIP_TOAST: u64 = u64::MAX - 3;

impl Browser {
    /// Sekmedeki videoyu küçük pencereye alır (açıksa geri getirir).
    pub(super) fn picture_in_picture(&mut self, id: u64) {
        let Some(page) = self
            .index_of(id)
            .and_then(|i| self.tabs[i].webview.as_ref())
            .map(|w| w.webview())
        else {
            return;
        };
        let proxy = self.proxy.clone();
        call(&page, "Runtime.evaluate", &pip::params(), move |result| {
            let outcome = match result {
                Ok(json) => pip::outcome(&json),
                Err(err) => Outcome::Failed(err),
            };
            let _ = proxy.send_event(UserEvent::PictureInPictureDone(outcome));
        });
    }

    pub(super) fn picture_in_picture_done(&mut self, outcome: Outcome) {
        let (title, body) = match outcome {
            Outcome::Opened | Outcome::Closed => return,
            Outcome::NoVideo => (
                "Video bulunamadı",
                "Bu sekmede oynatılabilir bir video yok. Videoyu başlatıp yeniden dene.",
            ),
            Outcome::Failed(err) => {
                crate::log::error(&format!("resim içinde resim açılamadı: {err}"));
                (
                    "Video küçük pencerede açılamadı",
                    "Site ya da video buna izin vermiyor olabilir.",
                )
            }
        };
        self.chrome.borrow_mut().show_toast(layout::ToastInfo {
            key: PIP_TOAST,
            site: "Julie".to_string(),
            icon: None,
            title: title.to_string(),
            body: body.to_string(),
        });
    }
}
