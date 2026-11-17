//! Dört düzenin yerleşimi: neyin nereye çizileceği ve nereye tıklanınca ne
//! olacağı.
//!
//! Bütün ölçüler DIP cinsindendir (100% ölçekte piksel); Direct2D ekran
//! ölçeğine göre kendisi büyütür.

use super::scene::{Align, Icon, Prim, Rect};
use super::theme::{LayoutKind, Theme};

/// Sekme şeridinde gösterilen bilgiler.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabInfo {
    pub title: String,
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
    /// Engelleyici rozetinin yazısı ("128", "Kapalı", "…").
    pub shield: String,
    /// Engelleyici bu sitede kapalı (rozet soluk çizilir).
    pub shield_off: bool,
    /// Kenar çubuğunun altındaki durum yazısı ("2 sekme uyuyor").
    pub status: String,
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
    b.out.prims.push(Prim::Fill {
        rect: Rect::new(0.0, 0.0, w, h),
        radius: 0.0,
        color: theme.palette.ground,
    });
    match kind {
        LayoutKind::Sidebar => sidebar(&mut b, model, w, h),
        LayoutKind::Rail => rail(&mut b, model, w, h),
        LayoutKind::SingleRow => single_row(&mut b, model, w, h),
        LayoutKind::Classic => classic(&mut b, model, w, h),
    }
    b.out
}

struct Builder<'a> {
    out: Layout,
    theme: &'a Theme,
    pointer: Pointer,
}

