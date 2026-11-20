//! Gömülü geliştirici araçları (F12). Chrome'un açık kaynak araçları
//! (devtools-frontend, BSD lisanslı) `assets/devtools.zip` olarak programın
//! içinde gelir ve sekmenin yanındaki panelde `https://julie.devtools/`
//! adresinden açılır. Edge'in kendi araç penceresi (Copilot vb.) kapalıdır.
//!
//! Araçlar sayfaya normalde WebSocket ile bağlanır. Burada bağlantı
//! `devtools_shim.js` ile Julie'ye yönlendirilir: komut (JSON) gelir,
//! sekmenin motoruna iletilir (`CallDevToolsProtocolMethod`), yanıt ve
//! motorun olayları aynı biçimde araçlara geri gönderilir. Bu modül paketi
//! okur ve mesajları biçimler; pencere işleri `browser.rs`'de.
//!
//! Paketi yenilemek için: `tools/fetch_devtools.py`.

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

use serde_json::{Value, json};

/// Araçların sayfası. Sonuna sekme kimliği eklenir (`ws=` bağlantısı).
const PAGE: &str = "https://julie.devtools/devtools_app.html?ws=julie-devtools/";

/// Araç panelinin sayfasına konan köprü betiği.
pub const SHIM: &str = include_str!("devtools_shim.js");

/// Araçların "kapat" isteği.
pub const CLOSE: &str = "julie:close";

static ARCHIVE: &[u8] = include_bytes!("../assets/devtools.zip");

/// Sekme için araç sayfasının adresi.
pub fn url(tab: u64) -> String {
    format!("{PAGE}{tab}")
}

/// Paketteki bir dosya (adres yolu, ör. `/devtools_app.html`) ve türü.
pub fn file(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let name = path.trim_start_matches('/');
    let data = index().get(name).and_then(|entry| entry.read(ARCHIVE))?;
    Some((data, mime_of(name)))
}

/// Araçların dinlediği motor olaylarının adları (paketteki liste).
pub fn events() -> &'static [String] {
    static EVENTS: OnceLock<Vec<String>> = OnceLock::new();
    EVENTS.get_or_init(|| {
        let text = index()
            .get("julie-events.txt")
            .and_then(|e| e.read(ARCHIVE))
            .unwrap_or_default();
        String::from_utf8_lossy(&text)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()
    })
}

fn index() -> &'static HashMap<String, Entry> {
    static INDEX: OnceLock<HashMap<String, Entry>> = OnceLock::new();
    INDEX.get_or_init(|| read_index(ARCHIVE).unwrap_or_default())
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "avif" => "image/avif",
        "gif" => "image/gif",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        _ => "text/plain; charset=utf-8",
    }
}

/// Zip içindeki bir dosyanın yeri.
#[derive(Debug, Clone, Copy)]
struct Entry {
    /// Yerel başlığın başı.
    offset: usize,
    /// 0: sıkıştırılmamış, 8: deflate.
    method: u16,
    packed: usize,
    size: usize,
}

impl Entry {
    fn read(&self, zip: &[u8]) -> Option<Vec<u8>> {
        let local = zip.get(self.offset..)?;
        if u32_at(local, 0)? != 0x0403_4b50 {
            return None;
        }
        let start = 30 + u16_at(local, 26)? as usize + u16_at(local, 28)? as usize;
        let packed = local.get(start..start + self.packed)?;
        match self.method {
            0 => Some(packed.to_vec()),
            8 => {
                let mut out = Vec::with_capacity(self.size);
                flate2::read::DeflateDecoder::new(packed)
                    .read_to_end(&mut out)
                    .ok()?;
                Some(out)
            }
            _ => None,
        }
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Zip'in merkezi dizinini okur: dosya adı -> yeri.
fn read_index(zip: &[u8]) -> Option<HashMap<String, Entry>> {
    // Dizinin sonu kaydı dosyanın sonundadır (ardından en çok 64 KB yorum).
    let tail = zip.len().saturating_sub(22 + 0xFFFF);
    let end = (tail..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(zip, i) == Some(0x0605_4b50))?;
    let count = u16_at(zip, end + 10)? as usize;
    let mut at = u32_at(zip, end + 16)? as usize;
    let mut index = HashMap::with_capacity(count);
    for _ in 0..count {
        if u32_at(zip, at)? != 0x0201_4b50 {
            return None;
        }
        let name_len = u16_at(zip, at + 28)? as usize;
        let skip = name_len + u16_at(zip, at + 30)? as usize + u16_at(zip, at + 32)? as usize;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_len)?).into_owned();
        index.insert(
            name,
            Entry {
                method: u16_at(zip, at + 10)?,
                packed: u32_at(zip, at + 20)? as usize,
                size: u32_at(zip, at + 24)? as usize,
                offset: u32_at(zip, at + 42)? as usize,
            },
        );
        at += 46 + skip;
    }
    Some(index)
}

