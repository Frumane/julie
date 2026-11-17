//! Görünüm ayarları ve bunlardan üretilen tema (renkler, ölçüler).
//!
//! Tema, Direct2D ile çizilen arayüzün bütün renklerini ve boyutlarını
//! belirler. Kullanıcı ayarları (düzen, renk modu, vurgu, ton, yoğunluk)
//! burada somut değerlere çevrilir.

/// RGBA renk (0.0–1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn hex(rgb: u32) -> Color {
        Color {
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// `self` ile `other` arasında `t` (0 = self, 1 = other) oranında karışım.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let m = |a: f32, b: f32| a + (b - a) * t;
        Color {
            r: m(self.r, other.r),
            g: m(self.g, other.g),
            b: m(self.b, other.b),
            a: m(self.a, other.a),
        }
    }

    /// `#rrggbb` (SVG önizlemesi ve ayarlar sayfası için).
    pub fn css(self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 0.999 {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        } else {
            format!(
                "rgba({},{},{},{:.3})",
                c(self.r),
                c(self.g),
                c(self.b),
                self.a
            )
        }
    }

    /// Windows `COLORREF` (0x00BBGGRR).
    pub fn colorref(self) -> u32 {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        c(self.r) | c(self.g) << 8 | c(self.b) << 16
    }
}

// ------------------------------------------------------------- ayarlar

macro_rules! choice {
    ($name:ident { $($variant:ident = $id:literal / $label:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            /// Ayar dosyasında ve ayarlar sayfasında kullanılan kimlik.
            pub fn id(self) -> &'static str { match self { $($name::$variant => $id),+ } }
            /// Kullanıcıya gösterilen ad.
            pub fn label(self) -> &'static str { match self { $($name::$variant => $label),+ } }
            pub fn from_id(id: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|v| v.id() == id.trim())
            }
        }
    };
}

choice!(LayoutKind {
    Sidebar = "sidebar" / "Kenar çubuğu",
    Rail = "rail" / "İnce şerit",
    SingleRow = "single-row" / "Tek satır",
    Classic = "classic" / "Klasik",
});

choice!(ColorMode {
    Light = "light" / "Açık",
    Dark = "dark" / "Koyu",
    System = "system" / "Windows'a göre",
});

choice!(Accent {
    Violet = "violet" / "Mor",
    Blue = "blue" / "Mavi",
    Green = "green" / "Yeşil",
    Orange = "orange" / "Turuncu",
    Pink = "pink" / "Pembe",
    Graphite = "graphite" / "Grafit",
    Windows = "windows" / "Windows vurgu rengi",
});

choice!(Tone {
    Gray = "gray" / "Gri",
    Lavender = "lavender" / "Lavanta",
    Stone = "stone" / "Taş",
    Mint = "mint" / "Nane",
    Sky = "sky" / "Gökyüzü",
    Peach = "peach" / "Şeftali",
});

choice!(Density {
    Normal = "normal" / "Normal",
    Compact = "compact" / "Kompakt",
});

impl Accent {
    /// Ayarlar sayfasındaki renk örneği (açık tema tonu).
    pub fn swatch(self, system: Color) -> Color {
        match self {
            Accent::Violet => Color::hex(0x5b4bd6),
            Accent::Blue => Color::hex(0x2563c9),
            Accent::Green => Color::hex(0x2e7d52),
            Accent::Orange => Color::hex(0xb4500f),
            Accent::Pink => Color::hex(0xc0336f),
            Accent::Graphite => Color::hex(0x4a4760),
            Accent::Windows => system,
        }
    }
}

impl Tone {
    /// Ayarlar sayfasındaki renk örneği (açık/koyu zemin).
    pub fn swatch(self, dark: bool) -> Color {
        let (light, dark_c) = self.grounds();
        Color::hex(if dark { dark_c } else { light })
    }

    fn grounds(self) -> (u32, u32) {
        // Her ton rengini belli edecek kadar doygun; açık ve koyu karşılıkları
        // aynı renk ailesinden.
        match self {
            Tone::Gray => (0xf0f0f4, 0x1c1b22),
            Tone::Lavender => (0xdcd5f2, 0x1b1734),
            Tone::Stone => (0xe8e3d8, 0x221f1a),
            Tone::Mint => (0xd2eadc, 0x0f2119),
            Tone::Sky => (0xd2e2f7, 0x0f1c2e),
            Tone::Peach => (0xf7dac8, 0x2a1a12),
        }
    }
}

/// Kullanıcının görünüm ayarları.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub layout: LayoutKind,
    pub mode: ColorMode,
    pub accent: Accent,
    pub tone: Tone,
    pub density: Density,
}

