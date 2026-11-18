//! Dört düzenin yerleşimi: neyin nereye çizileceği ve nereye tıklanınca ne
//! olacağı.
//!
//! Bütün ölçüler DIP cinsindendir (100% ölçekte piksel); Direct2D ekran
//! ölçeğine göre kendisi büyütür. Görünüm Windows uygulamalarına yakın
//! tutulur: küçük köşeler, alçak satırlar, site ikonları, gölge yerine ince
//! kenar çizgileri.

use super::scene::{Align, Icon, Prim, Rect};
use super::theme::{Color, LayoutKind, Theme};

/// Sekme şeridinde gösterilen bilgiler.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabInfo {
    pub title: String,
    /// Site ikonunun anahtarı (alan adı); yoksa küre ikonu çizilir.
    pub icon: Option<String>,
    pub active: bool,
    /// Ses ya da video çalıyor.
    pub audio: bool,
    /// Dondurulmuş ya da bellekten atılmış.
    pub sleeping: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookmarkInfo {
    pub title: String,
    pub url: String,
    pub icon: Option<String>,
}

/// Arayüzün o anki içeriği.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub tabs: Vec<TabInfo>,
    pub bookmarks: Vec<BookmarkInfo>,
    pub can_back: bool,
    pub can_forward: bool,
    pub bookmarked: bool,
    /// Sayfa yer imine eklenebilir mi (başlangıç sayfası eklenemez).
    pub star_enabled: bool,
    /// Engelleyici sayacı ("128", "Kapalı", "…").
    pub shield: String,
    /// Engelleyici bu sitede kapalı (sayaç soluk çizilir).
    pub shield_off: bool,
    /// Sekme listesinin kaydırma miktarı.
    pub tab_scroll: f32,
}

/// Tıklanabilir öğeler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Back,
    Forward,
    Reload,
    Tab(usize),
    CloseTab(usize),
    NewTab,
    Star,
    Shield,
    History,
    Settings,
    Bookmark(usize),
}

/// Fare durumu: üzerine gelinen ve basılı tutulan öğe.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pointer {
    pub hover: Option<Target>,
    pub pressed: Option<Target>,
}

/// Yerleşimin sonucu.
#[derive(Debug, Default)]
pub struct Layout {
    pub prims: Vec<Prim>,
    /// Tıklanabilir alanlar; sonra eklenen öne çıkar.
    pub hits: Vec<(Rect, Target)>,
    /// Sayfanın (WebView) yeri.
    pub content: Rect,
    pub content_radius: f32,
    /// Adres yazı kutusunun (Win32 EDIT) yeri.
    pub address: Rect,
    /// Fare tekerleğiyle kaydırılan sekme listesi ve en fazla kaydırma.
    pub scroll_area: Option<Rect>,
    pub scroll_max: f32,
}

impl Layout {
    /// Noktadaki tıklanabilir öğe.
    pub fn hit(&self, x: f32, y: f32) -> Option<Target> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, t)| *t)
    }
}

/// Arayüzü verilen pencere boyutuna (DIP) yerleştirir.
pub fn layout(
    kind: LayoutKind,
    theme: &Theme,
    model: &Model,
    width: f32,
    height: f32,
    pointer: Pointer,
) -> Layout {
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer,
    };
    let (w, h) = (width.max(320.0), height.max(240.0));
    b.fill(Rect::new(0.0, 0.0, w, h), 0.0, theme.palette.ground);
    match kind {
        LayoutKind::Sidebar => sidebar(&mut b, model, w, h),
        LayoutKind::Rail => rail(&mut b, model, w, h),
        LayoutKind::SingleRow => single_row(&mut b, model, w, h),
        LayoutKind::Classic => classic(&mut b, model, w, h),
    }
    b.out
}

/// Sürüklenen sekmenin (`from`) imleç (`x`, `y`) noktasına göre yeni sırası.
/// Sekme, imleç komşu sekmenin ortasını geçince yer değiştirir; hızlı
/// sürüklemede birden fazla sekmenin üstünden atlayabilir. Yer değişmiyorsa
/// `None`. `horizontal`: sekmeler yan yana mı (değilse alt alta).
pub fn drag_target(
    layout: &Layout,
    from: usize,
    x: f32,
    y: f32,
    horizontal: bool,
) -> Option<usize> {
    let pos = if horizontal { x } else { y };
    let mut to = from;
    for (r, target) in &layout.hits {
        let Target::Tab(j) = *target else {
            continue;
        };
        let center = if horizontal {
            r.x + r.w / 2.0
        } else {
            r.y + r.h / 2.0
        };
        if (j > from && pos > center && j > to) || (j < from && pos < center && j < to) {
            to = j;
        }
    }
    (to != from).then_some(to)
}