/// Araçlardan gelen bir protokol komutu.
#[derive(Debug, PartialEq)]
pub struct Command {
    pub id: u64,
    pub method: String,
    /// Parametreler (JSON nesnesi).
    pub params: String,
    /// Alt hedef (iframe, worker) oturumu; ana sayfa için `None`.
    pub session: Option<String>,
}

impl Command {
    pub fn parse(text: &str) -> Option<Command> {
        let value: Value = serde_json::from_str(text).ok()?;
        let params = match value.get("params") {
            Some(p @ Value::Object(_)) => p.to_string(),
            _ => "{}".to_string(),
        };
        Some(Command {
            id: value.get("id")?.as_u64()?,
            method: value.get("method")?.as_str()?.to_string(),
            params,
            session: value
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from),
        })
    }

    /// Motorun yanıtından araçlara gidecek mesaj. `answer`: başarılıysa
    /// sonuç (JSON), değilse motorun hata metni (JSON olabilir) ve
    /// yedek açıklama.
    pub fn reply(&self, answer: Result<&str, (&str, &str)>) -> String {
        let mut out = json!({ "id": self.id });
        match answer {
            Ok(result) => {
                out["result"] = serde_json::from_str(result).unwrap_or_else(|_| json!({}));
            }
            Err((text, fallback)) => {
                let error = serde_json::from_str::<Value>(text)
                    .ok()
                    .filter(|e| e.get("message").is_some())
                    .unwrap_or_else(|| {
                        json!({
                            "code": -32000,
                            "message": if text.trim().is_empty() { fallback } else { text },
                        })
                    });
                out["error"] = error;
            }
        }
        if let Some(session) = &self.session {
            out["sessionId"] = json!(session);
        }
        out.to_string()
    }
}

/// Motorun bir olayından araçlara gidecek mesaj.
pub fn event(method: &str, params: &str, session: Option<&str>) -> String {
    let mut out = json!({
        "method": method,
        "params": serde_json::from_str::<Value>(params).unwrap_or_else(|_| json!({})),
    });
    if let Some(session) = session.filter(|s| !s.is_empty()) {
        out["sessionId"] = json!(session);
    }
    out.to_string()
}

