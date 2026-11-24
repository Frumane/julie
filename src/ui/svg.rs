//! Çizim listesini SVG'ye çevirir. Yalnızca testlerde ve önizleme
//! görüntüleri üretmek için kullanılır; Windows'taki asıl çizim `d2d.rs`.

use std::fmt::Write;

use super::scene::{Align, Prim, Seg};

pub fn render(prims: &[Prim], width: f32, height: f32) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="'Segoe UI Variable Text','Segoe UI',Inter,system-ui,sans-serif">"#
    );
    let mut clip_id = 0;
    let mut open_groups = 0;
    for prim in prims {
        match prim {
            Prim::Fill {
                rect,
                radius,
                color,
            } => {
                let _ = write!(
                    out,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{radius}" fill="{}"/>"#,
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    color.css()
                );
            }
            Prim::Border {
                rect,
                radius,
                color,
            } => {
                let _ = write!(
                    out,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{radius}" fill="none" stroke="{}" stroke-width="1"/>"#,
                    rect.x + 0.5,
                    rect.y + 0.5,
                    rect.w - 1.0,
                    rect.h - 1.0,
                    color.css()
                );
            }
            Prim::Favicon { rect, key, color } => match key {
                // Önizlemede gerçek ikon yok: anahtarı olanlar için alan
                // adından türetilmiş renkte bir kare.
                Some(k) => {
                    let hue = k
                        .bytes()
                        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
                        % 360;
                    let _ = write!(
                        out,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="3" fill="hsl({hue},60%,46%)"/>"#,
                        rect.x, rect.y, rect.w, rect.h
                    );
                }
                None => icon(&mut out, super::scene::Icon::Globe, rect, *color),
            },
            Prim::Text {
                rect,
                text,
                size,
                bold,
                color,
                align,
            } => {
                clip_id += 1;
                let (x, anchor) = match align {
                    Align::Left => (rect.x, "start"),
                    Align::Center => (rect.x + rect.w / 2.0, "middle"),
                    Align::Right => (rect.x + rect.w, "end"),
                };
                let _ = write!(
                    out,
                    r#"<clipPath id="t{clip_id}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath><text clip-path="url(#t{clip_id})" x="{x}" y="{}" font-size="{size}" font-weight="{}" fill="{}" text-anchor="{anchor}" dominant-baseline="central">{}</text>"#,
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    rect.y + rect.h / 2.0,
                    if *bold { 600 } else { 400 },
                    color.css(),
                    escape(text)
                );
            }
            Prim::Icon {
                icon: i,
                rect,
                color,
            } => icon(&mut out, *i, rect, *color),
            Prim::Spinner { rect, color } => {
                icon(&mut out, super::scene::Icon::Spinner, rect, *color)
            }
            Prim::PushClip(rect) => {
                clip_id += 1;
                open_groups += 1;
                let _ = write!(
                    out,
                    r#"<clipPath id="c{clip_id}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath><g clip-path="url(#c{clip_id})">"#,
                    rect.x, rect.y, rect.w, rect.h
                );
            }
            Prim::PopClip => {
                if open_groups > 0 {
                    open_groups -= 1;
                    out.push_str("</g>");
                }
            }
        }
    }
    for _ in 0..open_groups {
        out.push_str("</g>");
    }
    out.push_str("</svg>");
    out
}