impl Builder<'_> {
    fn fill(&mut self, rect: Rect, radius: f32, color: super::theme::Color) {
        self.out.prims.push(Prim::Fill {
            rect,
            radius,
            color,
        });
    }

    fn text(&mut self, rect: Rect, text: &str, size: f32, bold: bool, color: super::theme::Color) {
        self.out.prims.push(Prim::Text {
            rect,
            text: text.to_string(),
            size,
            bold,
            color,
            align: Align::Left,
        });
    }

    fn centered_text(
        &mut self,
        rect: Rect,
        text: &str,
        size: f32,
        bold: bool,
        color: super::theme::Color,
    ) {
        self.out.prims.push(Prim::Text {
            rect,
            text: text.to_string(),
            size,
            bold,
            color,
            align: Align::Center,
        });
    }

    fn icon(&mut self, icon: Icon, rect: Rect, color: super::theme::Color) {
        self.out.prims.push(Prim::Icon { icon, rect, color });
    }

    fn hit(&mut self, rect: Rect, target: Target) {
        if rect.w > 0.0 && rect.h > 0.0 {
            self.out.hits.push((rect, target));
        }
    }

    /// Fareyle üzerine gelinen/basılan öğenin zemini.
    fn hover_bg(&mut self, rect: Rect, radius: f32, target: Target) {
        let p = &self.theme.palette;
        if self.pointer.pressed == Some(target) {
            self.fill(rect, radius, p.pressed);
        } else if self.pointer.hover == Some(target) {
            self.fill(rect, radius, p.hover);
        }
    }

    fn icon_button(&mut self, rect: Rect, icon: Icon, target: Target, enabled: bool) {
        let p = self.theme.palette;
        if enabled {
            self.hover_bg(rect, 8.0, target);
            self.hit(rect, target);
        }
        let color = if enabled {
            p.text_muted
        } else {
            p.text_faint.alpha(0.55)
        };
        self.icon(icon, rect.centered_square(18.0), color);
    }

    /// Engelleyici rozeti: kalkan ikonu ve sayı.
    fn shield_chip(&mut self, rect: Rect, model: &Model) {
        let p = self.theme.palette;
        let (bg, fg) = if model.shield_off {
            (p.hover, p.text_faint)
        } else {
            (p.accent_soft, p.accent_text)
        };
        self.fill(rect, rect.h / 2.0, bg);
        self.hover_bg(rect, rect.h / 2.0, Target::Shield);
        let icon = Rect::new(rect.x + 8.0, rect.y + (rect.h - 14.0) / 2.0, 14.0, 14.0);
        self.icon(Icon::Shield, icon, fg);
        let text = Rect::new(
            icon.right() + 5.0,
            rect.y,
            rect.right() - icon.right() - 9.0,
            rect.h,
        );
        let size = self.theme.metrics.font_small;
        self.text(text, &model.shield, size, true, fg);
        self.hit(rect, Target::Shield);
    }

    fn shield_width(model: &Model) -> f32 {
        36.0 + 7.0 * model.shield.chars().count() as f32
    }

    /// Adres kutusu: zemin, yıldız; yazı alanını `out.address`'e yazar.
    /// `extra_right`: sağda rozet gibi başka öğelere ayrılan genişlik.
    fn address_box(&mut self, rect: Rect, radius: f32, model: &Model, extra_right: f32) -> Rect {
        let p = self.theme.palette;
        self.fill(rect, radius, p.surface);
        let star = Rect::new(
            rect.right() - extra_right - 30.0,
            rect.y + (rect.h - 26.0) / 2.0,
            26.0,
            26.0,
        );
        if model.star_enabled {
            self.hover_bg(star, 6.0, Target::Star);
            self.hit(star, Target::Star);
        }
        let (icon, color) = if model.bookmarked {
            (Icon::StarFilled, p.accent)
        } else if model.star_enabled {
            (Icon::Star, p.text_faint)
        } else {
            (Icon::Star, p.text_faint.alpha(0.4))
        };
        self.icon(icon, star.centered_square(16.0), color);
        let text_h = (self.theme.metrics.font * 1.7).round();
        self.out.address = Rect::new(
            rect.x + 12.0,
            rect.y + (rect.h - text_h) / 2.0,
            (star.x - rect.x - 16.0).max(0.0),
            text_h,
        );
        star
    }

    /// Sayfa çerçevesi.
    fn content(&mut self, rect: Rect, radius: f32) {
        let p = self.theme.palette;
        self.out.prims.push(Prim::Shadow {
            rect,
            radius,
            color: p.shadow,
        });
        self.fill(rect, radius, p.surface);
        self.out.content = rect;
        self.out.content_radius = radius;
    }

    /// Sekme başlığının harf rozeti (site ikonları gelene kadar).
    fn badge(&mut self, rect: Rect, title: &str, tab: Option<&TabInfo>) {
        let p = self.theme.palette;
        let color = match tab {
            Some(t) if t.sleeping => p.text_faint,
            Some(t) if t.active => p.accent,
            _ => p.text_muted,
        };
        self.fill(rect, (rect.w * 0.28).round(), p.well);
        let size = (rect.h * 0.58).clamp(10.0, 16.0);
        self.centered_text(rect, &letter(title), size, true, color);
    }
}

/// Başlığın ilk harfi ya da rakamı (büyük harfle).
pub fn letter(title: &str) -> String {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "•".to_string())
}

fn tab_title(tab: &TabInfo) -> &str {
    let t = tab.title.trim();
    if t.is_empty() { "Yeni sekme" } else { t }
}