/// Bir öğe `from` sırasından `to` sırasına taşınınca `index` sırasındaki
/// öğenin yeni sırası (ör. öndeki sekme).
pub fn follow_move(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}

/// Adres çubuğu önerilerinin bir satırı.
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestionRow {
    /// Arama satırı (büyüteç); değilse site ikonu çizilir.
    pub search: bool,
    pub icon: Option<String>,
    pub title: String,
    pub detail: String,
}

/// Öneri listesinin çizimi; adres çubuğunun altındaki ayrı pencerede.
#[derive(Debug, Default)]
pub struct Popup {
    pub prims: Vec<Prim>,
    /// Satırların tıklanabilir alanları (satır sırası).
    pub hits: Vec<(Rect, usize)>,
    pub height: f32,
}

impl Popup {
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, i)| *i)
    }
}

const SUGGEST_ROW: f32 = 34.0;
const SUGGEST_PAD: f32 = 6.0;

/// Öneri satırlarını `width` genişliğindeki listeye yerleştirir.
pub fn suggestions(
    theme: &Theme,
    rows: &[SuggestionRow],
    selected: usize,
    hover: Option<usize>,
    width: f32,
) -> Popup {
    let p = &theme.palette;
    let m = &theme.metrics;
    let height = SUGGEST_PAD * 2.0 + rows.len() as f32 * SUGGEST_ROW;
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer: Pointer::default(),
    };
    let all = Rect::new(0.0, 0.0, width, height);
    b.fill(all, 0.0, p.surface);
    b.border(all, 8.0, p.border);
    let mut hits = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let r = Rect::new(
            SUGGEST_PAD,
            SUGGEST_PAD + i as f32 * SUGGEST_ROW,
            width - SUGGEST_PAD * 2.0,
            SUGGEST_ROW,
        );
        // Liste zemini zaten `surface`; seçili satır için bir ton koyusu.
        if i == selected {
            b.fill(r, m.radius, p.pressed);
        } else if hover == Some(i) {
            b.fill(r, m.radius, p.hover);
        }
        let icon = Rect::new(r.x + 10.0, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0);
        if row.search {
            b.icon(Icon::Search, icon, p.text_muted);
        } else {
            b.favicon(icon, &row.icon, false);
        }
        let x = icon.right() + 12.0;
        let avail = (r.right() - 10.0 - x).max(0.0);
        let title_w = if row.detail.is_empty() {
            avail
        } else {
            (avail * 0.6).floor()
        };
        b.text(
            Rect::new(x, r.y, title_w, r.h),
            &row.title,
            m.font,
            false,
            p.text,
        );
        if !row.detail.is_empty() {
            let dx = x + title_w + 12.0;
            b.text(
                Rect::new(dx, r.y, (r.right() - 10.0 - dx).max(0.0), r.h),
                &row.detail,
                m.font_small,
                false,
                p.text_muted,
            );
        }
        hits.push((r, i));
    }
    Popup {
        prims: b.out.prims,
        hits,
        height,
    }
}

struct Builder<'a> {
    out: Layout,
    theme: &'a Theme,
    pointer: Pointer,
}

