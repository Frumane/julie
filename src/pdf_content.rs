//! PDF'in sayfa resminden başka içeriği: seçilip kopyalanabilen ve
//! aranabilen metin, bağlantılar ve içindekiler.
//!
//! Konumların hepsi punto cinsinden ve sayfanın sol üst köşesine göredir
//! (görüntüleyicinin çizdiği sayfa resmiyle aynı düzen).

use std::collections::HashMap;

use hayro::hayro_interpret::font::{Glyph as FontGlyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, TransformExt, interpret_page,
};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, String as PdfString};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use serde_json::{Value, json};

/// Glif çiziminin birimi: yazı boyutu 1000 birim.
const EM: f64 = 1000.0;

/// Sayfaya çizilen bir karakter.
#[derive(Debug, Clone)]
struct Char {
    text: String,
    /// Yazı çizgisindeki başlangıç noktası.
    origin: Point,
    /// Yazı yönünde birim vektör.
    dir: Vec2,
    /// Yazı boyutu (punto).
    size: f64,
    /// Karakterin kendi genişliği (çiziminin sağ kenarına kadar).
    width: f64,
}

struct Collector {
    chars: Vec<Char>,
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for glyph in run.glyphs() {
            let t: Affine = props.transform * glyph.transform();
            let origin = t * Point::ZERO;
            let right = t * Point::new(EM, 0.0) - origin;
            let up = t * Point::new(0.0, EM) - origin;
            let size = up.hypot();
            let length = right.hypot();
            if !(0.5..2000.0).contains(&size) || length < 1e-6 {
                continue;
            }
            let dir = right / length;
            let text = match glyph.as_unicode() {
                Some(BfString::Char(c)) => unligature(c),
                Some(BfString::String(s)) => s.chars().map(unligature).collect(),
                None => continue,
            };
            if text.chars().all(char::is_control) {
                continue;
            }
            let width = match &**glyph {
                // Yazı tipinin kendi ilerleme genişliği; yoksa mürekkebin
                // sağ kenarı (o da daktilo harflerinde noktadan sonra boşluk
                // varmış gibi gösterir, onun için ikinci tercih).
                FontGlyph::Outline(outline) => match outline.advance_width() {
                    Some(advance) if advance > 0.0 => {
                        (t * Point::new(f64::from(advance), 0.0) - origin).dot(dir)
                    }
                    _ => {
                        let b = outline.outline().bounding_box();
                        (t * Point::new(b.x1, 0.0) - origin).dot(dir)
                    }
                },
                FontGlyph::Type3(_) => size * 0.5,
            };
            let width = if width.is_finite() && width > 0.0 {
                width.min(size * 3.0)
            } else if text.trim().is_empty() {
                size * 0.25
            } else {
                size * 0.5
            };
            self.chars.push(Char {
                text,
                origin,
                dir,
                size,
                width,
            });
        }
    }
}

fn page_chars(page: &Page<'_>) -> Vec<Char> {
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, w as f64, h as f64),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut collector = Collector { chars: Vec::new() };
    interpret_page(page, &mut ctx, &mut collector);
    collector.chars
}

/// Aynı satırda, aynı boyda yan yana yazılmış karakterler.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Yazının sol üst köşesi.
    pub x: f64,
    pub y: f64,
    /// Genişlik ve yazı boyutu.
    pub width: f64,
    pub size: f64,
    /// Yazı yönü (radyan; 0 soldan sağa).
    pub angle: f64,
    pub text: String,
    /// Satırın son parçası (kopyalarken alt satıra geçilir).
    pub line_end: bool,
}

struct Building {
    start: Point,
    dir: Vec2,
    size: f64,
    /// Yazı yönünde, başlangıçtan son karakterin sağ kenarına uzaklık.
    end: f64,
    text: String,
}

impl Building {
    fn new(c: &Char) -> Building {
        Building {
            start: c.origin,
            dir: c.dir,
            size: c.size,
            end: c.width,
            text: c.text.clone(),
        }
    }

    /// Karakterin başlangıca göre konumu: (yazı yönünde, ona dik).
    fn place(&self, c: &Char) -> (f64, f64) {
        let d = c.origin - self.start;
        (d.dot(self.dir), d.cross(self.dir))
    }

    fn finish(self, line_end: bool) -> Span {
        // Yazı boyutunun ~%80'i satır çizgisinin üstünde kalır.
        let up = Vec2::new(self.dir.y, -self.dir.x);
        let top = self.start + up * (self.size * 0.8);
        Span {
            x: top.x,
            y: top.y,
            width: self.end.max(0.0),
            size: self.size,
            angle: self.dir.y.atan2(self.dir.x),
            text: self.text.trim_end().to_string(),
            line_end,
        }
    }
}