fn icon(
    out: &mut String,
    icon: super::scene::Icon,
    rect: &super::scene::Rect,
    color: super::theme::Color,
) {
    let shape = icon.shape();
    let k = rect.w / 24.0;
    let _ = write!(
        out,
        r#"<g transform="translate({} {}) scale({k})" stroke="{c}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round" fill="none">"#,
        rect.x,
        rect.y,
        shape.stroke_width,
        c = color.css()
    );
    for path in &shape.fills {
        let _ = write!(
            out,
            r#"<path d="{}" fill="{}" stroke="none"/>"#,
            path_data(path),
            color.css()
        );
    }
    for path in &shape.strokes {
        let _ = write!(out, r#"<path d="{}"/>"#, path_data(path));
    }
    for (cx, cy, r, filled) in &shape.circles {
        let fill = if *filled { color.css() } else { "none".into() };
        let _ = write!(
            out,
            r#"<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}"/>"#
        );
    }
    out.push_str("</g>");
}

fn path_data(segs: &[Seg]) -> String {
    let mut d = String::new();
    for seg in segs {
        let _ = match seg {
            Seg::Move(x, y) => write!(d, "M{x} {y}"),
            Seg::Line(x, y) => write!(d, "L{x} {y}"),
            Seg::Cubic(a, b, c, e, x, y) => write!(d, "C{a} {b} {c} {e} {x} {y}"),
            Seg::Close => write!(d, "Z"),
        };
    }
    d
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::{Pointer, Target, layout, tests::sample_model};
    use crate::ui::theme::{Appearance, Color, ColorMode, Preset, Theme};

    /// Önizleme görüntüleri: `JULIE_UI_PREVIEW=<klasör> cargo test ui_previews -- --ignored`
    #[test]
    #[ignore]
    fn ui_previews() {
        let dir =
            std::path::PathBuf::from(std::env::var("JULIE_UI_PREVIEW").expect("JULIE_UI_PREVIEW"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut model = sample_model();
        // İlk sekme sabitlenmiş görünsün.
        model.tabs[0].pinned = true;
        // Bir sekme yükleniyor; öndeki de yükleniyormuş gibi "Durdur" görünsün.
        model.tabs[2].loading = true;
        model.loading = true;
        let pointer = Pointer {
            hover: Some(Target::Tab(1)),
            pressed: None,
            drag: None,
        };
        for preset in Preset::ALL {
            // Hazır temanın kendi renk modunda (Windows açık temadayken).
            let a = preset.appearance();
            let theme = Theme::new(&a, false, Color::hex(0x0078d4));
            let l = layout(a.layout, &theme, &model, 1280.0, 800.0, pointer);
            std::fs::write(
                dir.join(format!("{}.svg", preset.id())),
                render(&l.prims, 1280.0, 800.0),
            )
            .unwrap();
            for dark in [false, true] {
                let a = Appearance {
                    mode: if dark {
                        ColorMode::Dark
                    } else {
                        ColorMode::Light
                    },
                    ..preset.appearance()
                };
                let theme = Theme::new(&a, dark, Color::hex(0x0078d4));
                let l = layout(a.layout, &theme, &model, 1280.0, 800.0, pointer);
                let name = format!("{}-{}.svg", preset.id(), if dark { "koyu" } else { "acik" });
                std::fs::write(dir.join(name), render(&l.prims, 1280.0, 800.0)).unwrap();
            }
        }
    }

    /// Bütün ikonları büyük boyutta çizer:
    /// `JULIE_ICON_PREVIEW=<dosya.svg> cargo test icon_sheet -- --ignored`
    #[test]
    #[ignore]
    fn icon_sheet() {
        use crate::ui::scene::{Icon, Rect};
        let path = std::env::var("JULIE_ICON_PREVIEW").expect("JULIE_ICON_PREVIEW");
        let icons = [
            Icon::Back,
            Icon::Forward,
            Icon::Reload,
            Icon::Plus,
            Icon::Close,
            Icon::Star,
            Icon::StarFilled,
            Icon::Shield,
            Icon::Clock,
            Icon::Settings,
            Icon::Speaker,
            Icon::Globe,
            Icon::Search,
            Icon::ChevronUp,
            Icon::ChevronDown,
            Icon::Lock,
            Icon::Warning,
            Icon::Camera,
            Icon::Mic,
            Icon::Location,
            Icon::Bell,
        ];
        let mut prims = vec![Prim::Fill {
            rect: Rect::new(0.0, 0.0, 1120.0, 200.0),
            radius: 0.0,
            color: Color::hex(0xffffff),
        }];
        for (i, icon) in icons.iter().enumerate() {
            prims.push(Prim::Icon {
                icon: *icon,
                rect: Rect::new(10.0 + i as f32 * 78.0, 20.0, 64.0, 64.0),
                color: Color::hex(0x1f1d2b),
            });
            prims.push(Prim::Icon {
                icon: *icon,
                rect: Rect::new(30.0 + i as f32 * 78.0, 120.0, 18.0, 18.0),
                color: Color::hex(0x4a4760),
            });
        }
        std::fs::write(path, render(&prims, 1120.0, 200.0)).unwrap();
    }

    /// Adres çubuğu öneri listesini açık ve koyu temada çizer:
    /// `JULIE_SUGGEST_PREVIEW=<dizin> cargo test suggestion_preview -- --ignored`
    #[test]
    #[ignore]
    fn suggestion_preview() {
        use crate::ui::layout::{SuggestionRow, suggestions};
        let dir = std::path::PathBuf::from(
            std::env::var("JULIE_SUGGEST_PREVIEW").expect("JULIE_SUGGEST_PREVIEW"),
        );
        let row = |search: bool, icon: Option<&str>, title: &str, detail: &str| SuggestionRow {
            search,
            icon: icon.map(String::from),
            title: title.into(),
            detail: detail.into(),
        };
        let rows = [
            row(true, None, "you", "Google ile ara"),
            row(false, Some("youtube.com"), "YouTube", "youtube.com"),
            row(
                false,
                Some("youtube.com"),
                "Rust öğreniyorum: 1. bölüm – YouTube",
                "youtube.com/watch?v=abc",
            ),
            row(false, Some("news.site"), "Your daily news", "news.site"),
        ];
        for dark in [false, true] {
            let a = Appearance {
                mode: if dark {
                    ColorMode::Dark
                } else {
                    ColorMode::Light
                },
                ..Appearance::default()
            };
            let theme = Theme::new(&a, dark, Color::hex(0x0078d4));
            let p = suggestions(&theme, &rows, 0, Some(2), 640.0);
            let name = if dark {
                "oneriler-koyu.svg"
            } else {
                "oneriler-acik.svg"
            };
            std::fs::write(dir.join(name), render(&p.prims, 640.0, p.height)).unwrap();
        }
    }

    /// Sayfada bul kutusunu temalarda çizer:
    /// `JULIE_FIND_PREVIEW=<dizin> cargo test find_preview -- --ignored`
    #[test]
    #[ignore]
    fn find_preview() {
        use crate::ui::layout::{FIND_H, FIND_W, FindState, FindTarget, find_bar};
        let dir = std::path::PathBuf::from(
            std::env::var("JULIE_FIND_PREVIEW").expect("JULIE_FIND_PREVIEW"),
        );
        for preset in Preset::ALL {
            let a = preset.appearance();
            let dark = a.mode == ColorMode::Dark;
            let theme = Theme::new(&a, dark, Color::hex(0x0078d4));
            for (name, state) in [
                (
                    "bulunan",
                    FindState {
                        active: 3,
                        total: 12,
                        has_text: true,
                        hover: Some(FindTarget::Next),
                        ..FindState::default()
                    },
                ),
                (
                    "yok",
                    FindState {
                        has_text: true,
                        case_sensitive: true,
                        ..FindState::default()
                    },
                ),
            ] {
                let bar = find_bar(&theme, &state);
                let file = format!("bul-{}-{name}.svg", preset.id());
                std::fs::write(dir.join(file), render(&bar.prims, FIND_W, FIND_H)).unwrap();
            }
        }
    }

    /// Site bilgisi panelini temalarda çizer:
    /// `JULIE_SITE_PREVIEW=<dizin> cargo test site_preview -- --ignored`
    #[test]
    #[ignore]
    fn site_preview() {
        use crate::ui::layout::{PermState, SITE_W, Security, SiteInfo, site_panel};
        use crate::ui::scene::Icon;
        let dir = std::path::PathBuf::from(
            std::env::var("JULIE_SITE_PREVIEW").expect("JULIE_SITE_PREVIEW"),
        );
        let perms = vec![
            ("Kamera".to_string(), PermState::Allow),
            ("Mikrofon".to_string(), PermState::Ask),
            ("Konum".to_string(), PermState::Block),
            ("Bildirimler".to_string(), PermState::Ask),
        ];
        for preset in Preset::ALL {
            let a = preset.appearance();
            let dark = a.mode == ColorMode::Dark;
            let theme = Theme::new(&a, dark, Color::hex(0x0078d4));
            for (name, security) in [
                ("guvenli", Security::Secure),
                ("sifresiz", Security::Insecure),
                ("sertifika", Security::Danger),
            ] {
                let info = SiteInfo {
                    host: "www.youtube.com".into(),
                    security,
                    permissions: perms.clone(),
                    certificate: security != Security::Insecure,
                };
                let panel = site_panel(&theme, &info, Some(1));
                let file = format!("site-{}-{name}.svg", preset.id());
                std::fs::write(dir.join(file), render(&panel.prims, SITE_W, panel.height)).unwrap();
            }
            for (name, text, icon) in [
                ("kamera", "Kameranı kullanmak istiyor", Icon::Camera),
                ("mikrofon", "Mikrofonunu kullanmak istiyor", Icon::Mic),
                ("konum", "Konumunu öğrenmek istiyor", Icon::Location),
                ("bildirim", "Bildirim göndermek istiyor", Icon::Bell),
            ] {
                let info = crate::ui::layout::AskInfo {
                    host: "meet.google.com".into(),
                    text: text.into(),
                    icon,
                };
                let hover = (name == "kamera").then_some(crate::ui::layout::AskTarget::Allow);
                let panel = crate::ui::layout::ask_panel(&theme, &info, hover);
                let file = format!("ask-{}-{name}.svg", preset.id());
                let svg = render(&panel.prims, crate::ui::layout::ASK_W, panel.height);
                std::fs::write(dir.join(file), svg).unwrap();
            }
        }
    }

    /// İndirme panelini temalarda çizer:
    /// `JULIE_DL_PREVIEW=<dizin> cargo test downloads_preview -- --ignored`
    #[test]
    #[ignore]
    fn downloads_preview() {
        use crate::ui::layout::{DOWNLOADS_W, PanelClick, downloads_panel};
        let dir =
            std::path::PathBuf::from(std::env::var("JULIE_DL_PREVIEW").expect("JULIE_DL_PREVIEW"));
        let rows = crate::ui::layout::tests::sample_rows();
        for preset in Preset::ALL {
            let a = preset.appearance();
            let dark = a.mode == ColorMode::Dark;
            let theme = Theme::new(&a, dark, Color::hex(0x0078d4));
            let hover = Some(PanelClick::Primary(1).code());
            let panel = downloads_panel(&theme, &rows, hover);
            let entries = vec![
                crate::ui::layout::MenuEntry::item("Yeni sekme", Some("Ctrl+T"), true),
                crate::ui::layout::MenuEntry::Separator,
                crate::ui::layout::MenuEntry::item("Yeniden yükle", Some("Ctrl+R"), true),
                crate::ui::layout::MenuEntry::item("Çoğalt", None, true),
                crate::ui::layout::MenuEntry::item("Sekmenin sesini kapat", None, true),
                crate::ui::layout::MenuEntry::Separator,
                crate::ui::layout::MenuEntry::item("Kapat", Some("Ctrl+W"), true),
                crate::ui::layout::MenuEntry::item("Diğer sekmeleri kapat", None, true),
                crate::ui::layout::MenuEntry::item("Sağdaki sekmeleri kapat", None, false),
                crate::ui::layout::MenuEntry::Separator,
                crate::ui::layout::MenuEntry::item(
                    "Kapatılan sekmeyi yeniden aç",
                    Some("Ctrl+Shift+T"),
                    true,
                ),
            ];
            let menu = crate::ui::layout::menu(&theme, &entries, Some(3));
            let file = format!("menu-{}.svg", preset.id());
            std::fs::write(
                dir.join(file),
                render(&menu.prims, crate::ui::layout::MENU_W, menu.height),
            )
            .unwrap();
            let file = format!("dl-{}.svg", preset.id());
            std::fs::write(
                dir.join(file),
                render(&panel.prims, DOWNLOADS_W, panel.height),
            )
            .unwrap();
        }
    }

    #[test]
    fn renders_valid_looking_svg() {
        let theme = Theme::new(&Appearance::default(), false, Color::hex(0x0078d4));
        let l = layout(
            Appearance::default().layout,
            &theme,
            &sample_model(),
            1280.0,
            800.0,
            Pointer::default(),
        );
        let svg = render(&l.prims, 1280.0, 800.0);
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
        assert!(svg.contains("Rust (programlama dili)"));
    }
}
