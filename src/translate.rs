//! Sayfa çevirisi (Google Çeviri).
//!
//! Sayfadaki betik (`translate.js`) yazıları paketler halinde yollar;
//! arka plandaki iş parçacıkları her paketi Google'a sorar, çeviriler
//! `UserEvent::Translated` olarak gelir ve sayfaya geri verilir. Sayfanın
//! yazıları yalnızca kullanıcı "çevir" dediğinde gönderilir.

/// Sayfaya verilen betik.
pub const SCRIPT: &str = include_str!("translate.js");

/// Çeviri dili.
pub const TARGET: &str = "tr";

/// Bir paketteki en fazla yazı sayısı ve toplam uzunluk (betikle aynı).
pub const MAX_ITEMS: usize = 100;
pub const MAX_CHARS: usize = 4000;

/// Betikten gelen paket: numarası ve yazıları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub batch: u64,
    pub texts: Vec<String>,
}

impl Batch {
    /// `julie-translate:{"batch":1,"texts":[...]}` iletisi. Sınırları aşan
    /// paketler reddedilir.
    pub fn parse(message: &str) -> Option<Batch> {
        let json = message.strip_prefix("julie-translate:")?;
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let batch = value.get("batch")?.as_u64()?;
        let texts: Vec<String> = value
            .get("texts")?
            .as_array()?
            .iter()
            .map(|t| t.as_str().map(str::to_string))
            .collect::<Option<_>>()?;
        let chars: usize = texts.iter().map(|t| t.chars().count()).sum();
        if texts.is_empty() || texts.len() > MAX_ITEMS || chars > MAX_CHARS {
            return None;
        }
        Some(Batch { batch, texts })
    }
}