/// Karakterleri satır parçalarına toplar; kelime araları boşlukla ayrılır.
fn spans(chars: &[Char]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut current: Option<Building> = None;
    for c in chars {
        let Some(mut span) = current.take() else {
            current = Some(Building::new(c));
            continue;
        };
        let (along, across) = span.place(c);
        let same_dir = span.dir.dot(c.dir) > 0.999;
        // Üst/alt simgeler (dipnot işaretleri) satırın yarım em kadar
        // üstünde ya da altında olabilir; satır aralığı ise en az ~1 em.
        let same_line = same_dir && across.abs() < span.size.max(c.size) * 0.6;
        let gap = along - span.end;
        let ratio = c.size / span.size;
        let similar = (0.5..2.0).contains(&ratio);
        if same_line && similar && gap > -span.size * 0.4 && gap < span.size * 1.6 {
            let space = c.text.trim().is_empty();
            if space {
                if !span.text.ends_with(' ') {
                    span.text.push(' ');
                }
            } else {
                if gap > span.size * 0.15 && !span.text.ends_with(' ') {
                    span.text.push(' ');
                }
                span.text.push_str(&c.text);
            }
            span.end = span.end.max(along + c.width);
            current = Some(span);
            continue;
        }
        // Aynı satırda ileride (sütun arası): satır bitmiş sayılmaz.
        let line_end = !(same_line && gap > 0.0);
        if !span.text.trim().is_empty() {
            out.push(span.finish(line_end));
        } else if line_end && let Some(last) = out.last_mut() {
            last.line_end = true;
        }
        current = (!c.text.trim().is_empty()).then(|| Building::new(c));
    }
    if let Some(span) = current
        && !span.text.trim().is_empty()
    {
        out.push(span.finish(true));
    }
    if let Some(last) = out.last_mut() {
        last.line_end = true;
    }
    out
}

/// Sayfanın metni.
pub fn page_text(page: &Page<'_>) -> Vec<Span> {
    spans(&page_chars(page))
}

/// Bağlantının gittiği yer.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Web adresi (yalnızca http, https, mailto).
    Url(String),
    /// Belgenin içinde bir sayfa; varsa sayfadaki yükseklik (punto, üstten).
    Page(usize, Option<f64>),
}

impl Target {
    fn json(&self) -> Value {
        match self {
            Target::Url(url) => json!({ "url": url }),
            Target::Page(page, y) => json!({ "page": page, "y": y.map(round) }),
        }
    }
}

/// Sayfadaki tıklanabilir bir alan.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub rect: Rect,
    pub target: Target,
}

/// İçindekiler satırı.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub target: Option<Target>,
    pub children: Vec<OutlineItem>,
}

/// Sayfaların nesne numarası → sırası (bağlantıların hedefini bulmak için).
pub struct PageMap {
    ids: HashMap<(u32, i32), usize>,
    /// Her sayfanın PDF düzeninden ekran düzenine dönüşümü.
    transforms: Vec<Affine>,
}

impl PageMap {
    pub fn new(pdf: &Pdf) -> PageMap {
        let mut ids = HashMap::new();
        let mut transforms = Vec::new();
        for (i, page) in pdf.pages().iter().enumerate() {
            if let Some(id) = page.raw().obj_id() {
                ids.insert((id.obj_number, id.gen_number), i);
            }
            transforms.push(page.initial_transform(true).to_kurbo());
        }
        PageMap { ids, transforms }
    }

    fn page_of(&self, item: MaybeRef<Object<'_>>) -> Option<usize> {
        match item {
            MaybeRef::Ref(r) => self.ids.get(&(r.obj_number, r.gen_number)).copied(),
            // Bazı belgeler sayfa sırasını sayı olarak yazar.
            MaybeRef::NotRef(Object::Number(n)) => {
                let n = n.as_i64();
                (0..self.transforms.len() as i64)
                    .contains(&n)
                    .then_some(n as usize)
            }
            MaybeRef::NotRef(_) => None,
        }
    }

