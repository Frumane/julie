//! Okuma modu: sayfa yüklenince uygun mu diye bakar (adres çubuğunda kitap
//! düğmesi), düğmeye basınca yazıyı çıkarıp `julie://reader` sayfasında
//! gösterir. Kütüphane ve sayfa `crate::reader` içinde.

use super::Browser;
use crate::UserEvent;
use crate::reader::{self, Article};
use crate::ui::layout;

/// Okuma modu bildiriminin kimliği.
const READER_TOAST: u64 = u64::MAX - 4;

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Aynı sayfa mı (`#bölüm` farkı sayılmaz)?
fn same_page(a: &str, b: &str) -> bool {
    a.split('#').next() == b.split('#').next()
}

impl Browser {
    /// Yüklenen sayfa okuma moduna uygun mu? (Sonuç `Readerable` olarak gelir.)
    pub(super) fn check_readerable(&mut self, id: u64, url: &str) {
        if !is_web(url) || crate::address::is_internal_page(url) {
            return;
        }
        let Some(webview) = self
            .index_of(id)
            .and_then(|i| self.tabs[i].webview.as_ref())
        else {
            return;
        };
        let proxy = self.proxy.clone();
        let url = url.to_string();
        let _ = webview.evaluate_script_with_callback(&reader::detect_script(), move |result| {
            let _ = proxy.send_event(UserEvent::Readerable(
                id,
                url.clone(),
                result.trim() == "true",
            ));
        });
    }

    pub(super) fn readerable(&mut self, id: u64, url: &str, yes: bool) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        // Bu arada başka sayfaya geçildiyse eski sonuç sayılmaz.
        if !same_page(&self.tabs[index].url, url) || self.tabs[index].readerable == yes {
            return;
        }
        self.tabs[index].readerable = yes;
        self.sync_chrome();
    }

    /// Kitap düğmesi ya da F9: okuma moduna geç ya da çık.
    pub(super) fn toggle_reader(&mut self) {
        let index = self.active;
        let url = self.tabs[index].url.clone();
        let Some(webview) = self.tabs[index].webview.as_ref() else {
            return;
        };
        if url.starts_with(reader::READER_URL) {
            // Çık: geçmişte bir önceki sayfa asıl sayfadır.
            if webview.can_go_back().unwrap_or(false) {
                let _ = webview.go_back();
            } else if let Some(original) = reader::original_url(&url) {
                let _ = webview.load_url(&original);
            }
            return;
        }
        if !is_web(&url) {
            return;
        }
        let id = self.tabs[index].id;
        let proxy = self.proxy.clone();
        let _ = webview.evaluate_script_with_callback(&reader::extract_script(), move |json| {
            let article = (json.trim() != "null").then_some(json);
            let _ = proxy.send_event(UserEvent::ReaderArticle(id, url.clone(), article));
        });
    }

    /// Yazı çıkarıldı: saklayıp okuma modu sayfasını açar.
    pub(super) fn reader_article(&mut self, id: u64, url: &str, json: Option<String>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if !same_page(&self.tabs[index].url, url) {
            return;
        }
        let current = self.tabs[index].url.clone();
        let Some(article) = json.and_then(|j| Article::parse(&j, &current)) else {
            self.chrome.borrow_mut().show_toast(layout::ToastInfo {
                key: READER_TOAST,
                site: "Julie".to_string(),
                icon: None,
                title: "Okuma modu açılamadı".to_string(),
                body: "Bu sayfada okunacak bir yazı bulunamadı.".to_string(),
            });
            return;
        };
        let page = self.articles.lock().unwrap().add(article);
        if let Some(webview) = &self.tabs[index].webview {
            let _ = webview.load_url(&page);
        }
    }
}
