//! Ayarlar sayfası (`julie://settings`): görünüm seçenekleri.

use serde_json::{Value, json};

use crate::privacy::TrackingLevel;
use crate::settings::Engine;
use crate::sleep::SleepAfter;
use crate::start_page::{State, embed_state};
use crate::ui::theme::{
    Accent, Appearance, ColorMode, Density, FindCorner, LayoutKind, Preset, Tone,
};

const TEMPLATE: &str = include_str!("settings_page.html");

fn appearance_json(a: &Appearance) -> Value {
    json!({
        "layout": a.layout.id(),
        "mode": a.mode.id(),
        "accent": a.accent.id(),
        "tone": a.tone.id(),
        "density": a.density.id(),
        "find": a.find.id(),
    })
}

/// Sayfanın HTML'ini o anki ayarlarla üretir.
pub fn render(state: &State) -> String {
    let current = &state.appearance;
    let system_accent = state.system_accent;
    let notes = |l: LayoutKind| match l {
        LayoutKind::Sidebar => "Sekmeler solda, başlıklarıyla",
        LayoutKind::Rail => "Sayfaya en çok yer",
        LayoutKind::SingleRow => "Adres ve sekmeler yan yana",
        LayoutKind::Classic => "Sekmeler üstte, adres altında",
    };
    let data = json!({
        "current": appearance_json(current),
        "closeWithLastTab": state.close_with_last_tab,
        "tabSleep": state.tab_sleep.id(),
        "gpu": state.gpu,
        "gpuRunning": state.gpu_running,
        "tabSleepOptions": SleepAfter::ALL.iter().map(|s| json!({ "id": s.id(), "label": s.label() })).collect::<Vec<_>>(),
        "smartscreen": state.smartscreen,
        "savePasswords": state.save_passwords,
        "autofill": state.autofill,
        "pdfLinksNewTab": state.pdf_links_new_tab,
        "searchSuggestions": state.search_suggestions,
        "autocomplete": state.autocomplete,
        "httpsFirst": !state.privacy.https_first_off,
        "stripTracking": !state.privacy.strip_tracking_off,
        "clearOnClose": state.privacy.clear_on_close,
        "hideTimezone": state.privacy.hide_timezone,
        "tracking": state.privacy.tracking.id(),
        "trackingLevels": TrackingLevel::ALL.iter().map(|l| json!({
            "id": l.id(), "label": l.label(), "note": l.note(),
        })).collect::<Vec<_>>(),
        "engine": state.engine.unwrap_or(Engine::FALLBACK).id(),
        "engines": Engine::ALL.iter().map(|e| json!({
            "id": e.id(), "name": e.name(), "note": e.note(),
        })).collect::<Vec<_>>(),
        "presets": Preset::ALL.iter().map(|p| json!({
            "id": p.id(), "label": p.label(), "appearance": appearance_json(&p.appearance()),
        })).collect::<Vec<_>>(),
        "layouts": LayoutKind::ALL.iter().map(|l| json!({
            "id": l.id(), "label": l.label(), "note": notes(*l),
        })).collect::<Vec<_>>(),
        "modes": ColorMode::ALL.iter().map(|m| json!({ "id": m.id(), "label": m.label() })).collect::<Vec<_>>(),
        "densities": Density::ALL.iter().map(|d| json!({ "id": d.id(), "label": d.label() })).collect::<Vec<_>>(),
        "findCorners": FindCorner::ALL.iter().map(|f| json!({ "id": f.id(), "label": f.label() })).collect::<Vec<_>>(),
        "accents": Accent::ALL.iter().map(|a| json!({
            "id": a.id(), "label": a.label(), "color": a.swatch(system_accent).css(),
        })).collect::<Vec<_>>(),
        "tones": Tone::ALL.iter().map(|t| json!({
            "id": t.id(), "label": t.label(), "light": t.swatch(false).css(), "dark": t.swatch(true).css(),
        })).collect::<Vec<_>>(),
    });
    embed_state(TEMPLATE, &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Color;

    #[test]
    fn renders_current_settings_and_options() {
        let a = Preset::Night.appearance();
        let html = render(&State {
            engine: Some(Engine::Ecosia),
            smartscreen: false,
            appearance: a,
            system_accent: Color::hex(0x0078d4),
            theme: crate::ui::theme::Theme::new(&a, false, Color::hex(0x0078d4)),
            close_with_last_tab: true,
            search_suggestions: false,
            autocomplete: true,
            private: false,
            privacy: Default::default(),
            tab_sleep: Default::default(),
            gpu: true,
            gpu_running: true,
            save_passwords: true,
            autofill: true,
            pdf_links_new_tab: true,
        });
        assert!(!html.contains("/*STATE*/"));
        assert!(html.contains(r#""layout":"rail""#));
        assert!(html.contains(r#""closeWithLastTab":true"#));
        assert!(html.contains(r#""smartscreen":false"#));
        assert!(html.contains(r#""engine":"ecosia""#));
        assert!(html.contains(r#""searchSuggestions":false"#));
        assert!(
            html.contains(r##""color":"#0078d4""##),
            "Windows vurgu rengi örneği"
        );
        for p in Preset::ALL {
            assert!(html.contains(p.label()));
        }
    }
}

#[cfg(test)]
mod preview {
    /// `JULIE_SETTINGS_PREVIEW=yol cargo test -- --ignored settings_preview`
    #[test]
    #[ignore]
    fn settings_preview() {
        let Ok(path) = std::env::var("JULIE_SETTINGS_PREVIEW") else {
            return;
        };
        let state = crate::start_page::State {
            engine: None,
            smartscreen: true,
            appearance: Default::default(),
            system_accent: crate::ui::theme::Color::hex(0x0078d4),
            theme: crate::ui::theme::Theme::new(
                &Default::default(),
                false,
                crate::ui::theme::Color::hex(0x0078d4),
            ),
            close_with_last_tab: false,
            search_suggestions: true,
            autocomplete: true,
            private: false,
            privacy: Default::default(),
            tab_sleep: Default::default(),
            gpu: true,
            gpu_running: true,
            save_passwords: true,
            autofill: true,
            pdf_links_new_tab: true,
        };
        std::fs::write(&path, super::render(&state)).unwrap();
        for (name, kind, status) in [
            ("dns", "dns", 13),
            ("cert", "cert", 2),
            ("offline", "offline", 11),
            ("timeout", "timeout", 7),
            ("connect", "connect", 12),
            ("other", "other", 15),
        ] {
            let url = crate::error_page::error_url(kind, status, "https://ornek.example/sayfa");
            std::fs::write(
                format!("{path}.{name}.html"),
                crate::error_page::render(url.split_once('?').map(|(_, q)| q)),
            )
            .unwrap();
        }
        std::fs::write(format!("{path}.fly.html"), crate::error_page::render_game()).unwrap();
        let info = crate::error_page::CertInfo {
            subject: "*.badssl.com".into(),
            issuer: "COMODO RSA Domain Validation Secure Server CA".into(),
            valid_from: 1_428_537_600,
            valid_to: 1_428_883_199,
        };
        let chain: Vec<String> = include_str!("testdata/cert_chain.pem")
            .split_inclusive("-----END CERTIFICATE-----")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let viewer = crate::cert_page::url(&chain);
        std::fs::write(
            format!("{path}.certificate.html"),
            crate::cert_page::render(viewer.split_once('?').map(|(_, q)| q)),
        )
        .unwrap();
        let cert = crate::error_page::cert_url(2, "https://expired.badssl.com/", Some(&info));
        std::fs::write(
            format!("{path}.certinfo.html"),
            crate::error_page::render(cert.split_once('?').map(|(_, q)| q)),
        )
        .unwrap();
        let blocked = crate::error_page::blocked_url("edge://settings");
        std::fs::write(
            format!("{path}.blocked.html"),
            crate::error_page::render(blocked.split_once('?').map(|(_, q)| q)),
        )
        .unwrap();
        let warning = crate::privacy::warning_url("http://neverssl.com/online");
        std::fs::write(
            format!("{path}.insecure.html"),
            crate::privacy::render_warning(warning.split_once('?').map(|(_, q)| q)),
        )
        .unwrap();
    }
}