    /// `[sayfa /XYZ sol üst yakınlık]` gibi açık hedef.
    fn explicit(&self, dest: &Array<'_>) -> Option<Target> {
        let mut items = dest.raw_iter();
        let page = self.page_of(items.next()?)?;
        let kind = match items.next() {
            Some(MaybeRef::NotRef(Object::Name(n))) => n.as_str().to_string(),
            _ => String::new(),
        };
        let numbers: Vec<Option<f64>> = items
            .map(|item| match item {
                MaybeRef::NotRef(Object::Number(n)) => Some(n.as_f64()),
                _ => None,
            })
            .collect();
        // Sayfadaki yükseklik: XYZ'de 2., FitH/FitBH'de 1. sayı.
        let top = match kind.as_str() {
            "XYZ" => numbers.get(1).copied().flatten(),
            "FitH" | "FitBH" => numbers.first().copied().flatten(),
            "FitR" => numbers.get(3).copied().flatten(),
            _ => None,
        };
        let y = top.map(|top| (self.transforms[page] * Point::new(0.0, top)).y.max(0.0));
        Some(Target::Page(page, y))
    }
}

/// Belge düzeyindeki bilgiler: içindekiler ve adlandırılmış hedefler.
pub struct Catalog<'a> {
    catalog: Option<Dict<'a>>,
    pages: PageMap,
}

/// Ağaçta en fazla bu kadar iç içe girilir (bozuk belgelerde döngü olmasın).
const MAX_DEPTH: usize = 32;
/// İçindekilerdeki en fazla satır.
const MAX_ITEMS: usize = 10_000;

impl<'a> Catalog<'a> {
    pub fn new(pdf: &'a Pdf) -> Catalog<'a> {
        let xref = pdf.xref();
        Catalog {
            catalog: xref.get::<Dict<'_>>(xref.root_id()),
            pages: PageMap::new(pdf),
        }
    }

    /// `/Dest` ya da `/D` değeri: açık dizi, ad ya da metin.
    fn dest(&self, value: Object<'_>, depth: usize) -> Option<Target> {
        if depth > 4 {
            return None;
        }
        match value {
            Object::Array(a) => self.pages.explicit(&a),
            Object::Name(n) => self.named(n.as_str().as_bytes(), depth),
            Object::String(s) => self.named(s.as_bytes(), depth),
            Object::Dict(d) => self.dest(d.get::<Object<'_>>(b"D")?, depth + 1),
            _ => None,
        }
    }

    /// Adlandırılmış hedef: eski `/Dests` sözlüğü ya da `/Names /Dests` ağacı.
    fn named(&self, name: &[u8], depth: usize) -> Option<Target> {
        let catalog = self.catalog.as_ref()?;
        if let Some(dests) = catalog.get::<Dict<'_>>(b"Dests")
            && let Ok(key) = std::str::from_utf8(name)
            && let Some(value) = dests.get::<Object<'_>>(key)
        {
            return self.dest(value, depth + 1);
        }
        let tree = catalog
            .get::<Dict<'_>>(b"Names")?
            .get::<Dict<'_>>(b"Dests")?;
        let value = name_tree_lookup(&tree, name, 0)?;
        self.dest(value, depth + 1)
    }

    /// Bağlantının ya da içindekiler satırının hedefi (`/Dest` ya da `/A`).
    fn target(&self, dict: &Dict<'_>) -> Option<Target> {
        if let Some(dest) = dict.get::<Object<'_>>(b"Dest") {
            return self.dest(dest, 0);
        }
        let action = dict.get::<Dict<'_>>(b"A")?;
        match action.get::<Name<'_>>(b"S")?.as_str() {
            "GoTo" => self.dest(action.get::<Object<'_>>(b"D")?, 0),
            "URI" => {
                let uri = action.get::<PdfString<'_>>(b"URI")?;
                let uri = String::from_utf8_lossy(uri.as_bytes()).trim().to_string();
                safe_url(&uri).then_some(Target::Url(uri))
            }
            _ => None,
        }
    }

    /// Sayfadaki bağlantılar.
    pub fn links(&self, index: usize, page: &Page<'_>) -> Vec<Link> {
        let Some(annots) = page.raw().get::<Array<'_>>(b"Annots") else {
            return Vec::new();
        };
        let transform = self.pages.transforms[index];
        annots
            .iter::<Dict<'_>>()
            .take(2000)
            .filter(|a| {
                a.get::<Name<'_>>(b"Subtype")
                    .is_some_and(|s| s.as_str() == "Link")
            })
            .filter_map(|a| {
                let [x0, y0, x1, y1] = a.get::<[f64; 4]>(b"Rect")?;
                let rect = transform.transform_rect_bbox(Rect::new(x0, y0, x1, y1).abs());
                let target = self.target(&a)?;
                (rect.width() > 0.5 && rect.height() > 0.5).then_some(Link { rect, target })
            })
            .collect()
    }