impl Builder<'_> {
    fn fill(&mut self, rect: Rect, radius: f32, color: Color) {
        self.out.prims.push(Prim::Fill {
            rect,
            radius,
            color,
        });
    }

    fn border(&mut self, rect: Rect, radius: f32, color: Color) {
        self.out.prims.push(Prim::Border {
            rect,
            radius,
            color,
        });
    }

    fn text(&mut self, rect: Rect, text: &str, size: f32, bold: bool, color: Color) {
        self.out.prims.push(Prim::Text {
            rect,
            text: text.to_string(),
            size,
            bold,
            color,
            align: Align::Left,
        });
    }

    fn icon(&mut self, icon: Icon, rect: Rect, color: Color) {
        self.out.prims.push(Prim::Icon { icon, rect, color });
    }

    fn favicon(&mut self, rect: Rect, key: &Option<String>, faded: bool) {
        let p = self.theme.palette;
        self.out.prims.push(Prim::Favicon {
            rect,
            key: key.clone(),
            color: if faded { p.text_faint } else { p.text_muted },
        });
    }

    fn hit(&mut self, rect: Rect, target: Target) {
        if rect.w > 0.0 && rect.h > 0.0 {
            self.out.hits.push((rect, target));
        }
    }

    fn hovered(&self, target: Target) -> bool {
        self.pointer.hover == Some(target)
    }

    /// Fareyle üzerine gelinen/basılan öğenin zemini.
    fn hover_bg(&mut self, rect: Rect, radius: f32, target: Target) {
        let p = &self.theme.palette;
        if self.pointer.pressed == Some(target) {
            self.fill(rect, radius, p.pressed);
        } else if self.hovered(target) {
            self.fill(rect, radius, p.hover);
        }
    }

    fn icon_button(&mut self, rect: Rect, icon: Icon, target: Target, enabled: bool) {
        let p = self.theme.palette;
        if enabled {
            self.hover_bg(rect, 4.0, target);
            self.hit(rect, target);
        }
        let color = if enabled {
            p.text_muted
        } else {
            p.text_faint.alpha(0.5)
        };
        self.icon(icon, rect.centered_square(16.0), color);
    }

    /// Engelleyici: kalkan ikonu ve yanında sayı. Gereksiz zemin yok.
    fn shield(&mut self, rect: Rect, model: &Model) {
        let p = self.theme.palette;
        self.hover_bg(rect, 4.0, Target::Shield);
        let color = if model.shield_off {
            p.text_faint
        } else {
            p.text_muted
        };
        let icon = Rect::new(rect.x + 6.0, rect.y + (rect.h - 16.0) / 2.0, 16.0, 16.0);
        self.icon(Icon::Shield, icon, color);
        let count = if model.shield_off {
            p.text_faint
        } else {
            p.accent
        };
        let size = self.theme.metrics.font_small;
        let text = Rect::new(
            icon.right() + 4.0,
            rect.y,
            rect.right() - icon.right() - 6.0,
            rect.h,
        );
        self.text(text, &model.shield, size, true, count);
        self.hit(rect, Target::Shield);
    }

    fn shield_width(model: &Model) -> f32 {
        34.0 + 6.5 * model.shield.chars().count() as f32
    }

    /// Adres kutusu: zemin, ince kenar ve sağda yıldız. Yazı alanını
    /// `out.address`'e yazar.
    fn address_box(&mut self, rect: Rect, model: &Model) {
        let p = self.theme.palette;
        let r = self.theme.metrics.radius;
        self.fill(rect, r, p.surface);
        self.border(rect, r, p.border);
        let star = Rect::new(
            rect.right() - 30.0,
            rect.y + (rect.h - 26.0) / 2.0,
            26.0,
            26.0,
        );
        if model.star_enabled {
            self.hover_bg(star, 4.0, Target::Star);
            self.hit(star, Target::Star);
        }
        let (icon, color) = if model.bookmarked {
            (Icon::StarFilled, p.accent)
        } else if model.star_enabled {
            (Icon::Star, p.text_faint)
        } else {
            (Icon::Star, p.text_faint.alpha(0.35))
        };
        self.icon(icon, star.centered_square(15.0), color);
        let text_h = (self.theme.metrics.font * 1.7).round();
        self.out.address = Rect::new(
            rect.x + 10.0,
            rect.y + (rect.h - text_h) / 2.0,
            (star.x - rect.x - 14.0).max(0.0),
            text_h,
        );
    }

    /// Sayfa çerçevesi: düz yüzey ve ince kenar.
    fn content(&mut self, rect: Rect, radius: f32) {
        let p = self.theme.palette;
        self.fill(rect, radius, p.surface);
        self.border(rect, radius, p.border);
        self.out.content = rect;
        self.out.content_radius = radius;
    }

    /// Dikey sekme satırı (kenar çubuğu).
    fn tab_row(&mut self, r: Rect, area: Rect, i: usize, tab: &TabInfo) {
        let p = self.theme.palette;
        let m = self.theme.metrics;
        if tab.active {
            self.fill(r, m.radius, p.selected);
            self.fill(Rect::new(r.x, r.y + 7.0, 3.0, r.h - 14.0), 1.5, p.accent);
        } else {
            self.hover_bg(r, m.radius, Target::Tab(i));
        }
        self.hit(r.intersect(&area), Target::Tab(i));
        self.favicon(
            Rect::new(r.x + 10.0, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0),
            &tab.icon,
            tab.sleeping,
        );
        let mut right = r.right() - 4.0;
        if tab.active || self.hovered(Target::Tab(i)) || self.hovered(Target::CloseTab(i)) {
            let close = Rect::new(right - 22.0, r.y + (r.h - 22.0) / 2.0, 22.0, 22.0);
            self.hover_bg(close, 4.0, Target::CloseTab(i));
            self.icon(Icon::Close, close.centered_square(12.0), p.text_faint);
            self.hit(close.intersect(&area), Target::CloseTab(i));
            right = close.x - 2.0;
        }
        if tab.audio {
            let icon = Rect::new(right - 15.0, r.y + (r.h - 13.0) / 2.0, 13.0, 13.0);
            self.icon(Icon::Speaker, icon, p.text_muted);
            right = icon.x - 4.0;
        }
        let color = if tab.sleeping {
            p.text_faint
        } else if tab.active {
            p.text
        } else {
            p.text_muted
        };
        let text = Rect::new(r.x + 34.0, r.y, right - r.x - 36.0, r.h);
        self.text(text, tab_title(tab), m.font, false, color);
    }

    /// Yer imleri: başlıksız, yalnızca site ikonlarından oluşan sıra.
    fn bookmark_icons(&mut self, model: &Model, x: f32, y: f32, width: f32, max: usize) -> f32 {
        let cell = 30.0;
        let per_row = ((width + 2.0) / (cell + 2.0)).floor().max(1.0) as usize;
        let count = model.bookmarks.len().min(max);
        for (i, mark) in model.bookmarks.iter().take(count).enumerate() {
            let r = Rect::new(
                x + (i % per_row) as f32 * (cell + 2.0),
                y + (i / per_row) as f32 * (cell + 2.0),
                cell,
                cell,
            );
            self.hover_bg(r, 4.0, Target::Bookmark(i));
            self.favicon(r.centered_square(16.0), &mark.icon, false);
            self.hit(r, Target::Bookmark(i));
        }
        if count == 0 {
            0.0
        } else {
            count.div_ceil(per_row) as f32 * (cell + 2.0)
        }
    }
}

