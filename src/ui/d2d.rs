//! Çizim listesini Direct2D ve DirectWrite ile pencereye çizer.

use std::collections::HashMap;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, E_FAIL, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
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
    formats: HashMap<(u32, bool, u8, bool), IDWriteTextFormat>,
    /// Windows'un ikon yazı tipi (Windows 11: Segoe Fluent Icons,
    /// Windows 10: Segoe MDL2 Assets). Yoksa vektör ikonlar çizilir.
    icon_font: Option<HSTRING>,
    wic: Option<IWICImagingFactory>,
    /// Çözülmüş site ikonları: anahtar → (sürüm, resim). Resim yoksa ikon
    /// çözülemedi demektir; küre çizilir.
    bitmaps: HashMap<String, (u64, Option<ID2D1Bitmap>)>,
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
            let icon_font = find_family(&dwrite, &["Segoe Fluent Icons", "Segoe MDL2 Assets"]);
            let wic = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok();
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
                icon_font,
                wic,
                bitmaps: HashMap::new(),
                icons: HashMap::new(),
            })
        }
    }

    /// Listeyi çizer. `width`/`height` fiziksel piksel, `scale` ekran ölçeği.
    /// `favicons`: site ikonları (anahtar → sürüm, PNG).
    pub fn render(
        &mut self,
        prims: &[Prim],
        favicons: &HashMap<String, (u64, Vec<u8>)>,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<()> {
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
                // Resimler eski hedefe bağlıydı.
                self.bitmaps.clear();
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
                self.draw(&target, prim, favicons)?;
            }
            match target.EndDraw(None, None) {
                // Ekran kartı sıfırlandı (uyku, sürücü güncellemesi):
                // kaynakları bırak, sonraki çizimde yeniden oluşturulur.
                Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
                    self.target = None;
                    self.brush = None;
                    self.bitmaps.clear();
                    Ok(())
                }
                other => other,
            }
        }
    }

    unsafe fn draw(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        prim: &Prim,
        favicons: &HashMap<String, (u64, Vec<u8>)>,
    ) -> Result<()> {
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
                Prim::Border {
                    rect,
                    radius,
                    color,
                } => {
                    brush.SetColor(&d2d_color(*color));
                    let r = Rect::new(rect.x + 0.5, rect.y + 0.5, rect.w - 1.0, rect.h - 1.0);
                    target.DrawRoundedRectangle(&rounded(r, *radius), &brush, 1.0, None);
                }
                Prim::Favicon { rect, key, color } => {
                    let bitmap = key
                        .as_ref()
                        .and_then(|k| self.favicon_bitmap(target, k, favicons));
                    match bitmap {
                        Some(bitmap) => target.DrawBitmap(
                            &bitmap,
                            Some(&d2d_rect(*rect)),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        ),
                        None => self.draw_icon(target, &brush, Icon::Globe, *rect, *color)?,
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
                    let format = self.format(*size, *bold, *align)?;
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
                    self.draw_icon(target, &brush, *icon, *rect, *color)?;
                }
                Prim::Spinner { rect, color } => {
                    self.draw_spinner(target, &brush, *rect, *color)?;
                }
                Prim::PushClip(rect) => {
                    target.PushAxisAlignedClip(&d2d_rect(*rect), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                }
                Prim::PopClip => target.PopAxisAlignedClip(),
            }
        }
        Ok(())
    }

    /// İkonu Windows'un ikon yazı tipiyle, yoksa vektör olarak çizer.
    unsafe fn draw_icon(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        brush: &ID2D1SolidColorBrush,
        icon: Icon,
        rect: Rect,
        color: Color,
    ) -> Result<()> {
        unsafe {
            brush.SetColor(&d2d_color(color));
            if self.icon_font.is_some()
                && let Some(glyph) = fluent_glyph(icon)
            {
                let format = self.icon_format(rect.w)?;
                let utf16: Vec<u16> = glyph.encode_utf16(&mut [0; 2]).to_vec();
                // Yazı tipi ikonları satır yüksekliğine göre konumlanır;
                // kutunun biraz dışına taşacak bir alanda ortalanır.
                let r = Rect::new(rect.x - rect.w, rect.y - rect.h, rect.w * 3.0, rect.h * 3.0);
                target.DrawText(
                    &utf16,
                    &format,
                    &d2d_rect(r),
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                return Ok(());
            }
            self.ensure_icon(icon)?;
            let geo = &self.icons[&icon];
            let k = rect.w / 24.0;
            target.SetTransform(&Matrix3x2 {
                M11: k,
                M12: 0.0,
                M21: 0.0,
                M22: k,
                M31: rect.x,
                M32: rect.y,
            });
            if let Some(fill) = &geo.fill {
                target.FillGeometry(fill, brush, None);
            }
            if let Some(stroke) = &geo.stroke {
                target.DrawGeometry(stroke, brush, geo.width, &self.stroke_style);
            }
            for &(cx, cy, r, filled) in &geo.circles {
                let e = D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r,
                    radiusY: r,
                };
                if filled {
                    target.FillEllipse(&e, brush);
                } else {
                    target.DrawEllipse(&e, brush, geo.width, &self.stroke_style);
                }
            }
            target.SetTransform(&Matrix3x2::identity());
        }
        Ok(())
    }

    /// Yükleniyor göstergesi: saate göre döndürülmüş yay (bir turu 0,9 sn).
    unsafe fn draw_spinner(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        brush: &ID2D1SolidColorBrush,
        rect: Rect,
        color: Color,
    ) -> Result<()> {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let ms = START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis()
            % 900;
        let angle = ms as f32 / 900.0 * std::f32::consts::TAU;
        self.ensure_icon(Icon::Spinner)?;
        let geo = &self.icons[&Icon::Spinner];
        let k = rect.w / 24.0;
        let (sin, cos) = angle.sin_cos();
        // 24×24 kutunun ortası (12, 12) etrafında döndür, sonra ölçekle ve
        // yerine taşı.
        let c = 12.0;
        unsafe {
            brush.SetColor(&d2d_color(color));
            target.SetTransform(&Matrix3x2 {
                M11: k * cos,
                M12: k * sin,
                M21: -k * sin,
                M22: k * cos,
                M31: rect.x + k * (c - c * cos + c * sin),
                M32: rect.y + k * (c - c * sin - c * cos),
            });
            if let Some(stroke) = &geo.stroke {
                target.DrawGeometry(stroke, brush, geo.width, &self.stroke_style);
            }
            target.SetTransform(&Matrix3x2::identity());
        }
        Ok(())
    }

    /// Site ikonunu PNG'den çözer; aynı sürüm için bir kez yapılır.
    fn favicon_bitmap(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        key: &str,
        favicons: &HashMap<String, (u64, Vec<u8>)>,
    ) -> Option<ID2D1Bitmap> {
        let (version, png) = favicons.get(key)?;
        if let Some((v, bitmap)) = self.bitmaps.get(key)
            && v == version
        {
            return bitmap.clone();
        }
        let bitmap = self.decode(target, png).ok();
        self.bitmaps
            .insert(key.to_string(), (*version, bitmap.clone()));
        bitmap
    }

    fn decode(&self, target: &ID2D1HwndRenderTarget, png: &[u8]) -> Result<ID2D1Bitmap> {
        let wic = self
            .wic
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        unsafe {
            let stream = wic.CreateStream()?;
            stream.InitializeFromMemory(png)?;
            let decoder = wic.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = wic.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )?;
            target.CreateBitmapFromWicBitmap(&converter, None)
        }
    }

    /// İkon yazı tipi biçimi: yatayda ve dikeyde ortalı.
    fn icon_format(&mut self, size: f32) -> Result<IDWriteTextFormat> {
        let key = ((size * 100.0) as u32, false, 1, true);
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        let family = self.icon_font.clone().expect("çağıran kontrol eder");
        unsafe {
            let f = self.dwrite.CreateTextFormat(
                &family,
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("tr-tr"),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            self.formats.insert(key, f.clone());
            Ok(f)
        }
    }

    /// Yazı biçimi: tek satır, dikeyde ortalı, sığmazsa "…" ile kesilir.
    fn format(&mut self, size: f32, bold: bool, align: Align) -> Result<IDWriteTextFormat> {
        let key = ((size * 100.0) as u32, bold, align as u8, false);
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
            f.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
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

/// Windows'un ikon yazı tipindeki karşılıkları (Segoe Fluent Icons ve
/// Segoe MDL2 Assets aynı kod noktalarını kullanır).
/// Yazı tipinde karşılığı olmayanlar (dosya türleri) vektör olarak çizilir.
fn fluent_glyph(icon: Icon) -> Option<char> {
    Some(match icon {
        Icon::Back => '\u{E72B}',
        Icon::Forward => '\u{E72A}',
        Icon::Reload => '\u{E72C}',
        Icon::Plus => '\u{E710}',
        Icon::Close => '\u{E711}',
        Icon::Star => '\u{E734}',
        Icon::StarFilled => '\u{E735}',
        Icon::Shield => '\u{EA18}',
        Icon::Clock => '\u{E81C}',
        Icon::Settings => '\u{E713}',
        Icon::Speaker => '\u{E767}',
        Icon::Globe => '\u{E774}',
        Icon::Search => '\u{E721}',
        Icon::ChevronUp => '\u{E70E}',
        Icon::ChevronDown => '\u{E70D}',
        Icon::Lock => '\u{E72E}',
        Icon::Warning => '\u{E7BA}',
        Icon::Camera => '\u{E722}',
        Icon::Mic => '\u{E720}',
        Icon::Location => '\u{E707}',
        Icon::Bell => '\u{EA8F}',
        Icon::Download => '\u{E896}',
        Icon::Muted => '\u{E74F}',
        Icon::Pin => '\u{E718}',
        Icon::Pause => '\u{E769}',
        Icon::Play => '\u{E768}',
        Icon::Folder => '\u{E8B7}',
        Icon::File
        | Icon::Disc
        | Icon::Archive
        | Icon::Video
        | Icon::Audio
        | Icon::Image
        | Icon::Program
        | Icon::Spinner => return None,
    })
}

/// Listedeki ilk yüklü yazı tipi ailesi.
fn find_family(dwrite: &IDWriteFactory, names: &[&str]) -> Option<HSTRING> {
    unsafe {
        let mut collection = None;
        dwrite
            .GetSystemFontCollection(&mut collection, false)
            .ok()?;
        let collection = collection?;
        names.iter().map(|n| HSTRING::from(*n)).find(|name| {
            let mut index = 0u32;
            let mut exists = BOOL::default();
            collection
                .FindFamilyName(name, &mut index, &mut exists)
                .is_ok()
                && exists.as_bool()
        })
    }
}

/// Windows 11'in yazı tipi varsa onu, yoksa Segoe UI'ı kullan.
fn pick_font(dwrite: &IDWriteFactory) -> HSTRING {
    find_family(dwrite, &["Segoe UI Variable Text"]).unwrap_or_else(|| HSTRING::from("Segoe UI"))
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