    /// İçindekiler (belgede yoksa boş).
    pub fn outline(&self) -> Vec<OutlineItem> {
        let Some(first) = self
            .catalog
            .as_ref()
            .and_then(|c| c.get::<Dict<'_>>(b"Outlines"))
            .and_then(|o| o.get::<Dict<'_>>(b"First"))
        else {
            return Vec::new();
        };
        let mut count = 0;
        self.outline_level(first, 0, &mut count)
    }

    fn outline_level(&self, first: Dict<'_>, depth: usize, count: &mut usize) -> Vec<OutlineItem> {
        let mut items = Vec::new();
        let mut next = Some(first);
        while let Some(item) = next {
            *count += 1;
            if *count > MAX_ITEMS {
                break;
            }
            let title = item
                .get::<PdfString<'_>>(b"Title")
                .map(|t| pdf_text(t.as_bytes()))
                .unwrap_or_default();
            let children = match item.get::<Dict<'_>>(b"First") {
                Some(child) if depth < MAX_DEPTH => self.outline_level(child, depth + 1, count),
                _ => Vec::new(),
            };
            items.push(OutlineItem {
                title: title.split_whitespace().collect::<Vec<_>>().join(" "),
                target: self.target(&item),
                children,
            });
            next = item.get::<Dict<'_>>(b"Next");
        }
        items
    }
}

/// Ad ağacında arama (`/Names [ad değer …]`, `/Kids`).
fn name_tree_lookup<'a>(node: &Dict<'a>, name: &[u8], depth: usize) -> Option<Object<'a>> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(names) = node.get::<Array<'_>>(b"Names") {
        let mut items = names.iter::<Object<'_>>();
        while let (Some(key), Some(value)) = (items.next(), items.next()) {
            if let Object::String(key) = key
                && key.as_bytes() == name
            {
                return Some(value);
            }
        }
    }
    let kids = node.get::<Array<'_>>(b"Kids")?;
    for kid in kids.iter::<Dict<'_>>() {
        // Sınırlar varsa yalnızca uygun dala inilir.
        if let Some(limits) = kid.get::<Array<'_>>(b"Limits") {
            let limits: Vec<_> = limits.iter::<PdfString<'_>>().collect();
            if let [low, high] = limits.as_slice()
                && (name < low.as_bytes() || name > high.as_bytes())
            {
                continue;
            }
        }
        if let Some(found) = name_tree_lookup(&kid, name, depth + 1) {
            return Some(found);
        }
    }
    None
}

/// Bitişik harfler (ﬁ, ﬂ…) ayrı harflere çevrilir: kopyalanan ve aranan
/// metin "fi" olsun.
fn unligature(c: char) -> String {
    match c {
        '\u{FB00}' => "ff".into(),
        '\u{FB01}' => "fi".into(),
        '\u{FB02}' => "fl".into(),
        '\u{FB03}' => "ffi".into(),
        '\u{FB04}' => "ffl".into(),
        '\u{FB05}' | '\u{FB06}' => "st".into(),
        c => c.to_string(),
    }
}

/// Görüntüleyicide açılmasına izin verilen bağlantılar.
fn safe_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
        && !url.chars().any(char::is_control)
}

