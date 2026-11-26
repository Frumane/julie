//! Klavye kısayolları.
//!
//! Tuşlar iki yerden gelir: sayfa odaktayken WebView2'den, adres kutusu
//! odaktayken Win32'den. İkisi de buradaki tek tabloyu kullanır.

use crate::UserEvent;

/// Kullandığımız Windows sanal tuş kodları (VK_*).
pub mod vk {
    pub const TAB: u32 = 0x09;
    pub const F3: u32 = 0x72;
    pub const F4: u32 = 0x73;
    pub const F: u32 = 0x46;
    pub const F5: u32 = 0x74;
    pub const F6: u32 = 0x75;
    pub const F12: u32 = 0x7B;
    pub const I: u32 = 0x49;
    pub const LEFT: u32 = 0x25;
    pub const RIGHT: u32 = 0x27;
    pub const PRIOR: u32 = 0x21; // Page Up
    pub const NEXT: u32 = 0x22; // Page Down
    pub const KEY_1: u32 = 0x31;
    pub const KEY_8: u32 = 0x38;
    pub const KEY_9: u32 = 0x39;
    pub const D: u32 = 0x44;
    pub const H: u32 = 0x48;
    pub const J: u32 = 0x4A;
    pub const COMMA: u32 = 0xBC;
    pub const L: u32 = 0x4C;
    pub const N: u32 = 0x4E;
    pub const R: u32 = 0x52;
    pub const S: u32 = 0x53;
    pub const T: u32 = 0x54;
    pub const W: u32 = 0x57;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Basılan tuşa karşılık gelen tarayıcı komutu.
pub fn lookup(key: u32, m: Modifiers) -> Option<UserEvent> {
    use vk::*;
    let Modifiers { ctrl, shift, alt } = m;
    Some(match key {
        // Sekmeler
        T if ctrl && shift && !alt => UserEvent::ReopenTab,
        N if ctrl && shift && !alt => UserEvent::NewPrivateWindow,
        F if ctrl && !shift && !alt => UserEvent::Find,
        F3 if !ctrl && !alt => {
            if shift {
                UserEvent::FindPrev
            } else {
                UserEvent::FindNext
            }
        }
        T if ctrl && !shift && !alt => UserEvent::NewTab,
        W | F4 if ctrl && !shift && !alt => UserEvent::CloseTab,
        TAB if ctrl && !alt => {
            if shift {
                UserEvent::PrevTab
            } else {
                UserEvent::NextTab
            }
        }
        NEXT if ctrl && !shift && !alt => UserEvent::NextTab,
        PRIOR if ctrl && !shift && !alt => UserEvent::PrevTab,
        KEY_1..=KEY_8 if ctrl && !shift && !alt => UserEvent::SelectTab((key - KEY_1) as usize),
        KEY_9 if ctrl && !shift && !alt => UserEvent::LastTab,

        // Adres çubuğu
        L if ctrl && !shift && !alt => UserEvent::FocusAddress,
        D if alt && !ctrl && !shift => UserEvent::FocusAddress,
        F6 if !ctrl && !shift && !alt => UserEvent::FocusAddress,

        // Yer imleri ve geçmiş
        D if ctrl && !shift && !alt => UserEvent::ToggleBookmark,
        H if ctrl && !shift && !alt => UserEvent::OpenHistory,
        J if ctrl && !shift && !alt => UserEvent::OpenDownloads,
        COMMA if ctrl && !shift && !alt => UserEvent::OpenSettings,

        // Ekran görüntüsü (görünen alan)
        S if ctrl && shift && !alt => UserEvent::Screenshot(None, false),

        // Geliştirici araçları
        F12 if !ctrl && !shift && !alt => UserEvent::DevTools,
        I if ctrl && shift && !alt => UserEvent::DevTools,

        // Gezinme
        F5 if !shift && !alt => UserEvent::Reload,
        R if ctrl && !shift && !alt => UserEvent::Reload,
        LEFT if alt && !ctrl && !shift => UserEvent::Back,
        RIGHT if alt && !ctrl && !shift => UserEvent::Forward,

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    const CTRL_SHIFT: Modifiers = Modifiers {
        ctrl: true,
        shift: true,
        alt: false,
    };
    const ALT: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: true,
    };

    fn is(key: u32, m: Modifiers, expected: UserEvent) {
        let got = lookup(key, m);
        assert_eq!(
            format!("{got:?}"),
            format!("{:?}", Some(expected)),
            "tuş {key:#x} {m:?}"
        );
    }

    #[test]
    fn tab_shortcuts() {
        is(vk::T, CTRL, UserEvent::NewTab);
        is(vk::T, CTRL_SHIFT, UserEvent::ReopenTab);
        is(vk::W, CTRL, UserEvent::CloseTab);
        is(vk::F4, CTRL, UserEvent::CloseTab);
        is(vk::TAB, CTRL, UserEvent::NextTab);
        is(vk::TAB, CTRL_SHIFT, UserEvent::PrevTab);
        is(vk::NEXT, CTRL, UserEvent::NextTab);
        is(vk::PRIOR, CTRL, UserEvent::PrevTab);
        is(vk::KEY_1, CTRL, UserEvent::SelectTab(0));
        is(vk::KEY_8, CTRL, UserEvent::SelectTab(7));
        is(vk::KEY_9, CTRL, UserEvent::LastTab);
    }

    #[test]
    fn address_and_navigation() {
        is(vk::L, CTRL, UserEvent::FocusAddress);
        is(vk::D, ALT, UserEvent::FocusAddress);
        is(vk::F6, NONE, UserEvent::FocusAddress);
        is(vk::F5, NONE, UserEvent::Reload);
        is(vk::F5, CTRL, UserEvent::Reload);
        is(vk::R, CTRL, UserEvent::Reload);
        is(vk::LEFT, ALT, UserEvent::Back);
        is(vk::RIGHT, ALT, UserEvent::Forward);
        is(vk::F12, NONE, UserEvent::DevTools);
        is(vk::D, CTRL, UserEvent::ToggleBookmark);
        is(vk::H, CTRL, UserEvent::OpenHistory);
        is(vk::J, CTRL, UserEvent::OpenDownloads);
        is(vk::COMMA, CTRL, UserEvent::OpenSettings);
        is(vk::I, CTRL_SHIFT, UserEvent::DevTools);
        is(vk::S, CTRL_SHIFT, UserEvent::Screenshot(None, false));
    }

    #[test]
    fn ordinary_keys_pass_through() {
        assert!(lookup(vk::T, NONE).is_none());
        assert!(lookup(vk::TAB, NONE).is_none());
        assert!(lookup(vk::LEFT, NONE).is_none());
        assert!(lookup(vk::D, CTRL_SHIFT).is_none());
        assert!(lookup(0x43, CTRL).is_none()); // Ctrl+C sayfaya kalır
    }

    #[test]
    fn private_window() {
        let m = Modifiers {
            ctrl: true,
            shift: true,
            alt: false,
        };
        is(vk::N, m, UserEvent::NewPrivateWindow);
        let plain = Modifiers {
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert!(lookup(vk::N, plain).is_none());
    }

    #[test]
    fn find_in_page() {
        let ctrl = Modifiers {
            ctrl: true,
            shift: false,
            alt: false,
        };
        let shift = Modifiers {
            ctrl: false,
            shift: true,
            alt: false,
        };
        is(vk::F, ctrl, UserEvent::Find);
        is(vk::F3, NONE, UserEvent::FindNext);
        is(vk::F3, shift, UserEvent::FindPrev);
    }
}
