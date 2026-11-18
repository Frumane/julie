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
        }
        s
    }
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
        ] {
            let s = icon.shape();
            assert!(!s.strokes.is_empty() || !s.fills.is_empty() || !s.circles.is_empty());
        }
    }
}
