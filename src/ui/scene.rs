//! Çizim listesi: arayüzün platformdan bağımsız tarifi.
//!
//! Düzen kodu (`layout.rs`) bir `Prim` listesi üretir; Windows'ta Direct2D
//! (`d2d.rs`), testlerde SVG (`svg.rs`) bu listeyi çizer.

use super::theme::Color;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    /// Dikdörtgenin ortasına yerleştirilmiş `size`×`size` kare.
    pub fn centered_square(&self, size: f32) -> Rect {
        Rect::new(
            self.x + (self.w - size) / 2.0,
            self.y + (self.h - size) / 2.0,
            size,
            size,
        )
    }
    /// İki dikdörtgenin kesişimi.
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Rect::new(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// Dolu, köşeleri yuvarlatılmış dikdörtgen.
    Fill {
        rect: Rect,
        radius: f32,
        color: Color,
    },
    /// İnce kenar çizgisi (1 DIP).
    Border {
        rect: Rect,
        radius: f32,
        color: Color,
    },
    /// Site ikonu. `key` (alan adı) için ikon yoksa ya da çözülemezse yerine
    /// `color` renginde küre ikonu çizilir.
    Favicon {
        rect: Rect,
        key: Option<String>,
        color: Color,
    },
    /// Tek satır yazı; sığmazsa sonu "…" ile kesilir. Dikeyde ortalanır.
    Text {
        rect: Rect,
        text: String,
        size: f32,
        bold: bool,
        color: Color,
        align: Align,
    },
    Icon {
        icon: Icon,
        rect: Rect,
        color: Color,
    },
    /// Dönen yükleniyor göstergesi. Açısı çizildiği anın saatinden
    /// hesaplanır; arayüz yüklenme sürdükçe düzenli olarak yeniden çizilir.
    Spinner {
        rect: Rect,
        color: Color,
    },
    /// Bundan sonraki çizimler bu dikdörtgenle sınırlanır (kaydırılan liste).
    PushClip(Rect),
    PopClip,
}

/// İkon parçaları, 24×24'lük bir kutuda.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Back,
    Forward,
    Reload,
    Plus,
    Close,
    Star,
    StarFilled,
    Shield,
    Clock,
    Settings,
    Speaker,
    /// İkonu olmayan siteler.
    Globe,
    /// Büyüteç (adres çubuğu önerilerinde arama satırı).
    Search,
    /// Yukarı/aşağı ok (sayfada bul: önceki/sonraki eşleşme).
    ChevronUp,
    ChevronDown,
    /// Kilit (güvenli bağlantı).
    Lock,
    /// Üçgen içinde ünlem (güvenli olmayan bağlantı, sertifika hatası).
    Warning,
    /// İzin soruları.
    Camera,
    Mic,
    Location,
    Bell,
    /// Aşağı ok ve tepsi (indirmeler).
    Download,
    /// Sesi kapatılmış sekme.
    Muted,
    /// Sabitlenmiş sekme (raptiye).
    Pin,
    /// İndirme paneli: duraklat, devam et, klasörde göster.
    Pause,
    Play,
    Folder,
    /// Dosya türleri (Windows'un özel simgesi olmayan dosyalar için).
    File,
    Disc,
    Archive,
    Video,
    Audio,
    Image,
    Program,
    /// Yükleniyor göstergesinin yayı (döndürülerek çizilir).
    Spinner,
    /// Yapboz parçası (eklentiler).
    Puzzle,
    /// Açık kitap (okuma modu).
    Book,
    /// Başlık çubuğu: küçült, büyüt, geri al, pencereyi kapat.
    Minimize,
    Maximize,
    Restore,
    CloseWindow,
}

/// Bir ikonun çizimi: çizgiler (`strokes`) ve doldurulan şekiller (`fills`),
/// ayrıca çember listesi (merkez x, y, yarıçap, dolu mu).
pub struct IconShape {
    pub strokes: Vec<Vec<Seg>>,
    pub fills: Vec<Vec<Seg>>,
    pub circles: Vec<(f32, f32, f32, bool)>,
    pub stroke_width: f32,
}