/// Kenar çubuğu ve tek satır düzenlerindeki gezinme butonları.
fn nav_buttons(b: &mut Builder, x: f32, y: f32, model: &Model) -> f32 {
    let s = b.theme.metrics.button;
    b.icon_button(
        Rect::new(x, y, s, s),
        Icon::Back,
        Target::Back,
        model.can_back,
    );
    b.icon_button(
        Rect::new(x + s + 2.0, y, s, s),
        Icon::Forward,
        Target::Forward,
        model.can_forward,
    );
    b.icon_button(
        Rect::new(x + 2.0 * (s + 2.0), y, s, s),
        Icon::Reload,
        Target::Reload,
        true,
    );
    x + 3.0 * s + 4.0
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
        12.0,
    );

    // Gezinme ve engelleyici.
    let mut y = side.y;
    nav_buttons(b, side.x, y, model);
    let chip_w = Builder::shield_width(model);
    b.shield_chip(
        Rect::new(
            side.right() - chip_w - 2.0,
            y + (m.button - 26.0) / 2.0,
            chip_w,
            26.0,
        ),
        model,
    );
    y += m.button + 10.0;

    // Adres.
    b.address_box(
        Rect::new(side.x, y, side.w, m.address_h),
        m.radius,
        model,
        4.0,
    );
    y += m.address_h + 12.0;

    // Yer imleri: 4 sütunlu kutucuklar, en fazla 2 satır.
    let marks = model.bookmarks.len().min(8);
    if marks > 0 {
        let gap = 6.0;
        let cell_w = (side.w - 3.0 * gap) / 4.0;
        let cell_h = (44.0 * m.row / 36.0).round();
        for (i, mark) in model.bookmarks.iter().take(marks).enumerate() {
            let r = Rect::new(
                side.x + (i % 4) as f32 * (cell_w + gap),
                y + (i / 4) as f32 * (cell_h + gap),
                cell_w,
                cell_h,
            );
            b.fill(r, m.radius, p.well);
            b.hover_bg(r, m.radius, Target::Bookmark(i));
            let name = if mark.title.trim().is_empty() {
                &mark.url
            } else {
                &mark.title
            };
            b.centered_text(r, &letter(host_or(name)), 15.0, true, p.text_muted);
            b.hit(r, Target::Bookmark(i));
        }
        y += marks.div_ceil(4) as f32 * (cell_h + gap) + 6.0;
    }
    b.fill(
        Rect::new(side.x + 6.0, y, side.w - 12.0, 1.0),
        0.0,
        p.divider,
    );
    y += 11.0;

    // Alt satır: geçmiş, ayarlar, durum.
    let foot = Rect::new(side.x, side.bottom() - m.button, side.w, m.button);
    b.icon_button(
        Rect::new(foot.x, foot.y, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(foot.x + m.button + 2.0, foot.y, m.button, m.button),
        Icon::Sliders,
        Target::Settings,
        true,
    );
    let status_x = foot.x + 2.0 * m.button + 10.0;
    b.text(
        Rect::new(status_x, foot.y, foot.right() - status_x - 4.0, foot.h),
        &model.status,
        m.font_small,
        false,
        p.text_faint,
    );

    // Sekmeler: kaydırılabilir dikey liste.
    let area = Rect::new(side.x, y, side.w, (foot.y - 8.0 - y).max(0.0));
    let step = m.row + 2.0;
    let total = (model.tabs.len() + 1) as f32 * step;
    let max = (total - area.h).max(0.0);
    let offset = model.tab_scroll.clamp(0.0, max);
    b.out.scroll_area = Some(area);
    b.out.scroll_max = max;
    b.out.prims.push(Prim::PushClip(area));
    for (i, tab) in model.tabs.iter().enumerate() {
        let r = Rect::new(area.x, area.y + i as f32 * step - offset, area.w, m.row);
        if r.bottom() < area.y || r.y > area.bottom() {
            continue;
        }
        let visible = r.intersect(&area);
        if tab.active {
            b.fill(r, 9.0, p.surface);
        } else {
            b.hover_bg(r, 9.0, Target::Tab(i));
        }
        b.hit(visible, Target::Tab(i));
        b.badge(
            Rect::new(r.x + 10.0, r.y + (r.h - 18.0) / 2.0, 18.0, 18.0),
            tab_title(tab),
            Some(tab),
        );
        let show_close = tab.active
            || b.pointer.hover == Some(Target::Tab(i))
            || b.pointer.hover == Some(Target::CloseTab(i));
        let mut right = r.right() - 6.0;
        if show_close {
            let close = Rect::new(right - 24.0, r.y + (r.h - 24.0) / 2.0, 24.0, 24.0);
            b.hover_bg(close, 6.0, Target::CloseTab(i));
            b.icon(Icon::Close, close.centered_square(14.0), p.text_faint);
            b.hit(close.intersect(&area), Target::CloseTab(i));
            right = close.x - 2.0;
        }
        if tab.audio {
            let icon = Rect::new(right - 16.0, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0);
            b.icon(Icon::Speaker, icon, p.accent);
            right = icon.x - 4.0;
        }
        let color = if tab.sleeping {
            p.text_faint
        } else if tab.active {
            p.text
        } else {
            p.text_muted
        };
        b.text(
            Rect::new(r.x + 38.0, r.y, right - r.x - 40.0, r.h),
            tab_title(tab),
            m.font,
            tab.active,
            color,
        );
    }
    let r = Rect::new(
        area.x,
        area.y + model.tabs.len() as f32 * step - offset,
        area.w,
        m.row,
    );
    if r.bottom() >= area.y && r.y <= area.bottom() {
        b.hover_bg(r, 9.0, Target::NewTab);
        b.icon(
            Icon::Plus,
            Rect::new(r.x + 10.0, r.y + (r.h - 18.0) / 2.0, 18.0, 18.0),
            p.text_faint,
        );
        b.text(
            Rect::new(r.x + 38.0, r.y, r.w - 40.0, r.h),
            "Yeni sekme",
            m.font,
            false,
            p.text_faint,
        );
        b.hit(r.intersect(&area), Target::NewTab);
    }
    b.out.prims.push(Prim::PopClip);
}

