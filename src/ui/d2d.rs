//! Çizim listesini Direct2D ve DirectWrite ile pencereye çizer.

use std::collections::HashMap;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LINE_JOIN_ROUND, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateFactory, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::core::{BOOL, HSTRING, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::scene::{Align, Icon, Prim, Rect, Seg};
use super::theme::Color;

/// Bir ikonun 24×24 kutudaki hazır geometrileri.
struct IconGeometry {
    stroke: Option<ID2D1PathGeometry>,
    fill: Option<ID2D1PathGeometry>,
    circles: Vec<(f32, f32, f32, bool)>,
    width: f32,
}

pub struct Renderer {
    hwnd: HWND,
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    stroke_style: ID2D1StrokeStyle,
    font_family: HSTRING,
    target: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    size: (u32, u32),
    formats: HashMap<(u32, bool, bool), IDWriteTextFormat>,
    icons: HashMap<Icon, IconGeometry>,
}

impl Renderer {
    pub fn new(hwnd: HWND) -> Result<Renderer> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let stroke_style = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    ..Default::default()
                },
                None,
            )?;
            let font_family = pick_font(&dwrite);
            Ok(Renderer {
                hwnd,
                factory,
                dwrite,
                stroke_style,
                font_family,
                target: None,
                brush: None,
                size: (0, 0),
                formats: HashMap::new(),
                icons: HashMap::new(),
            })
        }
    }

    /// Listeyi çizer. `width`/`height` fiziksel piksel, `scale` ekran ölçeği.
    pub fn render(&mut self, prims: &[Prim], width: u32, height: u32, scale: f32) -> Result<()> {
        unsafe {
            if self.target.is_none() {
                let target = self.factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES::default(),
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: D2D_SIZE_U { width, height },
                        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                    },
                )?;
                self.brush = Some(target.CreateSolidColorBrush(&d2d_color(Color::hex(0)), None)?);
                self.target = Some(target);
                self.size = (width, height);
            }
            let target = self.target.clone().expect("az önce oluşturuldu");
            if self.size != (width, height) {
                target.Resize(&D2D_SIZE_U { width, height })?;
                self.size = (width, height);
            }
            target.SetDpi(96.0 * scale, 96.0 * scale);

            target.BeginDraw();
            target.SetTransform(&Matrix3x2::identity());
            for prim in prims {
                self.draw(&target, prim)?;
            }
            match target.EndDraw(None, None) {
                // Ekran kartı sıfırlandı (uyku, sürücü güncellemesi):
                // kaynakları bırak, sonraki çizimde yeniden oluşturulur.
                Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
                    self.target = None;
                    self.brush = None;
                    Ok(())
                }
                other => other,
            }
        }
    }

    unsafe fn draw(&mut self, target: &ID2D1HwndRenderTarget, prim: &Prim) -> Result<()> {
        let brush = self.brush.clone().expect("hedefle birlikte oluşturulur");
        unsafe {
            match prim {
                Prim::Fill {
                    rect,
                    radius,
                    color,
                } => {
                    brush.SetColor(&d2d_color(*color));
                    if *radius > 0.0 {
                        target.FillRoundedRectangle(&rounded(*rect, *radius), &brush);
                    } else {
                        target.FillRectangle(&d2d_rect(*rect), &brush);
                    }
                }
                Prim::Shadow {
                    rect,
                    radius,
                    color,
                } => {
                    // Bulanıklık yerine birkaç saydam katman: ucuz ve yeterince yumuşak.
                    for (spread, alpha) in [(3.0, 0.25), (2.0, 0.35), (1.0, 0.5)] {
                        let r = Rect::new(
                            rect.x - spread,
                            rect.y - spread + 1.5,
                            rect.w + 2.0 * spread,
                            rect.h + 2.0 * spread,
                        );
                        brush.SetColor(&d2d_color(color.alpha(color.a * alpha)));
                        target.FillRoundedRectangle(&rounded(r, radius + spread), &brush);
                    }
                }
                Prim::Text {
                    rect,
                    text,
                    size,
                    bold,
                    color,
                    align,
                } => {
                    if rect.w <= 1.0 || text.is_empty() {
                        return Ok(());
                    }
                    let format = self.format(*size, *bold, *align == Align::Center)?;
                    brush.SetColor(&d2d_color(*color));
                    let utf16: Vec<u16> = text.encode_utf16().collect();
                    target.DrawText(
                        &utf16,
                        &format,
                        &d2d_rect(*rect),
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_CLIP,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                Prim::Icon { icon, rect, color } => {
                    self.ensure_icon(*icon)?;
                    let geo = &self.icons[icon];
                    let k = rect.w / 24.0;
                    target.SetTransform(&Matrix3x2 {
                        M11: k,
                        M12: 0.0,
                        M21: 0.0,
                        M22: k,
                        M31: rect.x,
                        M32: rect.y,
                    });
                    brush.SetColor(&d2d_color(*color));
                    if let Some(fill) = &geo.fill {
                        target.FillGeometry(fill, &brush, None);
                    }
                    if let Some(stroke) = &geo.stroke {
                        target.DrawGeometry(stroke, &brush, geo.width, &self.stroke_style);
                    }
                    for &(cx, cy, r, filled) in &geo.circles {
                        let e = D2D1_ELLIPSE {
                            point: Vector2 { X: cx, Y: cy },
                            radiusX: r,
                            radiusY: r,
                        };
                        if filled {
                            target.FillEllipse(&e, &brush);
                        } else {
                            target.DrawEllipse(&e, &brush, geo.width, &self.stroke_style);
                        }
                    }
                    target.SetTransform(&Matrix3x2::identity());
                }
                Prim::PushClip(rect) => {
                    target.PushAxisAlignedClip(&d2d_rect(*rect), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                }
                Prim::PopClip => target.PopAxisAlignedClip(),
            }
        }
        Ok(())
    }

    /// Yazı biçimi: tek satır, dikeyde ortalı, sığmazsa "…" ile kesilir.
    fn format(&mut self, size: f32, bold: bool, center: bool) -> Result<IDWriteTextFormat> {
        let key = ((size * 100.0) as u32, bold, center);
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        unsafe {
            let weight = if bold {
                DWRITE_FONT_WEIGHT_SEMI_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };
            let f = self.dwrite.CreateTextFormat(
                &self.font_family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("tr-tr"),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetTextAlignment(if center {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            })?;
            let ellipsis = self.dwrite.CreateEllipsisTrimmingSign(&f)?;
            f.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                },
                &ellipsis,
            )?;
            self.formats.insert(key, f.clone());
            Ok(f)
        }
    }

    fn ensure_icon(&mut self, icon: Icon) -> Result<()> {
        if self.icons.contains_key(&icon) {
            return Ok(());
        }
        let shape = icon.shape();
        let geo = IconGeometry {
            stroke: self.path(&shape.strokes, false)?,
            fill: self.path(&shape.fills, true)?,
            circles: shape.circles,
            width: shape.stroke_width,
        };
        self.icons.insert(icon, geo);
        Ok(())
    }

    fn path(&self, paths: &[Vec<Seg>], filled: bool) -> Result<Option<ID2D1PathGeometry>> {
        if paths.is_empty() {
            return Ok(None);
        }
        unsafe {
            let geo = self.factory.CreatePathGeometry()?;
            let sink = geo.Open()?;
            let begin = if filled {
                D2D1_FIGURE_BEGIN_FILLED
            } else {
                D2D1_FIGURE_BEGIN_HOLLOW
            };
            for segs in paths {
                let mut open = false;
                for seg in segs {
                    match *seg {
                        Seg::Move(x, y) => {
                            if open {
                                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            }
                            sink.BeginFigure(Vector2 { X: x, Y: y }, begin);
                            open = true;
                        }
                        Seg::Line(x, y) => sink.AddLine(Vector2 { X: x, Y: y }),
                        Seg::Cubic(a, b, c, d, x, y) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: Vector2 { X: a, Y: b },
                            point2: Vector2 { X: c, Y: d },
                            point3: Vector2 { X: x, Y: y },
                        }),
                        Seg::Close => {
                            if open {
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                open = false;
                            }
                        }
                    }
                }
                if open {
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
            }
            sink.Close()?;
            Ok(Some(geo))
        }
    }
}

/// Windows 11'in yazı tipi varsa onu, yoksa Segoe UI'ı kullan.
fn pick_font(dwrite: &IDWriteFactory) -> HSTRING {
    let preferred = HSTRING::from("Segoe UI Variable Text");
    unsafe {
        let mut collection = None;
        if dwrite
            .GetSystemFontCollection(&mut collection, false)
            .is_ok()
            && let Some(collection) = collection
        {
            let mut index = 0u32;
            let mut exists = BOOL::default();
            if collection
                .FindFamilyName(&preferred, &mut index, &mut exists)
                .is_ok()
                && exists.as_bool()
            {
                return preferred;
            }
        }
    }
    HSTRING::from("Segoe UI")
}

fn d2d_color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

fn d2d_rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: d2d_rect(r),
        radiusX: radius,
        radiusY: radius,
    }
}