impl Icon {
    pub fn shape(self) -> IconShape {
        use Seg::*;
        let mut s = IconShape {
            strokes: vec![],
            fills: vec![],
            circles: vec![],
            stroke_width: 1.8,
        };
        let star = vec![
            Move(12.0, 3.5),
            Line(14.6, 8.8),
            Line(20.5, 9.7),
            Line(16.2, 13.8),
            Line(17.2, 19.6),
            Line(12.0, 16.9),
            Line(6.8, 19.6),
            Line(7.8, 13.8),
            Line(3.5, 9.7),
            Line(9.4, 8.8),
            Close,
        ];
        match self {
            Icon::Back => s
                .strokes
                .push(vec![Move(15.0, 18.0), Line(9.0, 12.0), Line(15.0, 6.0)]),
            Icon::Forward => {
                s.strokes
                    .push(vec![Move(9.0, 18.0), Line(15.0, 12.0), Line(9.0, 6.0)])
            }
            Icon::Reload => {
                // Saat yönünde dönen yay; ok ucu yayın bittiği yerde.
                let mut arc_segs = arc(12.0, 12.0, 8.0, 0.0, 300.0);
                arc_segs.push(Line(19.0, 7.0));
                s.strokes.push(arc_segs);
                s.strokes
                    .push(vec![Move(20.0, 2.5), Line(20.0, 7.5), Line(15.0, 7.5)]);
            }
            Icon::Plus => {
                s.strokes.push(vec![Move(12.0, 5.0), Line(12.0, 19.0)]);
                s.strokes.push(vec![Move(5.0, 12.0), Line(19.0, 12.0)]);
            }
            Icon::Close => {
                s.strokes.push(vec![Move(7.0, 7.0), Line(17.0, 17.0)]);
                s.strokes.push(vec![Move(17.0, 7.0), Line(7.0, 17.0)]);
            }
            Icon::Star => s.strokes.push(star),
            Icon::StarFilled => {
                s.fills.push(star.clone());
                s.strokes.push(star);
            }
            Icon::Shield => {
                s.stroke_width = 2.0;
                s.strokes.push(vec![
                    Move(12.0, 3.0),
                    Line(19.0, 6.0),
                    Line(19.0, 12.0),
                    Cubic(19.0, 16.4, 16.0, 19.6, 12.0, 21.0),
                    Cubic(8.0, 19.6, 5.0, 16.4, 5.0, 12.0),
                    Line(5.0, 6.0),
                    Close,
                ]);
            }
            Icon::Clock => {
                s.circles.push((12.0, 12.0, 8.5, false));
                s.strokes
                    .push(vec![Move(12.0, 7.5), Line(12.0, 12.0), Line(15.0, 14.0)]);
            }
            Icon::Settings => {
                // Dişli çark: iç çember ve sekiz diş.
                s.circles.push((12.0, 12.0, 3.0, false));
                s.circles.push((12.0, 12.0, 6.5, false));
                for i in 0..8 {
                    let a = (i as f32 * 45.0).to_radians();
                    let (c, si) = (a.cos(), a.sin());
                    s.strokes.push(vec![
                        Move(12.0 + 6.5 * c, 12.0 + 6.5 * si),
                        Line(12.0 + 9.0 * c, 12.0 + 9.0 * si),
                    ]);
                }
            }
            Icon::Globe => {
                s.stroke_width = 1.6;
                s.circles.push((12.0, 12.0, 8.5, false));
                s.strokes.push(vec![Move(3.5, 12.0), Line(20.5, 12.0)]);
                s.strokes.push(vec![
                    Move(12.0, 3.5),
                    Cubic(8.5, 7.0, 8.5, 17.0, 12.0, 20.5),
                ]);
                s.strokes.push(vec![
                    Move(12.0, 3.5),
                    Cubic(15.5, 7.0, 15.5, 17.0, 12.0, 20.5),
                ]);
            }
            Icon::Speaker => {
                s.stroke_width = 2.0;
                s.strokes.push(vec![
                    Move(4.0, 10.0),
                    Line(4.0, 14.0),
                    Line(8.0, 14.0),
                    Line(13.0, 18.0),
                    Line(13.0, 6.0),
                    Line(8.0, 10.0),
                    Close,
                ]);
                s.strokes.push(arc(13.0, 12.0, 4.0, -50.0, 50.0));
            }
            Icon::Search => {
                s.stroke_width = 2.0;
                s.circles.push((10.5, 10.5, 6.0, false));
                s.strokes.push(vec![Move(15.0, 15.0), Line(20.0, 20.0)]);
            }
            Icon::ChevronUp => {
                s.strokes
                    .push(vec![Move(6.0, 15.0), Line(12.0, 9.0), Line(18.0, 15.0)])
            }
            Icon::ChevronDown => {
                s.strokes
                    .push(vec![Move(6.0, 9.0), Line(12.0, 15.0), Line(18.0, 9.0)])
            }
            Icon::Lock => {
                s.stroke_width = 2.0;
                s.strokes.push(vec![
                    Move(6.0, 11.0),
                    Line(18.0, 11.0),
                    Line(18.0, 20.0),
                    Line(6.0, 20.0),
                    Close,
                ]);
                let mut shackle = vec![Move(8.5, 11.0), Line(8.5, 7.5)];
                shackle.extend(arc(12.0, 7.5, 3.5, 180.0, 360.0).into_iter().skip(1));
                shackle.push(Line(15.5, 11.0));
                s.strokes.push(shackle);
            }
            Icon::Warning => {
                s.stroke_width = 2.0;
                s.strokes.push(vec![
                    Move(12.0, 4.0),
                    Line(21.0, 20.0),
                    Line(3.0, 20.0),
                    Close,
                ]);
                s.strokes.push(vec![Move(12.0, 10.0), Line(12.0, 14.0)]);
                s.circles.push((12.0, 17.0, 0.9, true));
            }
            Icon::Camera => {
                s.stroke_width = 2.0;
                s.strokes.push(vec![
                    Move(3.0, 7.0),
                    Line(15.0, 7.0),
                    Line(15.0, 17.0),
                    Line(3.0, 17.0),
                    Close,
                ]);
                s.strokes.push(vec![
                    Move(15.0, 10.5),
                    Line(21.0, 7.5),
                    Line(21.0, 16.5),
                    Line(15.0, 13.5),
                ]);
            }
            Icon::Mic => {
                s.stroke_width = 2.0;
                let mut capsule = arc(12.0, 7.0, 3.0, 180.0, 360.0);
                capsule.push(Line(15.0, 12.0));
                capsule.extend(arc(12.0, 12.0, 3.0, 0.0, 180.0).into_iter().skip(1));
                capsule.push(Close);
                s.strokes.push(capsule);
                s.strokes.push(arc(12.0, 11.0, 6.0, 0.0, 180.0));
                s.strokes.push(vec![Move(12.0, 17.0), Line(12.0, 21.0)]);
            }
            Icon::Location => {
                s.stroke_width = 2.0;
                let mut pin = arc(12.0, 10.0, 6.0, 135.0, 405.0);
                pin.push(Line(12.0, 21.0));
                pin.push(Close);
                s.strokes.push(pin);
                s.circles.push((12.0, 10.0, 2.0, false));
            }
            Icon::Bell => {
                s.stroke_width = 2.0;
                let mut bell = vec![Move(6.0, 17.0), Line(6.0, 11.0)];
                bell.extend(arc(12.0, 11.0, 6.0, 180.0, 360.0).into_iter().skip(1));
                bell.push(Line(18.0, 17.0));
                s.strokes.push(bell);
                s.strokes.push(vec![Move(4.0, 17.0), Line(20.0, 17.0)]);
                s.circles.push((12.0, 19.5, 1.2, true));
            }
            Icon::Download => {
                s.strokes.push(vec![Move(12.0, 4.0), Line(12.0, 15.0)]);
                s.strokes
                    .push(vec![Move(7.5, 10.5), Line(12.0, 15.0), Line(16.5, 10.5)]);
                s.strokes.push(vec![Move(5.0, 19.5), Line(19.0, 19.5)]);
            }
            Icon::Muted => {
                s.strokes.push(vec![
                    Move(4.0, 9.5),
                    Line(7.5, 9.5),
                    Line(12.0, 5.5),
                    Line(12.0, 18.5),
                    Line(7.5, 14.5),
                    Line(4.0, 14.5),
                    Close,
                ]);
                s.strokes.push(vec![Move(15.5, 9.5), Line(20.5, 14.5)]);
                s.strokes.push(vec![Move(20.5, 9.5), Line(15.5, 14.5)]);
            }
            Icon::Pin => {
                s.strokes.push(vec![Move(8.5, 4.0), Line(15.5, 4.0)]);
                s.strokes.push(vec![
                    Move(10.0, 4.0),
                    Line(10.0, 10.0),
                    Line(7.0, 14.0),
                    Line(17.0, 14.0),
                    Line(14.0, 10.0),
                    Line(14.0, 4.0),
                ]);
                s.strokes.push(vec![Move(12.0, 14.0), Line(12.0, 20.0)]);
            }
            Icon::Puzzle => {
                // Gövde ve iki yuvarlak çıkıntı (üstte ve sağda).
                s.strokes.push(vec![
                    Move(4.0, 8.0),
                    Line(8.0, 8.0),
                    Cubic(7.0, 2.5, 13.0, 2.5, 12.0, 8.0),
                    Line(16.0, 8.0),
                    Line(16.0, 12.0),
                    Cubic(21.5, 11.0, 21.5, 17.0, 16.0, 16.0),
                    Line(16.0, 20.0),
                    Line(4.0, 20.0),
                    Close,
                ]);
            }
            Icon::Book => {
                // İki sayfa, ortada sırt.
                s.strokes.push(vec![
                    Move(12.0, 7.0),
                    Cubic(9.5, 5.0, 6.5, 5.0, 3.5, 5.5),
                    Line(3.5, 18.0),
                    Cubic(6.5, 17.5, 9.5, 17.5, 12.0, 19.5),
                    Cubic(14.5, 17.5, 17.5, 17.5, 20.5, 18.0),
                    Line(20.5, 5.5),
                    Cubic(17.5, 5.0, 14.5, 5.0, 12.0, 7.0),
                    Close,
                ]);
                s.strokes.push(vec![Move(12.0, 7.0), Line(12.0, 19.5)]);
            }
            Icon::Minimize => s.strokes.push(vec![Move(2.0, 12.0), Line(22.0, 12.0)]),
            Icon::Maximize => s.strokes.push(vec![
                Move(2.0, 2.0),
                Line(22.0, 2.0),
                Line(22.0, 22.0),
                Line(2.0, 22.0),
                Close,
            ]),
            Icon::Restore => {
                s.strokes.push(vec![
                    Move(2.0, 7.0),
                    Line(17.0, 7.0),
                    Line(17.0, 22.0),
                    Line(2.0, 22.0),
                    Close,
                ]);
                s.strokes.push(vec![
                    Move(7.0, 7.0),
                    Line(7.0, 2.0),
                    Line(22.0, 2.0),
                    Line(22.0, 17.0),
                    Line(17.0, 17.0),
                ]);
            }
            Icon::CloseWindow => {
                s.strokes.push(vec![Move(2.0, 2.0), Line(22.0, 22.0)]);
                s.strokes.push(vec![Move(22.0, 2.0), Line(2.0, 22.0)]);
            }
            Icon::Pause => {
                s.fills.push(rounded(7.0, 5.0, 3.5, 14.0, 1.0));
                s.fills.push(rounded(13.5, 5.0, 3.5, 14.0, 1.0));
            }
            Icon::Play => s.fills.push(vec![
                Move(8.0, 5.0),
                Line(19.0, 12.0),
                Line(8.0, 19.0),
                Close,
            ]),
            Icon::Folder => s.strokes.push(vec![
                Move(3.5, 7.0),
                Line(3.5, 18.0),
                Line(20.5, 18.0),
                Line(20.5, 8.5),
                Line(11.5, 8.5),
                Line(9.5, 6.0),
                Line(3.5, 6.0),
                Close,
            ]),
            Icon::File => {
                s.strokes.push(page());
                s.strokes
                    .push(vec![Move(14.0, 3.0), Line(14.0, 8.0), Line(19.0, 8.0)]);
            }
            Icon::Disc => {
                s.circles.push((12.0, 12.0, 9.0, false));
                s.circles.push((12.0, 12.0, 2.5, false));
            }
            Icon::Archive => {
                s.strokes.push(rounded(3.0, 4.0, 18.0, 5.0, 1.0));
                s.strokes.push(vec![
                    Move(5.0, 9.0),
                    Line(5.0, 20.0),
                    Line(19.0, 20.0),
                    Line(19.0, 9.0),
                ]);
                s.strokes.push(vec![Move(10.0, 13.0), Line(14.0, 13.0)]);
            }
            Icon::Video => {
                s.strokes.push(rounded(3.0, 5.0, 18.0, 14.0, 2.0));
                s.fills.push(vec![
                    Move(10.0, 9.2),
                    Line(14.8, 12.0),
                    Line(10.0, 14.8),
                    Close,
                ]);
            }
            Icon::Audio => {
                s.strokes.push(vec![
                    Move(9.0, 18.0),
                    Line(9.0, 6.0),
                    Line(20.0, 4.0),
                    Line(20.0, 16.0),
                ]);
                s.circles.push((6.0, 18.0, 3.0, false));
                s.circles.push((17.0, 16.0, 3.0, false));
            }
            Icon::Image => {
                s.strokes.push(rounded(3.0, 4.0, 18.0, 16.0, 2.0));
                s.circles.push((9.0, 10.0, 1.8, false));
                s.strokes
                    .push(vec![Move(21.0, 16.0), Line(16.0, 11.0), Line(7.0, 20.0)]);
            }
            Icon::Program => {
                s.strokes.push(rounded(3.0, 4.0, 18.0, 16.0, 2.0));
                s.strokes.push(vec![Move(3.0, 8.0), Line(21.0, 8.0)]);
                s.strokes
                    .push(vec![Move(8.0, 12.0), Line(10.5, 14.5), Line(8.0, 17.0)]);
                s.strokes.push(vec![Move(13.0, 17.0), Line(16.0, 17.0)]);
            }
            Icon::Spinner => {
                s.stroke_width = 2.4;
                s.strokes.push(arc(12.0, 12.0, 8.5, 0.0, 270.0));
            }
        }
        s
    }
}