impl Appearance {
    /// Ayarlar sayfasından gelen tek bir değişikliği uygular
    /// (`preset=focus`, `layout=rail` ...). Geçersizse `false`.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        match key {
            "preset" => Preset::from_id(value)
                .map(|p| *self = p.appearance())
                .is_some(),
            "layout" => LayoutKind::from_id(value)
                .map(|v| self.layout = v)
                .is_some(),
            "mode" => ColorMode::from_id(value).map(|v| self.mode = v).is_some(),
            "accent" => Accent::from_id(value).map(|v| self.accent = v).is_some(),
            "tone" => Tone::from_id(value).map(|v| self.tone = v).is_some(),
            "density" => Density::from_id(value).map(|v| self.density = v).is_some(),
            _ => false,
        }
    }
}

impl Default for Appearance {
    /// "Odak" teması.
    fn default() -> Self {
        Preset::Focus.appearance()
    }
}

choice!(Preset {
    Focus = "focus" / "Odak",
    Night = "night" / "Gece",
    Classic = "classic" / "Klasik",
    Minimal = "minimal" / "Minimal",
    Forest = "forest" / "Orman",
});

impl Preset {
    pub fn appearance(self) -> Appearance {
        use Accent as A;
        use ColorMode as M;
        use LayoutKind as L;
        use Tone as T;
        // 3 açık, 2 koyu tema; her biri ayrı bir renk ailesinde.
        let (layout, mode, accent, tone, density) = match self {
            Preset::Focus => (
                L::Sidebar,
                M::Light,
                A::Violet,
                T::Lavender,
                Density::Normal,
            ),
            Preset::Night => (L::Rail, M::Dark, A::Violet, T::Lavender, Density::Normal),
            Preset::Classic => (L::Classic, M::Light, A::Blue, T::Gray, Density::Normal),
            Preset::Minimal => (
                L::SingleRow,
                M::Light,
                A::Graphite,
                T::Stone,
                Density::Compact,
            ),
            Preset::Forest => (L::Sidebar, M::Dark, A::Green, T::Mint, Density::Normal),
        };
        Appearance {
            layout,
            mode,
            accent,
            tone,
            density,
        }
    }

    /// Ayarlar bu hazır temayla birebir aynıysa onu döndürür.
    #[cfg(test)]
    pub fn matching(appearance: &Appearance) -> Option<Preset> {
        Preset::ALL
            .iter()
            .copied()
            .find(|p| p.appearance() == *appearance)
    }
}

// --------------------------------------------------------------- tema

/// Çizimde kullanılan somut renkler.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Pencere zemini (sayfanın etrafındaki ton).
    pub ground: Color,
    /// Adres kutusu, seçili sekme gibi yükseltilmiş yüzeyler.
    pub surface: Color,
    /// Fareyle üzerine gelinen öğe.
    pub hover: Color,
    pub pressed: Color,
    /// Yer imi kutucukları, sekme rozetleri.
    pub well: Color,
    pub divider: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub accent: Color,
    /// Vurgu renginin hafif zemini (engelleyici rozeti).
    pub accent_soft: Color,
    /// `accent_soft` üzerindeki yazı.
    pub accent_text: Color,
    /// Sayfa çerçevesinin gölgesi.
    pub shadow: Color,
}

/// Ölçüler (DIP, yani 100% ölçekte piksel).
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub pad: f32,
    pub button: f32,
    pub row: f32,
    pub address_h: f32,
    pub radius: f32,
    pub font: f32,
    pub font_small: f32,
    pub sidebar_w: f32,
    pub rail_w: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub palette: Palette,
    pub metrics: Metrics,
}