/// Yer imi adı yoksa adresin alan adı.
fn host_or(text: &str) -> &str {
    let rest = text.split_once("://").map_or(text, |(_, r)| r);
    rest.strip_prefix("www.").unwrap_or(rest)
}

// ------------------------------------------------------------ B: ince şerit

fn rail(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let pad = (m.pad - 2.0).max(4.0);
    let rail = Rect::new(pad, pad, m.rail_w, h - 2.0 * pad);
    let x0 = rail.right() + pad;
    let bar = Rect::new(x0, pad, w - x0 - pad, (m.button + 8.0).round());
    b.content(
        Rect::new(
            x0,
            bar.bottom() + pad,
            w - x0 - pad,
            h - bar.bottom() - 2.0 * pad,
        ),
        10.0,
    );

    // Üst çubuk: gezinme, ortada adres, sağda rozet.
    let after_nav = nav_buttons(b, bar.x, bar.y + (bar.h - m.button) / 2.0, model);
    let chip_w = Builder::shield_width(model);
    b.shield_chip(
        Rect::new(
            bar.right() - chip_w,
            bar.y + (bar.h - 26.0) / 2.0,
            chip_w,
            26.0,
        ),
        model,
    );
    let free = (bar.right() - chip_w - 12.0) - (after_nav + 12.0);
    let addr_w = free.clamp(120.0, 560.0);
    let addr_x = (after_nav + 12.0 + (free - addr_w) / 2.0).max(after_nav + 12.0);
    let addr_h = (m.button).min(bar.h);
    b.address_box(
        Rect::new(addr_x, bar.y + (bar.h - addr_h) / 2.0, addr_w, addr_h),
        9.0,
        model,
        2.0,
    );

    // Şerit: yer imleri, sekmeler, yeni sekme, altta geçmiş ve ayarlar.
    let cell = (m.rail_w - 12.0).round();
    let cx = rail.x + (rail.w - cell) / 2.0;
    let mut y = rail.y + 4.0;
    for (i, mark) in model.bookmarks.iter().take(6).enumerate() {
        let r = Rect::new(cx, y, cell, cell);
        b.fill(r, 10.0, p.well);
        b.hover_bg(r, 10.0, Target::Bookmark(i));
        let name = if mark.title.trim().is_empty() {
            &mark.url
        } else {
            &mark.title
        };
        b.centered_text(r, &letter(host_or(name)), 14.0, true, p.text_muted);
        b.hit(r, Target::Bookmark(i));
        y += cell + 6.0;
    }
    if !model.bookmarks.is_empty() {
        b.fill(
            Rect::new(rail.x + 14.0, y + 2.0, rail.w - 28.0, 1.0),
            0.0,
            p.divider,
        );
        y += 10.0;
    }
    let foot_h = 2.0 * (cell + 6.0);
    let area = Rect::new(
        rail.x,
        y,
        rail.w,
        (rail.bottom() - foot_h - 6.0 - y).max(0.0),
    );
    let step = cell + 6.0;
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
            b.fill(r, 10.0, p.surface);
            b.fill(
                Rect::new(rail.x, r.y + 10.0, 3.0, r.h - 20.0),
                1.5,
                p.accent,
            );
        } else {
            b.hover_bg(r, 10.0, Target::Tab(i));
        }
        b.hit(r.intersect(&area), Target::Tab(i));
        let color = if tab.sleeping {
            p.text_faint
        } else if tab.active {
            p.accent
        } else {
            p.text_muted
        };
        b.centered_text(r, &letter(tab_title(tab)), 15.0, true, color);
        if tab.audio {
            b.fill(
                Rect::new(r.right() - 11.0, r.bottom() - 11.0, 7.0, 7.0),
                3.5,
                p.accent,
            );
        }
        if b.pointer.hover == Some(Target::Tab(i)) || b.pointer.hover == Some(Target::CloseTab(i)) {
            let close = Rect::new(r.right() - 16.0, r.y - 2.0, 18.0, 18.0);
            b.fill(close, 9.0, p.surface);
            b.icon(Icon::Close, close.centered_square(11.0), p.text_muted);
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
    b.icon_button(
        Rect::new(cx, hy, cell, cell),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(cx, hy + cell + 6.0, cell, cell),
        Icon::Sliders,
        Target::Settings,
        true,
    );
}