/// Köşeleri yuvarlatılmış dikdörtgen.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Seg> {
    use Seg::*;
    let k = r * 0.5523;
    let (rx, by) = (x + w, y + h);
    vec![
        Move(x + r, y),
        Line(rx - r, y),
        Cubic(rx - r + k, y, rx, y + r - k, rx, y + r),
        Line(rx, by - r),
        Cubic(rx, by - r + k, rx - r + k, by, rx - r, by),
        Line(x + r, by),
        Cubic(x + r - k, by, x, by - r + k, x, by - r),
        Line(x, y + r),
        Cubic(x, y + r - k, x + r - k, y, x + r, y),
        Close,
    ]
}

/// Köşesi kıvrık sayfa (dosya simgeleri).
fn page() -> Vec<Seg> {
    use Seg::*;
    vec![
        Move(14.0, 3.0),
        Line(7.0, 3.0),
        Cubic(5.9, 3.0, 5.0, 3.9, 5.0, 5.0),
        Line(5.0, 19.0),
        Cubic(5.0, 20.1, 5.9, 21.0, 7.0, 21.0),
        Line(17.0, 21.0),
        Cubic(18.1, 21.0, 19.0, 20.1, 19.0, 19.0),
        Line(19.0, 8.0),
        Close,
    ]
}