impl Theme {
    /// Görünüm ayarlarından tema üretir. `system_dark`: Windows koyu temada mı;
    /// `system_accent`: Windows vurgu rengi.
    pub fn new(a: &Appearance, system_dark: bool, system_accent: Color) -> Theme {
        let dark = match a.mode {
            ColorMode::Light => false,
            ColorMode::Dark => true,
            ColorMode::System => system_dark,
        };
        let (ground_light, ground_dark) = a.tone.grounds();
        let accent = match a.accent {
            Accent::Violet => Color::hex(if dark { 0x9b8fff } else { 0x5b4bd6 }),
            Accent::Blue => Color::hex(if dark { 0x7aa8ff } else { 0x2563c9 }),
            Accent::Green => Color::hex(if dark { 0x63d39a } else { 0x2e7d52 }),
            Accent::Orange => Color::hex(if dark { 0xffa25e } else { 0xb4500f }),
            Accent::Pink => Color::hex(if dark { 0xff80b4 } else { 0xc0336f }),
            Accent::Graphite => Color::hex(if dark { 0xc3c0d2 } else { 0x4a4760 }),
            Accent::Windows => system_accent,
        };
        let palette = if dark {
            let ground = Color::hex(ground_dark);
            let white = Color::hex(0xffffff);
            Palette {
                ground,
                surface: ground.mix(white, 0.10),
                hover: white.alpha(0.07),
                pressed: white.alpha(0.12),
                well: ground.mix(white, 0.07),
                divider: white.alpha(0.10),
                text: Color::hex(0xecebf3),
                text_muted: Color::hex(0xb3b0c2),
                text_faint: Color::hex(0x7f7c90),
                accent,
                accent_soft: ground.mix(accent, 0.22),
                accent_text: accent.mix(white, 0.35),
                shadow: Color::hex(0x000000).alpha(0.35),
            }
        } else {
            let ground = Color::hex(ground_light);
            let ink = Color::hex(0x1f1d2b);
            Palette {
                ground,
                surface: Color::hex(0xffffff),
                hover: ink.alpha(0.06),
                pressed: ink.alpha(0.10),
                well: ground.mix(Color::hex(0xffffff), 0.55),
                divider: ink.alpha(0.10),
                text: ink,
                text_muted: Color::hex(0x4a4760),
                text_faint: Color::hex(0x6b6880),
                accent,
                accent_soft: ground.mix(accent, 0.16),
                accent_text: accent.mix(ink, 0.25),
                shadow: ink.alpha(0.10),
            }
        };
        let k: f32 = match a.density {
            Density::Normal => 1.0,
            Density::Compact => 0.86,
        };
        let metrics = Metrics {
            pad: (8.0 * k).round(),
            button: (32.0 * k).round(),
            row: (36.0 * k).round(),
            address_h: (38.0 * k).round(),
            radius: 10.0,
            font: if k < 1.0 { 12.5 } else { 13.0 },
            font_small: 11.5,
            sidebar_w: 264.0,
            rail_w: (52.0 * k).round(),
        };
        Theme {
            dark,
            palette,
            metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for l in LayoutKind::ALL {
            assert_eq!(LayoutKind::from_id(l.id()), Some(*l));
        }
        for p in Preset::ALL {
            assert_eq!(Preset::from_id(p.id()), Some(*p));
        }
        assert_eq!(Accent::from_id("yok"), None);
    }

    #[test]
    fn presets_are_distinct_and_detected() {
        for p in Preset::ALL {
            assert_eq!(Preset::matching(&p.appearance()), Some(*p));
        }
        assert_eq!(Appearance::default(), Preset::Focus.appearance());
        let custom = Appearance {
            tone: Tone::Peach,
            ..Appearance::default()
        };
        assert_eq!(Preset::matching(&custom), None);
    }

    #[test]
    fn system_mode_follows_windows() {
        let a = Appearance {
            mode: ColorMode::System,
            ..Appearance::default()
        };
        let accent = Color::hex(0x0078d4);
        assert!(!Theme::new(&a, false, accent).dark);
        assert!(Theme::new(&a, true, accent).dark);
        let light = Appearance {
            mode: ColorMode::Light,
            ..a
        };
        assert!(!Theme::new(&light, true, accent).dark);
    }

    #[test]
    fn windows_accent_is_used() {
        let a = Appearance {
            accent: Accent::Windows,
            ..Appearance::default()
        };
        let accent = Color::hex(0x0078d4);
        assert_eq!(Theme::new(&a, false, accent).palette.accent, accent);
    }

    #[test]
    fn presets_look_different() {
        // Her hazır tema zemin rengiyle de ayırt edilebilmeli.
        let accent = Color::hex(0x0078d4);
        let grounds: Vec<Color> = Preset::ALL
            .iter()
            .map(|p| Theme::new(&p.appearance(), false, accent).palette.ground)
            .collect();
        for (i, a) in grounds.iter().enumerate() {
            for b in &grounds[i + 1..] {
                let d = (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs();
                assert!(d > 0.06, "iki hazır temanın zemini neredeyse aynı");
            }
        }
        let dark = Preset::ALL
            .iter()
            .filter(|p| Theme::new(&p.appearance(), false, accent).dark)
            .count();
        assert_eq!(dark, 2, "açık Windows'ta da iki koyu tema olmalı");
    }

    #[test]
    fn applies_single_changes() {
        let mut a = Appearance::default();
        assert!(a.set("layout", "rail"));
        assert_eq!(a.layout, LayoutKind::Rail);
        assert!(a.set("tone", "mint"));
        assert!(!a.set("tone", "yok"));
        assert!(!a.set("bilinmeyen", "x"));
        assert!(a.set("preset", "night"));
        assert_eq!(a, Preset::Night.appearance());
    }

    #[test]
    fn colors_convert() {
        assert_eq!(Color::hex(0x5b4bd6).css(), "#5b4bd6");
        assert_eq!(Color::hex(0x112233).colorref(), 0x332211);
    }
}
