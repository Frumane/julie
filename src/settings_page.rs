//! Ayarlar sayfası (`julie://settings`): görünüm seçenekleri.

use serde_json::{Value, json};

use crate::start_page::embed_state;
use crate::ui::theme::{Accent, Appearance, Color, ColorMode, Density, LayoutKind, Preset, Tone};

const TEMPLATE: &str = include_str!("settings_page.html");

fn appearance_json(a: &Appearance) -> Value {
    json!({
        "layout": a.layout.id(),
        "mode": a.mode.id(),
        "accent": a.accent.id(),
        "tone": a.tone.id(),
        "density": a.density.id(),
    })
}

/// Sayfanın HTML'ini o anki görünüm ayarlarıyla üretir.
pub fn render(current: &Appearance, system_accent: Color) -> String {
    let notes = |l: LayoutKind| match l {
        LayoutKind::Sidebar => "Sekmeler solda, başlıklarıyla",
        LayoutKind::Rail => "Sayfaya en çok yer",
        LayoutKind::SingleRow => "Adres ve sekmeler yan yana",
        LayoutKind::Classic => "Sekmeler üstte, adres altında",
    };
    let data = json!({
        "current": appearance_json(current),
        "presets": Preset::ALL.iter().map(|p| json!({
            "id": p.id(), "label": p.label(), "appearance": appearance_json(&p.appearance()),
        })).collect::<Vec<_>>(),
        "layouts": LayoutKind::ALL.iter().map(|l| json!({
            "id": l.id(), "label": l.label(), "note": notes(*l),
        })).collect::<Vec<_>>(),
        "modes": ColorMode::ALL.iter().map(|m| json!({ "id": m.id(), "label": m.label() })).collect::<Vec<_>>(),
        "densities": Density::ALL.iter().map(|d| json!({ "id": d.id(), "label": d.label() })).collect::<Vec<_>>(),
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

    #[test]
    fn renders_current_settings_and_options() {
        let a = Preset::Night.appearance();
        let html = render(&a, Color::hex(0x0078d4));
        assert!(!html.contains("/*STATE*/"));
        assert!(html.contains(r#""layout":"rail""#));
        assert!(
            html.contains(r##""color":"#0078d4""##),
            "Windows vurgu rengi örneği"
        );
        for p in Preset::ALL {
            assert!(html.contains(p.label()));
        }
    }
}