/// Çember yayı (derece; 0 = sağ, saat yönünde) Bézier eğrileriyle.
fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Vec<Seg> {
    let steps = ((to - from).abs() / 90.0).ceil().max(1.0) as usize;
    let step = (to - from) / steps as f32;
    let point = |deg: f32| {
        let a = deg.to_radians();
        (cx + r * a.cos(), cy + r * a.sin())
    };
    let (x0, y0) = point(from);
    let mut segs = vec![Seg::Move(x0, y0)];
    for i in 0..steps {
        let a0 = (from + step * i as f32).to_radians();
        let a1 = (from + step * (i + 1) as f32).to_radians();
        let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan() * r;
        let (p0x, p0y) = (cx + r * a0.cos(), cy + r * a0.sin());
        let (p3x, p3y) = (cx + r * a1.cos(), cy + r * a1.sin());
        segs.push(Seg::Cubic(
            p0x - k * a0.sin(),
            p0y + k * a0.cos(),
            p3x + k * a1.sin(),
            p3y - k * a1.cos(),
            p3x,
            p3y,
        ));
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_helpers() {
        let r = Rect::new(10.0, 10.0, 100.0, 40.0);
        assert!(r.contains(10.0, 10.0));
        assert!(!r.contains(110.0, 20.0));
        assert_eq!(r.centered_square(20.0), Rect::new(50.0, 20.0, 20.0, 20.0));
        assert_eq!(
            r.intersect(&Rect::new(50.0, 0.0, 200.0, 30.0)),
            Rect::new(50.0, 10.0, 60.0, 20.0)
        );
    }

    #[test]
    fn arcs_end_where_expected() {
        let segs = arc(0.0, 0.0, 10.0, 0.0, 180.0);
        let Seg::Cubic(.., x, y) = *segs.last().unwrap() else {
            panic!("son parça eğri olmalı")
        };
        assert!((x + 10.0).abs() < 1e-3 && y.abs() < 1e-3);
    }

    #[test]
    fn every_icon_draws_something() {
        for icon in [
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
            Icon::Lock,
            Icon::Warning,
            Icon::Camera,
            Icon::Mic,
            Icon::Location,
            Icon::Bell,
            Icon::Download,
            Icon::Muted,
            Icon::Pin,
            Icon::Pause,
            Icon::Play,
            Icon::Folder,
            Icon::File,
            Icon::Disc,
            Icon::Archive,
            Icon::Video,
            Icon::Audio,
            Icon::Image,
            Icon::Program,
            Icon::Spinner,
            Icon::Puzzle,
            Icon::Book,
            Icon::Minimize,
            Icon::Maximize,
            Icon::Restore,
            Icon::CloseWindow,
        ] {
            let s = icon.shape();
            assert!(!s.strokes.is_empty() || !s.fills.is_empty() || !s.circles.is_empty());
        }
    }
}