// ------------------------------------------------------------ C: tek satır

/// Yatay sekme hapları; her sekmenin genişliği alana göre daralır.
fn tab_pills(b: &mut Builder, model: &Model, area: Rect, max_w: f32) -> f32 {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let plus = m.button;
    let n = model.tabs.len().max(1) as f32;
    let tab_w = ((area.w - plus - 6.0) / n - 4.0).clamp(36.0, max_w);
    let mut x = area.x;
    b.out.prims.push(Prim::PushClip(area));
    for (i, tab) in model.tabs.iter().enumerate() {
        let r = Rect::new(x, area.y, tab_w, area.h);
        if r.x > area.right() {
            break;
        }
        if tab.active {
            b.fill(r, 9.0, p.surface);
        } else {
            b.hover_bg(r, 9.0, Target::Tab(i));
        }
        b.hit(r.intersect(&area), Target::Tab(i));
        let compact = tab_w < 80.0;
        let badge = if compact {
            r.centered_square(16.0)
        } else {
            Rect::new(r.x + 10.0, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0)
        };
        b.badge(badge, tab_title(tab), Some(tab));
        if !compact {
            let hovered = b.pointer.hover == Some(Target::Tab(i))
                || b.pointer.hover == Some(Target::CloseTab(i));
            let mut right = r.right() - 6.0;
            if tab.active || hovered {
                let close = Rect::new(right - 20.0, r.y + (r.h - 20.0) / 2.0, 20.0, 20.0);
                b.hover_bg(close, 5.0, Target::CloseTab(i));
                b.icon(Icon::Close, close.centered_square(12.0), p.text_faint);
                b.hit(close.intersect(&area), Target::CloseTab(i));
                right = close.x - 2.0;
            }
            if tab.audio {
                let icon = Rect::new(right - 15.0, r.y + (r.h - 13.0) / 2.0, 13.0, 13.0);
                b.icon(Icon::Speaker, icon, p.accent);
                right = icon.x - 3.0;
            }
            let color = if tab.sleeping {
                p.text_faint
            } else if tab.active {
                p.text
            } else {
                p.text_muted
            };
            b.text(
                Rect::new(badge.right() + 8.0, r.y, right - badge.right() - 10.0, r.h),
                tab_title(tab),
                m.font,
                tab.active,
                color,
            );
        } else if tab.audio {
            b.fill(
                Rect::new(r.right() - 9.0, r.bottom() - 9.0, 6.0, 6.0),
                3.0,
                p.accent,
            );
        }
        x += tab_w + 4.0;
    }
    b.out.prims.push(Prim::PopClip);
    let plus_x = x.min(area.right() - plus);
    b.icon_button(
        Rect::new(plus_x, area.y + (area.h - plus) / 2.0, plus, plus),
        Icon::Plus,
        Target::NewTab,
        true,
    );
    plus_x + plus
}