/// PDF metni: `FE FF` ile başlıyorsa UTF-16, değilse Latin-1'e yakın.
pub fn pdf_text(raw: &[u8]) -> String {
    if let Some(rest) = raw.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    raw.iter().map(|&b| b as char).collect()
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Görüntüleyicinin istediği sayfa içeriği: metin ve bağlantılar.
pub fn page_json(spans: &[Span], links: &[Link]) -> Value {
    let text: Vec<Value> = spans
        .iter()
        .map(|s| {
            json!([
                round(s.x),
                round(s.y),
                round(s.width),
                round(s.size),
                round(s.angle),
                s.text,
                u8::from(s.line_end)
            ])
        })
        .collect();
    let links: Vec<Value> = links
        .iter()
        .map(|l| {
            let mut v = l.target.json();
            v["rect"] = json!([
                round(l.rect.x0),
                round(l.rect.y0),
                round(l.rect.width()),
                round(l.rect.height())
            ]);
            v
        })
        .collect();
    json!({ "text": text, "links": links })
}

/// İçindekiler, görüntüleyici için.
pub fn outline_json(items: &[OutlineItem]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|i| {
                let mut v = json!({ "title": i.title, "items": outline_json(&i.children) });
                if let Some(Target::Page(page, y)) = &i.target {
                    v["page"] = json!(page);
                    v["y"] = json!(y.map(round));
                } else if let Some(Target::Url(url)) = &i.target {
                    v["url"] = json!(url);
                }
                v
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(text: &str, x: f64, y: f64, size: f64, width: f64) -> Char {
        Char {
            text: text.into(),
            origin: Point::new(x, y),
            dir: Vec2::new(1.0, 0.0),
            size,
            width,
        }
    }

    #[test]
    fn groups_characters_into_lines() {
        let chars = [
            ch("A", 10.0, 100.0, 10.0, 6.0),
            ch("b", 16.5, 100.0, 10.0, 5.0),
            // Kelime arası (boşluk karakteri yok).
            ch("c", 25.0, 100.0, 10.0, 5.0),
            // Sütun arası: aynı satır ama ayrı parça.
            ch("D", 200.0, 100.0, 10.0, 6.0),
            // Üst simge aynı satırda kalır.
            ch("*", 206.5, 96.0, 7.0, 3.0),
            // Alt satır.
            ch("e", 10.0, 114.0, 10.0, 5.0),
            ch(" ", 15.0, 114.0, 10.0, 2.5),
            ch("f", 18.0, 114.0, 10.0, 5.0),
        ];
        let spans = spans(&chars);
        let texts: Vec<_> = spans
            .iter()
            .map(|s| (s.text.as_str(), s.line_end))
            .collect();
        assert_eq!(texts, [("Ab c", false), ("D*", true), ("e f", true)]);
        assert_eq!(spans[0].x, 10.0);
        assert!((spans[0].y - 92.0).abs() < 1e-9);
        assert!((spans[0].width - 20.0).abs() < 1e-9);
    }

    #[test]
    fn texts_and_urls() {
        assert_eq!(pdf_text(&[0xFE, 0xFF, 0x00, 0xD6, 0x00, 0x64]), "Öd");
        assert_eq!(pdf_text(b"Plain"), "Plain");
        assert_eq!(unligature('\u{FB01}'), "fi");
        assert!(safe_url("https://a.com/"));
        assert!(safe_url("mailto:a@b.c"));
        assert!(!safe_url("javascript:alert(1)"));
        assert!(!safe_url("file:///C:/x"));
    }

    /// Elle yazılmış, içindekileri ve bağlantıları olan iki sayfalık PDF.
    fn linked_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R /Outlines 7 0 R /Names << /Dests 10 0 R >> >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Contents 5 0 R \
             /Resources << /Font << /F1 6 0 R >> >> /Annots [11 0 R 12 0 R] >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>",
            "STREAM",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            "<< /Type /Outlines /First 8 0 R /Last 9 0 R /Count 2 >>",
            "<< /Title (Giris) /Parent 7 0 R /Next 9 0 R /Dest [3 0 R /XYZ 0 250 0] >>",
            "<< /Title <FEFF00D6006E> /Parent 7 0 R /Prev 8 0 R /A << /S /GoTo /D (son) >> >>",
            "<< /Names [(son) [4 0 R /Fit]] >>",
            "<< /Type /Annot /Subtype /Link /Rect [10 10 60 30] \
             /A << /S /URI /URI (https://example.com/) >> >>",
            "<< /Type /Annot /Subtype /Link /Rect [10 40 60 60] /Dest (son) >>",
        ];
        let content = "BT /F1 20 Tf 20 260 Td (Merhaba dunya) Tj ET";
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            let body = if *body == "STREAM" {
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len()
                )
            } else {
                body.to_string()
            };
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn reads_text_links_and_outline() {
        let pdf = Pdf::new(linked_pdf()).unwrap();
        let catalog = Catalog::new(&pdf);
        let page = pdf.pages().first().unwrap();
        let text = page_text(page);
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].text, "Merhaba dunya");
        assert!((text[0].x - 20.0).abs() < 0.5);
        // Satır çizgisi y=300-260=40; üst kenarı yazı boyunun %80'i kadar yukarıda.
        assert!(
            (text[0].y - (40.0 - 20.0 * 0.8)).abs() < 1.5,
            "{:?}",
            text[0]
        );
        assert!(text[0].width > 100.0 && text[0].width < 140.0);

        let links = catalog.links(0, page);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, Target::Url("https://example.com/".into()));
        assert_eq!(links[0].rect, Rect::new(10.0, 270.0, 60.0, 290.0));
        assert_eq!(links[1].target, Target::Page(1, None));

        let outline = catalog.outline();
        let titles: Vec<_> = outline.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Giris", "Ön"]);
        assert_eq!(outline[0].target, Some(Target::Page(0, Some(50.0))));
        assert_eq!(outline[1].target, Some(Target::Page(1, None)));

        let json = page_json(&text, &links);
        assert_eq!(json["text"][0][5], "Merhaba dunya");
        assert_eq!(json["links"][0]["url"], "https://example.com/");
        assert_eq!(json["links"][1]["page"], 1);
        assert_eq!(outline_json(&outline)[0]["y"], 50.0);
    }
}
