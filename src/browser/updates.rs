//! Otomatik güncelleme: yeni sürüm bildirimi, indirme ve yeniden
//! başlatma. Denetleme ve indirme `crate::update` içinde.

use std::path::PathBuf;
use std::time::Duration;

use tao::event_loop::EventLoopProxy;

use super::Browser;
use crate::UserEvent;
use crate::ui::layout;
use crate::update::{self, Release};

/// Güncelleme bildiriminin kimliği.
const UPDATE_TOAST: u64 = u64::MAX - 5;
/// Açılıştan sonra ilk denetleme (açılışı yavaşlatmasın).
const FIRST_CHECK: Duration = Duration::from_secs(15);
/// Julie açık kaldıkça tekrar denetleme aralığı.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Güncellemenin durumu.
#[derive(Debug, Default)]
pub(super) enum State {
    #[default]
    Idle,
    /// Yeni sürüm var; kullanıcı bildirime tıklarsa indirilir.
    Found(Release),
    Downloading,
    /// Kuruldu; Julie yeniden başlayınca açılır.
    Ready(String),
}

/// Kurulu Julie'de arka planda güncelleme denetimini başlatır.
pub(super) fn start(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            match update::net::check() {
                Ok(Some(release)) => {
                    if proxy.send_event(UserEvent::UpdateFound(release)).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(err) => crate::log::error(&err),
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

impl Browser {
    fn update_toast(&self, title: String, body: &str) {
        self.chrome.borrow_mut().show_toast(layout::ToastInfo {
            key: UPDATE_TOAST,
            site: "Julie".to_string(),
            icon: None,
            title,
            body: body.to_string(),
        });
    }

    pub(super) fn update_found(&mut self, release: Release) {
        if matches!(self.update, State::Downloading | State::Ready(_)) {
            return;
        }
        self.update_toast(
            format!("Julie {} çıktı", release.version),
            "Güncellemek için tıkla. Sekmelerin kaybolmaz.",
        );
        self.update = State::Found(release);
    }

    /// Güncelleme bildirimine tıklandıysa `true`.
    pub(super) fn update_toast_click(&mut self, key: u64) -> bool {
        if key != UPDATE_TOAST {
            return false;
        }
        self.chrome.borrow_mut().hide_toast(Some(key));
        match std::mem::take(&mut self.update) {
            State::Found(release) => self.download_update(release),
            State::Ready(version) => {
                self.update = State::Ready(version);
                self.restart();
            }
            other => self.update = other,
        }
        true
    }

    fn download_update(&mut self, release: Release) {
        let Some(exe) = crate::install::running_installed() else {
            return;
        };
        self.update_toast(
            format!("Julie {} indiriliyor…", release.version),
            "Bitince haber vereceğim.",
        );
        self.update = State::Downloading;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = update::net::install(&release, &exe).map(|()| release.version);
            let _ = proxy.send_event(UserEvent::UpdateInstalled(result));
        });
    }

    pub(super) fn update_installed(&mut self, result: Result<String, String>) {
        match result {
            Ok(version) => {
                self.update_toast(
                    format!("Julie {version} hazır"),
                    "Yeniden başlatmak için tıkla. Sonraya bırakırsan Julie'yi \
                     bir dahaki açışında yeni sürüm açılır.",
                );
                self.update = State::Ready(version);
            }
            Err(err) => {
                crate::log::error(&format!("güncelleme yapılamadı: {err}"));
                self.update_toast(
                    "Güncelleme yapılamadı".to_string(),
                    "Julie bir dahaki açılışta yeniden deneyecek.",
                );
                self.update = State::Idle;
            }
        }
    }

    /// Yeni sürümü açar ve bu Julie'yi kapatır (oturum kaydedilir,
    /// sekmeler yeni Julie'de geri gelir).
    fn restart(&mut self) {
        let Some(exe): Option<PathBuf> = crate::install::running_installed() else {
            return;
        };
        let spawned = std::process::Command::new(exe)
            .arg(update::WAIT_ARG)
            .arg(std::process::id().to_string())
            .spawn();
        match spawned {
            Ok(_) => self.quit = true,
            Err(err) => crate::log::error(&format!("Julie yeniden başlatılamadı: {err}")),
        }
    }
}