/// Google'a gönderilen form gövdesi: her yazı bir `q`.
pub fn form_body(texts: &[String]) -> String {
    texts
        .iter()
        .map(|t| format!("q={}", encode(t)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `application/x-www-form-urlencoded` kodlaması.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Google'ın yanıtı: her yazının çevirisi (HTML: `<b i="1">…</b>` ve
/// `&amp;` gibi kaçışlar korunur; betik çözer) ve (varsa) kaynak dili.
/// Yanıt `[["çeviri","en"], …]`, `["çeviri", …]` ya da tek yazı için
/// `"çeviri"` olabilir. Sayı tutmazsa `None`.
pub fn parse_reply(json: &str, count: usize) -> Option<(Vec<String>, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let items = match &value {
        serde_json::Value::String(s) => vec![(s.clone(), None)],
        serde_json::Value::Array(list) => list
            .iter()
            .map(|item| match item {
                serde_json::Value::String(s) => Some((s.clone(), None)),
                serde_json::Value::Array(pair) => Some((
                    pair.first()?.as_str()?.to_string(),
                    pair.get(1).and_then(|l| l.as_str()).map(str::to_string),
                )),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    if items.len() != count {
        return None;
    }
    let lang = items.iter().find_map(|(_, l)| l.clone());
    Some((items.into_iter().map(|(t, _)| t).collect(), lang))
}

/// Çevirileri sayfaya veren betik.
pub fn apply_script(batch: u64, texts: &[String]) -> String {
    let texts = serde_json::to_string(texts).unwrap_or_else(|_| "[]".into());
    format!("window.__julieTranslate && window.__julieTranslate.apply({batch}, {texts});")
}

/// Paket çevrilemedi: betik o yazıları yeniden denenebilir sayar.
pub fn fail_script(batch: u64) -> String {
    format!("window.__julieTranslate && window.__julieTranslate.fail({batch});")
}

/// Çeviriyi bırakıp ilk hale dönen betik.
pub const RESTORE_SCRIPT: &str = "window.__julieTranslate && window.__julieTranslate.restore();";

/// Ağ işleri (yalnızca Windows'ta: ureq orada).
#[cfg(windows)]
pub mod net {
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tao::event_loop::EventLoopProxy;

    use super::{Batch, TARGET};
    use crate::UserEvent;

    /// Aynı anda en fazla bu kadar istek (ekrandaki yazılar hızlı gelsin,
    /// Google'ı da boğmasın).
    const WORKERS: usize = 3;
    const MAX_BODY: u64 = 1024 * 1024;

    /// Bir iş: sekme ve paket.
    pub type Job = (u64, Batch);

    /// İş parçacıklarını başlatır; dönen kanala paketler gönderilir.
    pub fn spawn(proxy: EventLoopProxy<UserEvent>) -> Sender<Job> {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for n in 0..WORKERS {
            let rx = Arc::clone(&rx);
            let proxy = proxy.clone();
            let started = std::thread::Builder::new()
                .name(format!("ceviri-{n}"))
                .spawn(move || {
                    let agent = agent();
                    loop {
                        let job = rx.lock().ok().and_then(|rx| rx.recv().ok());
                        let Some((tab, batch)) = job else { break };
                        let result = translate(&agent, &batch.texts);
                        if let Err(err) = &result {
                            crate::log::error(&format!("çeviri yapılamadı: {err}"));
                        }
                        let event = UserEvent::Translated(tab, batch.batch, result);
                        if proxy.send_event(event).is_err() {
                            break;
                        }
                    }
                });
            if let Err(err) = started {
                crate::log::error(&format!("çeviri başlatılamadı: {err}"));
            }
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
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Julie")
            .build()
            .into()
    }

    /// Yazıları Türkçeye çevirir; çeviriler ve kaynak dil.
    fn translate(
        agent: &ureq::Agent,
        texts: &[String],
    ) -> Result<(Vec<String>, Option<String>), String> {
        let url = format!(
            "https://translate.googleapis.com/translate_a/t?client=gtx&sl=auto&tl={TARGET}&format=html"
        );
        let body = agent
            .post(&url)
            .header(
                "content-type",
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .send(super::form_body(texts))
            .map_err(|e| e.to_string())?
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map_err(|e| e.to_string())?;
        super::parse_reply(&body, texts.len()).ok_or_else(|| "yanıt okunamadı".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches() {
        let b = Batch::parse(r#"julie-translate:{"batch":3,"texts":["Hello","World"]}"#).unwrap();
        assert_eq!(b.batch, 3);
        assert_eq!(b.texts, ["Hello", "World"]);
        assert!(Batch::parse(r#"julie-translate:{"batch":3,"texts":[]}"#).is_none());
        assert!(Batch::parse(r#"julie-translate:{"batch":3,"texts":[1]}"#).is_none());
        assert!(Batch::parse(r#"other:{"batch":3,"texts":["a"]}"#).is_none());
        let long = format!(
            r#"julie-translate:{{"batch":1,"texts":["{}"]}}"#,
            "a".repeat(MAX_CHARS + 1)
        );
        assert!(Batch::parse(&long).is_none());
    }

    #[test]
    fn body() {
        let texts = ["it's a & b".to_string(), "Ünlü çay".to_string()];
        assert_eq!(
            form_body(&texts),
            "q=it%27s+a+%26+b&q=%C3%9Cnl%C3%BC+%C3%A7ay"
        );
    }

    #[test]
    fn replies() {
        let (texts, lang) = parse_reply(
            r#"[["Merhaba <b i=\"1\">dünya</b>","en"],["Ayarlar","en"]]"#,
            2,
        )
        .unwrap();
        assert_eq!(texts, ["Merhaba <b i=\"1\">dünya</b>", "Ayarlar"]);
        assert_eq!(lang.as_deref(), Some("en"));
        assert_eq!(parse_reply(r#""Günaydın""#, 1).unwrap().0, ["Günaydın"]);
        assert_eq!(parse_reply(r#"["a","b"]"#, 2).unwrap().0, ["a", "b"]);
        assert!(parse_reply(r#"["a"]"#, 2).is_none());
        assert!(parse_reply("<html>", 1).is_none());
    }

    #[test]
    fn scripts() {
        let s = apply_script(7, &["a\"</script>".to_string()]);
        assert_eq!(
            s,
            r#"window.__julieTranslate && window.__julieTranslate.apply(7, ["a\"</script>"]);"#
        );
        assert!(fail_script(2).contains("fail(2)"));
    }
}
