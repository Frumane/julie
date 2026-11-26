//! Sağ tık menüsündeki "Türkçeye çevir": betiği sayfaya verir, gelen
//! paketleri çeviri iş parçacıklarına yollar, çevirileri sayfaya geri koyar.
//! Google ile konuşan kısım `crate::translate` içinde.

use super::Browser;
use crate::translate::{self, Batch};
use crate::ui::layout;

/// Çeviri hatası bildiriminin kimliği.
const TRANSLATE_TOAST: u64 = u64::MAX - 2;

impl Browser {
    /// Sekmeyi çevirir (`on`) ya da ilk haline döndürür.
    pub(super) fn translate(&mut self, id: u64, on: bool) {
        let Some(webview) = self
            .index_of(id)
            .and_then(|i| self.tabs[i].webview.as_ref())
        else {
            return;
        };
        if on {
            self.translated.lock().unwrap().insert(id);
            self.translate_warned.remove(&id);
            let _ = webview.evaluate_script(translate::SCRIPT);
        } else {
            self.translated.lock().unwrap().remove(&id);
            let _ = webview.evaluate_script(translate::RESTORE_SCRIPT);
        }
    }

    /// Sayfa bir paket yolladı: yalnızca kullanıcının çevirmek istediği
    /// sekmeden kabul edilir (başka sayfalar Google'a yazı yollatamasın).
    pub(super) fn translate_batch(&mut self, id: u64, batch: Batch) {
        if !self.translated.lock().unwrap().contains(&id) {
            return;
        }
        let proxy = &self.proxy;
        let tx = self
            .translator
            .get_or_insert_with(|| translate::net::spawn(proxy.clone()));
        let _ = tx.send((id, batch));
    }

    /// Paket çevrildi (ya da çevrilemedi).
    pub(super) fn translated(
        &mut self,
        id: u64,
        batch: u64,
        result: Result<(Vec<String>, Option<String>), String>,
    ) {
        if !self.translated.lock().unwrap().contains(&id) {
            return;
        }
        let Some(webview) = self
            .index_of(id)
            .and_then(|i| self.tabs[i].webview.as_ref())
        else {
            return;
        };
        match result {
            Ok((texts, _)) => {
                let _ = webview.evaluate_script(&translate::apply_script(batch, &texts));
            }
            Err(_) => {
                let _ = webview.evaluate_script(&translate::fail_script(batch));
                if self.translate_warned.insert(id) {
                    self.chrome.borrow_mut().show_toast(layout::ToastInfo {
                        key: TRANSLATE_TOAST,
                        site: "Julie".to_string(),
                        icon: None,
                        title: "Sayfanın bir kısmı çevrilemedi".to_string(),
                        body: "Google Çeviri'ye ulaşılamadı. İnternet bağlantını kontrol edip \
                               yeniden dene."
                            .to_string(),
                    });
                }
            }
        }
    }
}