fn tab_title(tab: &TabInfo) -> &str {
    let t = tab.title.trim();
    if t.is_empty() { "Yeni sekme" } else { t }
}

/// Geri, ileri, yenile.
fn nav_buttons(b: &mut Builder, x: f32, y: f32, model: &Model) -> f32 {
    let s = b.theme.metrics.button;
    b.icon_button(
        Rect::new(x, y, s, s),
        Icon::Back,
        Target::Back,
        model.can_back,
    );
    b.icon_button(
        Rect::new(x + s, y, s, s),
        Icon::Forward,
        Target::Forward,
        model.can_forward,
    );
    b.icon_button(
        Rect::new(x + 2.0 * s, y, s, s),
        Icon::Reload,
        Target::Reload,
        true,
    );
    x + 3.0 * s
}

// ------------------------------------------------------------ A: kenar çubuğu

fn sidebar(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let pad = m.pad;
    let side_w = m.sidebar_w.min(w * 0.4);
    let side = Rect::new(pad, pad, side_w, h - 2.0 * pad);
    b.content(
        Rect::new(
            side.right() + pad,
            pad,
            w - side_w - 3.0 * pad,
            h - 2.0 * pad,
        ),
        8.0,
    );

    let mut y = side.y;
    nav_buttons(b, side.x, y, model);
    let shield_w = Builder::shield_width(model);
    b.shield(
        Rect::new(side.right() - shield_w, y, shield_w, m.button),
        model,
    );
    y += m.button + 6.0;

    b.address_box(Rect::new(side.x, y, side.w, m.address_h), model);
    y += m.address_h + 8.0;

    let used = b.bookmark_icons(model, side.x, y, side.w, 16);
    if used > 0.0 {
        y += used + 4.0;
    }
    b.fill(
        Rect::new(side.x + 4.0, y, side.w - 8.0, 1.0),
        0.0,
        p.divider,
    );
    y += 7.0;

    // Alt satır: geçmiş ve ayarlar.
    let foot = Rect::new(side.x, side.bottom() - m.button, side.w, m.button);
    b.icon_button(
        Rect::new(foot.x, foot.y, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(foot.x + m.button, foot.y, m.button, m.button),
        Icon::Settings,
        Target::Settings,
        true,
    );

    // Sekmeler: kaydırılabilir dikey liste.
    let area = Rect::new(side.x, y, side.w, (foot.y - 6.0 - y).max(0.0));
    let step = m.row + 1.0;
    let total = (model.tabs.len() + 1) as f32 * step;
    let max = (total - area.h).max(0.0);
    let offset = model.tab_scroll.clamp(0.0, max);
    b.out.scroll_area = Some(area);
    b.out.scroll_max = max;
    b.out.prims.push(Prim::PushClip(area));
    for (i, tab) in model.tabs.iter().enumerate() {
        let r = Rect::new(area.x, area.y + i as f32 * step - offset, area.w, m.row);
        if r.bottom() >= area.y && r.y <= area.bottom() {
            b.tab_row(r, area, i, tab);
        }
    }
    let r = Rect::new(
        area.x,
        area.y + model.tabs.len() as f32 * step - offset,
        area.w,
        m.row,
    );
    if r.bottom() >= area.y && r.y <= area.bottom() {
        b.hover_bg(r, m.radius, Target::NewTab);
        b.icon(
            Icon::Plus,
            Rect::new(r.x + 10.0, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0),
            p.text_faint,
        );
        b.text(
            Rect::new(r.x + 34.0, r.y, r.w - 36.0, r.h),
            "Yeni sekme",
            m.font,
            false,
            p.text_faint,
        );
        b.hit(r.intersect(&area), Target::NewTab);
    }
    b.out.prims.push(Prim::PopClip);
}

// ------------------------------------------------------------ B: ince şerit

fn rail(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let pad = m.pad;
    let rail = Rect::new(0.0, pad, m.rail_w, h - 2.0 * pad);
    let x0 = rail.right();
    let bar = Rect::new(x0, pad, w - x0 - pad, m.button);
    b.content(
        Rect::new(
            x0,
            bar.bottom() + pad,
            w - x0 - pad,
            h - bar.bottom() - 2.0 * pad,
        ),
        8.0,
    );

    // Üst çubuk: gezinme, ortada adres, sağda engelleyici.
    let after_nav = nav_buttons(b, bar.x, bar.y, model);
    let shield_w = Builder::shield_width(model);
    b.shield(
        Rect::new(bar.right() - shield_w, bar.y, shield_w, bar.h),
        model,
    );
    let free = (bar.right() - shield_w - 8.0) - (after_nav + 8.0);
    let addr_w = free.clamp(120.0, 640.0);
    let addr_x = after_nav + 8.0 + ((free - addr_w) / 2.0).max(0.0);
    b.address_box(Rect::new(addr_x, bar.y, addr_w, bar.h), model);

    // Şerit: yer imleri, sekmeler, yeni sekme; altta geçmiş ve ayarlar.
    let cell = (m.rail_w - 12.0).round();
    let cx = rail.x + (rail.w - cell) / 2.0;
    let mut y = rail.y;
    for (i, mark) in model.bookmarks.iter().take(6).enumerate() {
        let r = Rect::new(cx, y, cell, cell);
        b.hover_bg(r, m.radius, Target::Bookmark(i));
        b.favicon(r.centered_square(16.0), &mark.icon, false);
        b.hit(r, Target::Bookmark(i));
        y += cell + 2.0;
    }
    if !model.bookmarks.is_empty() {
        b.fill(
            Rect::new(rail.x + 12.0, y + 3.0, rail.w - 24.0, 1.0),
            0.0,
            p.divider,
        );
        y += 8.0;
    }
    let foot_h = 2.0 * m.button;
    let area = Rect::new(
        rail.x,
        y,
        rail.w,
        (rail.bottom() - foot_h - 6.0 - y).max(0.0),
    );
    let step = cell + 2.0;
    let total = (model.tabs.len() + 1) as f32 * step;
    let max = (total - area.h).max(0.0);
    let offset = model.tab_scroll.clamp(0.0, max);
    b.out.scroll_area = Some(area);
    b.out.scroll_max = max;
    b.out.prims.push(Prim::PushClip(area));
    for (i, tab) in model.tabs.iter().enumerate() {
        let r = Rect::new(cx, area.y + i as f32 * step - offset, cell, cell);
        if r.bottom() < area.y || r.y > area.bottom() {
            continue;
        }
        if tab.active {
            b.fill(r, m.radius, p.selected);
            b.fill(Rect::new(rail.x, r.y + 8.0, 3.0, r.h - 16.0), 1.5, p.accent);
        } else {
            b.hover_bg(r, m.radius, Target::Tab(i));
        }
        b.hit(r.intersect(&area), Target::Tab(i));
        b.favicon(r.centered_square(16.0), &tab.icon, tab.sleeping);
        if tab.audio {
            b.fill(
                Rect::new(r.right() - 9.0, r.bottom() - 9.0, 6.0, 6.0),
                3.0,
                p.accent,
            );
        }
        if b.hovered(Target::Tab(i)) || b.hovered(Target::CloseTab(i)) {
            let close = Rect::new(r.right() - 14.0, r.y - 1.0, 15.0, 15.0);
            b.fill(close, 7.5, p.surface);
            b.border(close, 7.5, p.border);
            b.icon(Icon::Close, close.centered_square(9.0), p.text_muted);
            b.hit(close.intersect(&area), Target::CloseTab(i));
        }
    }
    let r = Rect::new(
        cx,
        area.y + model.tabs.len() as f32 * step - offset,
        cell,
        cell,
    );
    if r.bottom() >= area.y && r.y <= area.bottom() {
        b.icon_button(r, Icon::Plus, Target::NewTab, true);
    }
    b.out.prims.push(Prim::PopClip);
    let hy = rail.bottom() - foot_h;
    let fx = rail.x + (rail.w - m.button) / 2.0;
    b.icon_button(
        Rect::new(fx, hy, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(fx, hy + m.button, m.button, m.button),
        Icon::Settings,
        Target::Settings,
        true,
    );
}

// ------------------------------------------------------------ C: tek satır

/// Yatay sekmeler; her sekmenin genişliği alana göre daralır.
fn tab_strip(b: &mut Builder, model: &Model, area: Rect, max_w: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let plus = m.button;
    let n = model.tabs.len().max(1) as f32;
    let tab_w = ((area.w - plus - 4.0) / n - 2.0).clamp(36.0, max_w);
    let mut x = area.x;
    b.out.prims.push(Prim::PushClip(area));
    for (i, tab) in model.tabs.iter().enumerate() {
        let r = Rect::new(x, area.y, tab_w, area.h);
        if r.x > area.right() {
            break;
        }
        if tab.active {
            b.fill(r, m.radius, p.surface);
            b.border(r, m.radius, p.border);
        } else {
            b.hover_bg(r, m.radius, Target::Tab(i));
        }
        b.hit(r.intersect(&area), Target::Tab(i));
        let compact = tab_w < 80.0;
        let icon = if compact {
            r.centered_square(16.0)
        } else {
            Rect::new(r.x + 9.0, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0)
        };
        b.favicon(icon, &tab.icon, tab.sleeping);
        if !compact {
            let mut right = r.right() - 4.0;
            if tab.active || b.hovered(Target::Tab(i)) || b.hovered(Target::CloseTab(i)) {
                let close = Rect::new(right - 20.0, r.y + (r.h - 20.0) / 2.0, 20.0, 20.0);
                b.hover_bg(close, 4.0, Target::CloseTab(i));
                b.icon(Icon::Close, close.centered_square(11.0), p.text_faint);
                b.hit(close.intersect(&area), Target::CloseTab(i));
                right = close.x - 2.0;
            }
            if tab.audio {
                let a = Rect::new(right - 14.0, r.y + (r.h - 12.0) / 2.0, 12.0, 12.0);
                b.icon(Icon::Speaker, a, p.text_muted);
                right = a.x - 3.0;
            }
            let color = if tab.sleeping {
                p.text_faint
            } else if tab.active {
                p.text
            } else {
                p.text_muted
            };
            let text = Rect::new(icon.right() + 8.0, r.y, right - icon.right() - 10.0, r.h);
            b.text(text, tab_title(tab), m.font, false, color);
        } else if tab.audio {
            b.fill(
                Rect::new(r.right() - 8.0, r.bottom() - 8.0, 5.0, 5.0),
                2.5,
                p.accent,
            );
        }
        x += tab_w + 2.0;
    }
    b.out.prims.push(Prim::PopClip);
    let plus_x = x.min(area.right() - plus);
    b.icon_button(
        Rect::new(plus_x, area.y + (area.h - plus) / 2.0, plus, plus),
        Icon::Plus,
        Target::NewTab,
        true,
    );
}

fn single_row(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let pad = m.pad;
    let row_h = m.button + 2.0 * pad;
    let y = pad;
    b.content(Rect::new(pad, row_h, w - 2.0 * pad, h - row_h - pad), 6.0);

    let x = nav_buttons(b, pad, y, model);
    let addr_w = (w * 0.3).clamp(220.0, 420.0);
    let addr = Rect::new(x + 4.0, y, addr_w, m.button);
    b.address_box(addr, model);
    let shield_w = Builder::shield_width(model);
    b.shield(Rect::new(addr.right() + 2.0, y, shield_w, m.button), model);
    let div_x = addr.right() + shield_w + 8.0;
    b.fill(
        Rect::new(div_x, y + 7.0, 1.0, m.button - 14.0),
        0.0,
        p.divider,
    );

    let buttons = 2.0 * m.button;
    let tabs = Rect::new(
        div_x + 7.0,
        y,
        w - pad - buttons - 6.0 - (div_x + 7.0),
        m.button,
    );
    tab_strip(b, model, tabs, 200.0);
    let hx = w - pad - buttons;
    b.icon_button(
        Rect::new(hx, y, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(hx + m.button, y, m.button, m.button),
        Icon::Settings,
        Target::Settings,
        true,
    );
}

// ------------------------------------------------------------ D: klasik

fn classic(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let pad = m.pad;
    tab_strip(
        b,
        model,
        Rect::new(pad, pad, w - 2.0 * pad, m.button),
        220.0,
    );

    let y = pad + m.button + 4.0;
    let x = nav_buttons(b, pad, y, model);
    let buttons = 2.0 * m.button;
    let shield_w = Builder::shield_width(model);
    let right = w - pad - buttons;
    b.shield(
        Rect::new(right - shield_w - 2.0, y, shield_w, m.button),
        model,
    );
    b.address_box(
        Rect::new(x + 4.0, y, right - shield_w - 8.0 - x, m.button),
        model,
    );
    b.icon_button(
        Rect::new(right, y, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(right + m.button, y, m.button, m.button),
        Icon::Settings,
        Target::Settings,
        true,
    );

    let top = y + m.button + pad;
    b.content(Rect::new(pad, top, w - 2.0 * pad, h - top - pad), 6.0);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ui::theme::{Appearance, Color, Preset};

    pub fn sample_model() -> Model {
        Model {
            tabs: vec![
                TabInfo {
                    title: "Rust (programlama dili) — Vikipedi".into(),
                    icon: Some("tr.wikipedia.org".into()),
                    active: true,
                    ..Default::default()
                },
                TabInfo {
                    title: "Lo-fi çalışma listesi".into(),
                    icon: Some("muzik.example".into()),
                    audio: true,
                    ..Default::default()
                },
                TabInfo {
                    title: "julie: hızlı tarayıcı — PR #1".into(),
                    ..Default::default()
                },
                TabInfo {
                    title: "Hava durumu".into(),
                    sleeping: true,
                    ..Default::default()
                },
            ],
            bookmarks: vec![
                BookmarkInfo {
                    title: "Posta".into(),
                    url: "https://mail.example/".into(),
                    icon: Some("mail.example".into()),
                },
                BookmarkInfo {
                    title: "".into(),
                    url: "https://www.takvim.example/".into(),
                    icon: None,
                },
                BookmarkInfo {
                    title: "Müzik".into(),
                    url: "https://muzik.example/".into(),
                    icon: Some("muzik.example".into()),
                },
            ],
            can_back: true,
            can_forward: false,
            bookmarked: true,
            star_enabled: true,
            shield: "128".into(),
            shield_off: false,
            tab_scroll: 0.0,
        }
    }

    fn theme() -> Theme {
        Theme::new(&Appearance::default(), false, Color::hex(0x0078d4))
    }

    #[test]
    fn every_layout_places_content_and_address_inside_the_window() {
        let model = sample_model();
        for preset in Preset::ALL {
            let a = preset.appearance();
            let t = Theme::new(&a, false, Color::hex(0x0078d4));
            for (w, h) in [(1280.0, 800.0), (800.0, 600.0)] {
                let l = layout(a.layout, &t, &model, w, h, Pointer::default());
                let win = Rect::new(0.0, 0.0, w, h);
                assert_eq!(l.content.intersect(&win), l.content, "{preset:?} {w}");
                assert!(
                    l.content.w > w * 0.5 && l.content.h > h * 0.6,
                    "{preset:?}: sayfa alanı küçük"
                );
                assert!(l.address.w > 80.0, "{preset:?}: adres kutusu dar");
                assert!(
                    l.address.intersect(&l.content).w == 0.0
                        || l.address.intersect(&l.content).h == 0.0
                );
            }
        }
    }

    #[test]
    fn every_layout_exposes_all_controls() {
        let model = sample_model();
        let t = theme();
        for kind in LayoutKind::ALL {
            let l = layout(*kind, &t, &model, 1280.0, 800.0, Pointer::default());
            let has = |target: Target| l.hits.iter().any(|(_, t)| *t == target);
            for target in [
                Target::Back,
                Target::Reload,
                Target::NewTab,
                Target::Star,
                Target::Shield,
                Target::History,
                Target::Settings,
            ] {
                assert!(has(target), "{kind:?}: {target:?} yok");
            }
            assert!(
                !has(Target::Forward),
                "{kind:?}: ileri gidilemezken buton tıklanabilir olmamalı"
            );
            for i in 0..model.tabs.len() {
                assert!(has(Target::Tab(i)), "{kind:?}: sekme {i} yok");
            }
            // Öndeki sekmenin kapatma düğmesi sekmenin önünde olmalı.
            if *kind != LayoutKind::Rail {
                let (r, _) = l
                    .hits
                    .iter()
                    .find(|(_, t)| *t == Target::CloseTab(0))
                    .unwrap();
                assert_eq!(
                    l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                    Some(Target::CloseTab(0))
                );
            }
        }
    }

    #[test]
    fn sidebar_tab_list_scrolls() {
        let mut model = sample_model();
        model.tabs = (0..60)
            .map(|i| TabInfo {
                title: format!("Sekme {i}"),
                active: i == 0,
                ..Default::default()
            })
            .collect();
        let t = theme();
        let l = layout(
            LayoutKind::Sidebar,
            &t,
            &model,
            1280.0,
            800.0,
            Pointer::default(),
        );
        assert!(l.scroll_max > 0.0);
        let area = l.scroll_area.unwrap();
        for (r, target) in &l.hits {
            if matches!(target, Target::Tab(_) | Target::CloseTab(_)) {
                assert_eq!(r.intersect(&area), *r, "sekme listenin dışına taşmamalı");
            }
        }

        model.tab_scroll = l.scroll_max;
        let l = layout(
            LayoutKind::Sidebar,
            &t,
            &model,
            1280.0,
            800.0,
            Pointer::default(),
        );
        assert!(l.hits.iter().any(|(_, t)| *t == Target::Tab(59)));
        assert!(!l.hits.iter().any(|(_, t)| *t == Target::Tab(0)));
    }

    #[test]
    fn suggestion_rows_stack_and_hit() {
        let theme = Theme::new(&Default::default(), false, Color::hex(0x0078d4));
        let rows = vec![
            SuggestionRow {
                search: true,
                icon: None,
                title: "rust".into(),
                detail: "Google ile ara".into(),
            },
            SuggestionRow {
                search: false,
                icon: Some("rust-lang.org".into()),
                title: "Rust".into(),
                detail: "rust-lang.org".into(),
            },
        ];
        let popup = suggestions(&theme, &rows, 0, Some(1), 600.0);
        assert_eq!(popup.height, 6.0 * 2.0 + 34.0 * 2.0);
        assert_eq!(popup.hit(100.0, 10.0), Some(0));
        assert_eq!(popup.hit(100.0, 50.0), Some(1));
        assert_eq!(popup.hit(100.0, 2.0), None);
        let texts: Vec<_> = popup
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["rust", "Google ile ara", "Rust", "rust-lang.org"]);
        assert!(popup.prims.iter().any(|p| matches!(
            p,
            Prim::Icon {
                icon: Icon::Search,
                ..
            }
        )));
    }

    #[test]
    fn dragging_a_tab_swaps_after_the_neighbours_middle() {
        let mut layout = Layout::default();
        for i in 0..4 {
            layout.hits.push((
                Rect::new(i as f32 * 100.0, 0.0, 100.0, 30.0),
                Target::Tab(i),
            ));
            layout.hits.push((
                Rect::new(i as f32 * 100.0 + 80.0, 5.0, 16.0, 16.0),
                Target::CloseTab(i),
            ));
        }
        // 1. sekme (100–200) sağa sürükleniyor: 2.'nin ortası 250.
        assert_eq!(drag_target(&layout, 1, 240.0, 10.0, true), None);
        assert_eq!(drag_target(&layout, 1, 260.0, 10.0, true), Some(2));
        // Hızlı sürüklemede iki sekme birden atlanır.
        assert_eq!(drag_target(&layout, 1, 390.0, 10.0, true), Some(3));
        // Sola: 0.'nın ortası 50.
        assert_eq!(drag_target(&layout, 1, 60.0, 10.0, true), None);
        assert_eq!(drag_target(&layout, 1, 40.0, 10.0, true), Some(0));
        // Pencere dışına taşınca en uca gider.
        assert_eq!(drag_target(&layout, 2, -50.0, 10.0, true), Some(0));
        // Dikey düzende y'ye bakılır.
        assert_eq!(drag_target(&layout, 0, 500.0, 10.0, false), None);
    }

    #[test]
    fn other_tabs_follow_a_move() {
        // [a b c d e]: b (1) → 3 olunca [a c d b e].
        assert_eq!(follow_move(1, 1, 3), 3);
        assert_eq!(follow_move(0, 1, 3), 0);
        assert_eq!(follow_move(2, 1, 3), 1);
        assert_eq!(follow_move(3, 1, 3), 2);
        assert_eq!(follow_move(4, 1, 3), 4);
        // d (3) → 1 olunca [a d b c e].
        assert_eq!(follow_move(3, 3, 1), 1);
        assert_eq!(follow_move(1, 3, 1), 2);
        assert_eq!(follow_move(2, 3, 1), 3);
        assert_eq!(follow_move(0, 3, 1), 0);
        assert_eq!(follow_move(4, 3, 1), 4);
    }
}
