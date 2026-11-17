//! Sekme uyutma politikası.
//!
//! Arka plandaki sekmeler iki aşamada uyutulur:
//! 1. **Askıya alma** (`SUSPEND_AFTER` sonra): WebView2 sayfayı dondurur;
//!    betikler ve zamanlayıcılar durur, CPU kullanımı sıfıra iner. Sayfa
//!    bellekte kalır, geri dönünce anında açılır.
//! 2. **Bellekten atma** (`DISCARD_AFTER` sonra): Sayfa tamamen kapatılır,
//!    yalnızca adresi ve başlığı tutulur. Sekmeye tıklanınca yeniden yüklenir.
//!
//! Ses ya da video çalan sekmelere dokunulmaz.
//!
//! Denemek için `JULIE_FAST_SLEEP=1` ortam değişkeniyle başlatın: süreler
//! 20 saniye / 1 dakikaya iner.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub suspend_after: Duration,
    pub discard_after: Duration,
    /// Uyutma kontrolünün ne sıklıkla yapılacağı.
    pub check_every: Duration,
}

impl Timing {
    pub const NORMAL: Timing = Timing {
        suspend_after: Duration::from_secs(5 * 60),
        discard_after: Duration::from_secs(30 * 60),
        check_every: Duration::from_secs(30),
    };

    pub const FAST: Timing = Timing {
        suspend_after: Duration::from_secs(20),
        discard_after: Duration::from_secs(60),
        check_every: Duration::from_secs(5),
    };

    pub fn from_env() -> Timing {
        if std::env::var_os("JULIE_FAST_SLEEP").is_some() {
            Timing::FAST
        } else {
            Timing::NORMAL
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    /// Normal çalışıyor (gizli olabilir ama çalışıyor).
    Awake,
    /// WebView2 tarafından donduruldu.
    Suspended,
    /// Bellekten atıldı; WebView yok.
    Discarded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Suspend,
    Discard,
}

/// Arka plandaki bir sekmeye ne yapılmalı?
///
/// - `hidden_for`: sekme ne kadar süredir arka planda,
/// - `playing_audio`: sekmede ses çalıyor mu,
/// - `is_start_page`: başlangıç sayfası mı (yeniden oluşturması bedava).
pub fn decide(
    timing: &Timing,
    power: Power,
    hidden_for: Duration,
    playing_audio: bool,
    is_start_page: bool,
) -> Option<Action> {
    if playing_audio || power == Power::Discarded {
        return None;
    }
    // Başlangıç sayfası hiçbir şey kaybetmeden yeniden açılabilir;
    // dondurmakla uğraşmadan doğrudan bellekten at.
    if is_start_page && hidden_for >= timing.suspend_after {
        return Some(Action::Discard);
    }
    if hidden_for >= timing.discard_after {
        return Some(Action::Discard);
    }
    if hidden_for >= timing.suspend_after && power == Power::Awake {
        return Some(Action::Suspend);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn recently_hidden_tabs_stay_awake() {
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, MIN, false, false),
            None
        );
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, MIN, false, true),
            None
        );
    }

    #[test]
    fn suspends_then_discards() {
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, 6 * MIN, false, false),
            Some(Action::Suspend)
        );
        assert_eq!(
            decide(&Timing::NORMAL, Power::Suspended, 6 * MIN, false, false),
            None
        );
        assert_eq!(
            decide(&Timing::NORMAL, Power::Suspended, 31 * MIN, false, false),
            Some(Action::Discard)
        );
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, 31 * MIN, false, false),
            Some(Action::Discard)
        );
        assert_eq!(
            decide(&Timing::NORMAL, Power::Discarded, 60 * MIN, false, false),
            None
        );
    }

    #[test]
    fn never_touches_tabs_playing_audio() {
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, 60 * MIN, true, false),
            None
        );
        assert_eq!(
            decide(&Timing::NORMAL, Power::Suspended, 60 * MIN, true, false),
            None
        );
    }

    #[test]
    fn start_pages_are_discarded_early() {
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, 6 * MIN, false, true),
            Some(Action::Discard)
        );
    }
}
