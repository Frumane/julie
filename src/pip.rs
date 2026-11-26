//! Resim içinde resim: sayfadaki videoyu her şeyin üstünde duran küçük bir
//! pencerede oynatır (motorun kendi Picture-in-Picture penceresi).
//!
//! Betik sayfada en uygun videoyu seçer: oynayan, ekranda görünen, en büyük.
//! Video zaten küçük penceredeyse geri getirir. Motor bu isteği yalnızca
//! kullanıcı bir şeye tıklamışken kabul ettiği için betik DevTools'un
//! `Runtime.evaluate` komutuyla, `userGesture` ile çalıştırılır.

/// Sayfada çalışan betik. Sonuç: "ok", "exit", "none" ya da "error:…".
pub const SCRIPT: &str = r#"(async () => {
  try {
    if (document.pictureInPictureElement) {
      await document.exitPictureInPicture();
      return "exit";
    }
    if (!document.pictureInPictureEnabled) return "error:disabled";
    const videos = [];
    const walk = (root) => {
      for (const v of root.querySelectorAll("video")) videos.push(v);
      for (const el of root.querySelectorAll("*")) if (el.shadowRoot) walk(el.shadowRoot);
    };
    walk(document);
    const score = (v) => {
      const r = v.getBoundingClientRect();
      const area = Math.max(0, r.width) * Math.max(0, r.height);
      const visible = r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
      const playing = !v.paused && !v.ended && v.readyState > 2;
      return (playing ? 4e9 : 0) + (visible ? 2e9 : 0) + Math.min(area, 1e9);
    };
    const usable = videos.filter((v) => v.readyState > 0 && v.videoWidth > 0);
    if (!usable.length) return "none";
    usable.sort((a, b) => score(b) - score(a));
    const video = usable[0];
    // Bazı siteler küçük pencereyi kapatır; kullanıcı istediği için açılır.
    video.disablePictureInPicture = false;
    video.removeAttribute("disablepictureinpicture");
    await video.requestPictureInPicture();
    return "ok";
  } catch (e) {
    return "error:" + (e && e.name ? e.name + ": " + e.message : String(e));
  }
})()"#;

/// `Runtime.evaluate` parametreleri.
pub fn params() -> String {
    serde_json::json!({
        "expression": SCRIPT,
        "userGesture": true,
        "awaitPromise": true,
        "returnByValue": true,
    })
    .to_string()
}

/// Betiğin sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Video küçük pencerede.
    Opened,
    /// Küçük pencere kapatıldı.
    Closed,
    /// Sayfada oynatılabilir video yok.
    NoVideo,
    /// Motor ya da site izin vermedi.
    Failed(String),
}

/// `Runtime.evaluate` yanıtını okur.
pub fn outcome(reply: &str) -> Outcome {
    let value: serde_json::Value = serde_json::from_str(reply).unwrap_or_default();
    if let Some(details) = value.get("exceptionDetails") {
        let text = details
            .pointer("/exception/description")
            .or_else(|| details.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("bilinmeyen hata");
        return Outcome::Failed(text.to_string());
    }
    match value.pointer("/result/value").and_then(|v| v.as_str()) {
        Some("ok") => Outcome::Opened,
        Some("exit") => Outcome::Closed,
        Some("none") => Outcome::NoVideo,
        Some(other) => Outcome::Failed(other.strip_prefix("error:").unwrap_or(other).to_string()),
        None => Outcome::Failed("yanıt okunamadı".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply() {
        let ok = r#"{"result":{"type":"string","value":"ok"}}"#;
        assert_eq!(outcome(ok), Outcome::Opened);
        assert_eq!(
            outcome(r#"{"result":{"type":"string","value":"exit"}}"#),
            Outcome::Closed
        );
        assert_eq!(
            outcome(r#"{"result":{"type":"string","value":"none"}}"#),
            Outcome::NoVideo
        );
        assert_eq!(
            outcome(r#"{"result":{"type":"string","value":"error:NotAllowedError: no"}}"#),
            Outcome::Failed("NotAllowedError: no".into())
        );
        assert!(matches!(
            outcome(r#"{"exceptionDetails":{"text":"Uncaught"}}"#),
            Outcome::Failed(t) if t == "Uncaught"
        ));
        assert!(matches!(outcome("bozuk"), Outcome::Failed(_)));
    }

    #[test]
    fn params_ask_for_gesture() {
        let p: serde_json::Value = serde_json::from_str(&params()).unwrap();
        assert_eq!(p["userGesture"], true);
        assert_eq!(p["awaitPromise"], true);
        assert!(
            p["expression"]
                .as_str()
                .unwrap()
                .contains("requestPictureInPicture")
        );
    }
}