/// Araçlar kapanınca sayfayı eski hâline getiren komutlar. Chrome'da
/// araçlar kapanınca bağlantı kopar ve motor bunları kendisi geri alır;
/// burada bağlantı (Julie'nin kendi komutları için) açık kaldığından elle
/// yapılır. Durdurulmuş betik devam eder, cihaz ve ağ taklidi kalkar.
pub const CLEANUP: &[(&str, &str)] = &[
    ("Debugger.disable", "{}"),
    (
        "Overlay.setInspectMode",
        r#"{"mode":"none","highlightConfig":{}}"#,
    ),
    ("Overlay.hideHighlight", "{}"),
    ("Overlay.disable", "{}"),
    ("Emulation.clearDeviceMetricsOverride", "{}"),
    ("Emulation.setTouchEmulationEnabled", r#"{"enabled":false}"#),
    (
        "Emulation.setEmitTouchEventsForMouse",
        r#"{"enabled":false}"#,
    ),
    (
        "Emulation.setEmulatedMedia",
        r#"{"media":"","features":[]}"#,
    ),
    (
        "Emulation.setEmulatedVisionDeficiency",
        r#"{"type":"none"}"#,
    ),
    ("Emulation.setCPUThrottlingRate", r#"{"rate":1}"#),
    ("Emulation.setFocusEmulationEnabled", r#"{"enabled":false}"#),
    ("Emulation.setAutoDarkModeOverride", "{}"),
    ("Emulation.clearGeolocationOverride", "{}"),
    ("Emulation.setLocaleOverride", "{}"),
    (
        "Network.emulateNetworkConditions",
        r#"{"offline":false,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}"#,
    ),
    ("Network.setCacheDisabled", r#"{"cacheDisabled":false}"#),
    ("Network.setBlockedURLs", r#"{"urls":[]}"#),
    (
        "Target.setAutoAttach",
        r#"{"autoAttach":false,"waitForDebuggerOnStart":false}"#,
    ),
    ("DOM.disable", "{}"),
    ("CSS.disable", "{}"),
    ("Log.disable", "{}"),
    ("Network.disable", "{}"),
    ("Profiler.disable", "{}"),
    ("HeapProfiler.disable", "{}"),
    ("Animation.disable", "{}"),
    ("Audits.disable", "{}"),
    ("Runtime.disable", "{}"),
];

/// Panelin varsayılan genişliği: sayfa alanının binde kaçı.
pub const DEFAULT_WIDTH: u16 = 400;

/// Sayfa ile panel arasındaki boşluk (sürükleme tutamağı), fiziksel piksel.
fn gap(scale: f32) -> u32 {
    (8.0 * scale).round() as u32
}

/// Sayfa alanını sayfa ve araç paneli olarak böler (fiziksel piksel):
/// `(sayfa genişliği, panelin x'i, panel genişliği)`. Arada sürüklenebilir
/// bir boşluk kalır. `share`: panelin binde payı (kullanıcının ayarı).
pub fn split(x: i32, width: u32, scale: f32, share: u16) -> (u32, i32, u32) {
    let gap = gap(scale);
    let usable = width.saturating_sub(gap);
    // Ne sayfa ne panel çok daralmasın.
    let min_panel = ((280.0 * scale) as u32).min(usable / 2);
    let min_page = ((240.0 * scale) as u32).min(usable / 2);
    let panel = (usable as u64 * share as u64 / 1000) as u32;
    let panel = panel.clamp(min_panel, usable - min_page);
    let page = usable - panel;
    (page, x + (page + gap) as i32, panel)
}

/// Ayraç `mouse_x`'e sürüklenince panelin binde payı.
pub fn share_at(x: i32, width: u32, scale: f32, mouse_x: i32) -> u16 {
    let gap = gap(scale) as i32;
    let usable = (width as i32 - gap).max(1);
    let panel = x + width as i32 - mouse_x - gap / 2;
    (panel.clamp(0, usable) as i64 * 1000 / usable as i64).clamp(100, 900) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_has_the_app() {
        let (html, mime) = file("/devtools_app.html").expect("devtools_app.html");
        assert!(mime.starts_with("text/html"));
        assert!(String::from_utf8_lossy(&html).contains("devtools_app"));
        assert!(file("/entrypoints/devtools_app/devtools_app.js").is_some());
        assert!(file("/core/i18n/locales/tr.json").is_some());
        assert!(file("/licenses/LICENSE").is_some());
        assert!(file("/yok.js").is_none());
        let events = events();
        assert!(events.len() > 100);
        assert!(events.iter().any(|e| e == "Runtime.consoleAPICalled"));
        assert!(events.iter().any(|e| e == "Target.attachedToTarget"));
    }

    #[test]
    fn commands_and_replies() {
        let c =
            Command::parse(r#"{"id":7,"method":"DOM.getDocument","params":{"depth":-1}}"#).unwrap();
        assert_eq!(c.id, 7);
        assert_eq!(c.method, "DOM.getDocument");
        assert_eq!(c.params, r#"{"depth":-1}"#);
        assert_eq!(c.session, None);
        assert_eq!(c.reply(Ok(r#"{"a":1}"#)), r#"{"id":7,"result":{"a":1}}"#);

        let c = Command::parse(r#"{"id":8,"method":"Runtime.enable","sessionId":"S1"}"#).unwrap();
        assert_eq!(c.params, "{}");
        assert_eq!(c.session.as_deref(), Some("S1"));
        assert_eq!(
            c.reply(Err((r#"{"code":-32601,"message":"yok"}"#, "E_FAIL"))),
            r#"{"error":{"code":-32601,"message":"yok"},"id":8,"sessionId":"S1"}"#
        );
        assert_eq!(
            c.reply(Err(("", "E_FAIL"))),
            r#"{"error":{"code":-32000,"message":"E_FAIL"},"id":8,"sessionId":"S1"}"#
        );
        assert!(Command::parse("julie:close").is_none());
        assert!(Command::parse(r#"{"method":"X.y"}"#).is_none());
    }

    #[test]
    fn events_are_wrapped() {
        assert_eq!(
            event("Log.entryAdded", r#"{"x":true}"#, None),
            r#"{"method":"Log.entryAdded","params":{"x":true}}"#
        );
        assert_eq!(
            event("A.b", "{}", Some("S")),
            r#"{"method":"A.b","params":{},"sessionId":"S"}"#
        );
    }

    #[test]
    fn split_keeps_both_usable() {
        // Geniş pencere, varsayılan pay: panel %40.
        assert_eq!(split(10, 2008, 1.0, DEFAULT_WIDTH), (1200, 1218, 800));
        // Kullanıcı genişletti.
        assert_eq!(split(0, 1008, 1.0, 700), (300, 308, 700));
        // Dar pencere: panel en az 280, sayfa en az 240 (ya da yarısı).
        assert_eq!(split(0, 808, 1.0, 100), (520, 528, 280));
        assert_eq!(split(0, 808, 1.0, 900), (240, 248, 560));
        assert_eq!(split(0, 408, 1.0, 400), (200, 208, 200));
    }

    #[test]
    fn dragging_sets_share() {
        // Ayraç ortada: panel yarı yarıya.
        assert_eq!(share_at(0, 1008, 1.0, 504), 500);
        // Sınırların dışına sürüklenemez.
        assert_eq!(share_at(0, 1008, 1.0, 0), 900);
        assert_eq!(share_at(0, 1008, 1.0, 2000), 100);
        // Sürüklenen yer, bölünce ayracın yeri olur.
        let share = share_at(100, 1608, 1.0, 1000);
        let (page, _, _) = split(100, 1608, 1.0, share);
        assert!((100 + page as i32 + 4 - 1000).abs() <= 2);
    }
}
