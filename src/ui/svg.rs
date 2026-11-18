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
        let model = sample_model();
        let pointer = Pointer {
            hover: Some(Target::Tab(1)),
            pressed: None,
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
        ];
        let mut prims = vec![Prim::Fill {
            rect: Rect::new(0.0, 0.0, 880.0, 200.0),
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
        std::fs::write(path, render(&prims, 880.0, 200.0)).unwrap();
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
