//! Arama motorundan yazarken öneri alma (arka plan iş parçacığı).
//!
//! Her harfte istek atılmaz: iş parçacığı kısa bir süre bekleyip kuyruktaki
//! en son metni ister. Cevap gelmezse ya da okunamazsa sessizce vazgeçilir;
//! liste geçmişten gelen satırlarla kalır.

use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use tao::event_loop::EventLoopProxy;

use crate::UserEvent;
use crate::settings::Engine;
use crate::suggest;

/// Hızlı yazarken araya giren harfleri atlamak için bekleme.
const DEBOUNCE: Duration = Duration::from_millis(70);
/// Öneri bu sürede gelmezse beklemeye değmez.
const TIMEOUT: Duration = Duration::from_secs(3);
/// Cevabın en fazla boyutu (öneriler birkaç yüz bayttır).
const MAX_BODY: u64 = 64 * 1024;

/// İş parçacığını başlatır; dönen kanala `(motor, metin)` gönderilir.
/// Sonuç `UserEvent::SearchSuggestions` olarak gelir.
pub fn spawn(proxy: EventLoopProxy<UserEvent>) -> Sender<(Engine, String)> {
    let (tx, rx) = mpsc::channel::<(Engine, String)>();
    let started = std::thread::Builder::new()
        .name("arama-onerileri".into())
        .spawn(move || {
            let agent = agent();
            while let Ok(mut job) = rx.recv() {
                std::thread::sleep(DEBOUNCE);
                while let Ok(newer) = rx.try_recv() {
                    job = newer;
                }
                let (engine, query) = job;
                let Some(list) = fetch(&agent, &engine.suggest_url(&query)) else {
                    continue;
                };
                let event = UserEvent::SearchSuggestions(engine, query, list);
                if proxy.send_event(event).is_err() {
                    break;
                }
            }
        });
    if let Err(err) = started {
        crate::log::error(&format!("arama önerileri başlatılamadı: {err}"));
    }
    tx
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(TIMEOUT))
        // Bazı motorlar tarayıcı olmayan istemcilere öneri vermiyor.
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Julie")
        .build()
        .into()
}

fn fetch(agent: &ureq::Agent, url: &str) -> Option<Vec<String>> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| crate::log::error(&format!("arama önerileri alınamadı: {e}")))
        .ok()?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .ok()?;
    Some(suggest::parse_remote(&suggest::decode_body(
        &bytes,
        &content_type,
    )))
}