fn single_row(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let pad = (m.pad - 2.0).max(4.0);
    let row_h = (m.button + 14.0).round();
    let cy = row_h / 2.0;
    b.content(Rect::new(pad, row_h, w - 2.0 * pad, h - row_h - pad), 12.0);

    let x = nav_buttons(b, pad, cy - m.button / 2.0, model);
    let right_buttons = 2.0 * (m.button + 2.0);
    let addr_w = (w * 0.3).clamp(220.0, 400.0);
    let addr_h = (m.button + 2.0).round();
    let chip_w = Builder::shield_width(model);
    let addr = Rect::new(x + 6.0, cy - addr_h / 2.0, addr_w, addr_h);
    b.address_box(addr, 10.0, model, chip_w + 4.0);
    b.shield_chip(
        Rect::new(addr.right() - chip_w - 6.0, cy - 11.0, chip_w, 22.0),
        model,
    );
    let div_x = addr.right() + 10.0;
    b.fill(Rect::new(div_x, cy - 11.0, 1.0, 22.0), 0.0, p.divider);

    let tabs = Rect::new(
        div_x + 10.0,
        cy - (m.button / 2.0),
        w - pad - right_buttons - 8.0 - (div_x + 10.0),
        m.button,
    );
    tab_pills(b, model, tabs, 200.0);
    let hx = w - pad - right_buttons;
    b.icon_button(
        Rect::new(hx, cy - m.button / 2.0, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(hx + m.button + 2.0, cy - m.button / 2.0, m.button, m.button),
        Icon::Sliders,
        Target::Settings,
        true,
    );
}

// ------------------------------------------------------------ D: klasik

fn classic(b: &mut Builder, model: &Model, w: f32, h: f32) {
    let m = b.theme.metrics;
    let pad = (m.pad - 2.0).max(4.0);
    let strip = Rect::new(pad, pad, w - 2.0 * pad, m.button + 2.0);
    tab_pills(
        b,
        model,
        Rect::new(strip.x, strip.y, strip.w - 4.0, strip.h),
        220.0,
    );

    let bar_y = strip.bottom() + 4.0;
    let bar_h = m.button + 6.0;
    let cy = bar_y + bar_h / 2.0;
    let x = nav_buttons(b, pad, cy - m.button / 2.0, model);
    let chip_w = Builder::shield_width(model);
    let right = w - pad - 2.0 * (m.button + 2.0);
    b.shield_chip(
        Rect::new(right - chip_w - 6.0, cy - 13.0, chip_w, 26.0),
        model,
    );
    let addr = Rect::new(
        x + 6.0,
        cy - (m.button + 2.0) / 2.0,
        right - chip_w - 18.0 - x,
        m.button + 2.0,
    );
    b.address_box(addr, addr.h / 2.0, model, 2.0);
    b.icon_button(
        Rect::new(right, cy - m.button / 2.0, m.button, m.button),
        Icon::Clock,
        Target::History,
        true,
    );
    b.icon_button(
        Rect::new(
            right + m.button + 2.0,
            cy - m.button / 2.0,
            m.button,
            m.button,
        ),
        Icon::Sliders,
        Target::Settings,
        true,
    );

    let top = bar_y + bar_h + pad;
    b.content(Rect::new(pad, top, w - 2.0 * pad, h - top - pad), 8.0);
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
                    active: true,
                    ..Default::default()
                },
                TabInfo {
                    title: "Lo-fi çalışma listesi".into(),
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
                },
                BookmarkInfo {
                    title: "".into(),
                    url: "https://www.takvim.example/".into(),
                },
                BookmarkInfo {
                    title: "Müzik".into(),
                    url: "https://muzik.example/".into(),
                },
            ],
            can_back: true,
            can_forward: false,
            bookmarked: true,
            star_enabled: true,
            shield: "128".into(),
            shield_off: false,
            status: "1 sekme uyuyor".into(),
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
    fn letters() {
        assert_eq!(letter("rust dili"), "R");
        assert_eq!(letter("  — çay"), "Ç");
        assert_eq!(letter(""), "•");
        assert_eq!(host_or("https://www.takvim.example/"), "takvim.example/");
    }
}
