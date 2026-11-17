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
            Prim::Shadow {
                rect,
                radius,
                color,
            } => {
                let _ = write!(
                    out,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{radius}" fill="{}" style="filter:blur(6px)"/>"#,
                    rect.x,
                    rect.y + 2.0,
                    rect.w,
                    rect.h,
                    color.css()
                );
            }
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
            Prim::Icon { icon, rect, color } => {
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
