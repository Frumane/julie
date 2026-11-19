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

    /// Ayardaki süreye göre zamanlama; `None`: sekmeler hiç uyutulmaz.
    /// `JULIE_FAST_SLEEP` ortam değişkeni ayarı geçersiz kılar (deneme için).
    pub fn for_setting(after: SleepAfter) -> Option<Timing> {
        if std::env::var_os("JULIE_FAST_SLEEP").is_some() {
            return Some(Timing::FAST);
        }
        let min = |m: u64| Duration::from_secs(m * 60);
        let (suspend, discard) = match after {
            SleepAfter::Never => return None,
            SleepAfter::OneMinute => (1, 10),
            SleepAfter::FiveMinutes => (5, 30),
            SleepAfter::FifteenMinutes => (15, 60),
        };
        Some(Timing {
            suspend_after: min(suspend),
            discard_after: min(discard),
            ..Timing::NORMAL
        })
    }
}

/// Arka plandaki sekmeler ne zaman uyutulsun (ayar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SleepAfter {
    OneMinute,
    #[default]
    FiveMinutes,
    FifteenMinutes,
    Never,
}

impl SleepAfter {
    pub const ALL: [SleepAfter; 4] = [
        Self::OneMinute,
        Self::FiveMinutes,
        Self::FifteenMinutes,
        Self::Never,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::OneMinute => "1",
            Self::FiveMinutes => "5",
            Self::FifteenMinutes => "15",
            Self::Never => "never",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "1 dakika",
            Self::FiveMinutes => "5 dakika",
            Self::FifteenMinutes => "15 dakika",
            Self::Never => "Hiç",
        }
    }

    pub fn from_id(id: &str) -> Option<SleepAfter> {
        Self::ALL.into_iter().find(|s| s.id() == id.trim())
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
    fn setting_controls_timing() {
        // Ortam değişkeni testi bozmasın diye yalnızca o yokken bak.
        if std::env::var_os("JULIE_FAST_SLEEP").is_some() {
            return;
        }
        assert!(Timing::for_setting(SleepAfter::Never).is_none());
        let one = Timing::for_setting(SleepAfter::OneMinute).unwrap();
        assert_eq!(one.suspend_after, MIN);
        assert_eq!(one.discard_after, 10 * MIN);
        let default = Timing::for_setting(SleepAfter::default()).unwrap();
        assert_eq!(default.suspend_after, Timing::NORMAL.suspend_after);
        assert_eq!(default.discard_after, Timing::NORMAL.discard_after);
        for s in SleepAfter::ALL {
            assert_eq!(SleepAfter::from_id(s.id()), Some(s));
        }
    }

    #[test]
    fn start_pages_are_discarded_early() {
        assert_eq!(
            decide(&Timing::NORMAL, Power::Awake, 6 * MIN, false, true),
            Some(Action::Discard)
        );
    }
}
