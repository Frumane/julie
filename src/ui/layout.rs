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
    /// Sekmenin sesi kapatıldı.
    pub muted: bool,
    /// Dondurulmuş ya da bellekten atılmış.
    pub sleeping: bool,
    /// Sabitlenmiş: şeridin başında, dar, kapatma düğmesi yok.
    pub pinned: bool,
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
    /// Yer değiştirirken yeni yerine kayarak giden sekmeler: sekme ve
    /// yeni yerine göre o anki kayması (DIP).
    pub tab_shift: Vec<(usize, f32, f32)>,
    /// Adres çubuğundaki kilidin durumu.
    pub security: Security,
    /// Bu oturumda indirme yapıldıysa indirme düğmesi (yoksa düğme yok).
    pub downloads: Option<DownloadBadge>,
    /// Sayfa yakınlaştırılmışsa yüzdesi (%100'de gösterilmez).
    pub zoom: Option<u16>,
}

/// Araç çubuğundaki indirme düğmesinin durumu.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DownloadBadge {
    /// Süren indirmelerin toplam ilerlemesi (0–1); boyut bilinmiyorsa ya da
    /// süren indirme yoksa `None`.
    pub progress: Option<f32>,
    /// Süren (ya da duraklatılmış) indirme var.
    pub active: bool,
    /// Kullanıcının henüz görmediği biten indirme var (vurgu rengi).
    pub attention: bool,
}

/// İndirme panelindeki bir satır.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadRow {
    pub id: u64,
    pub name: String,
    /// Windows'un dosya simgesi (`favicons` içindeki anahtar).
    pub icon_key: Option<String>,
    /// Simge yoksa türüne göre çizilen ikon.
    pub fallback: Icon,
    /// Adın altındaki satır (boyut, hız, kalan süre ya da hata).
    pub status: String,
    /// Süren indirmenin ilerlemesi (0–1); bitmişse `None`.
    pub progress: Option<f32>,
    pub state: RowState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Active,
    Paused,
    Done,
    Failed { retry: bool },
    Cancelled,
}

/// İndirme panelinde tıklanan yer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelClick {
    /// Satırın kendisi (bitmişse dosyayı açar).
    Row(u64),
    /// Satırdaki ana düğme: duraklat, devam et, yeniden dene ya da
    /// klasörde göster.
    Primary(u64),
    Cancel(u64),
    /// "Tüm indirmeler" (indirmeler sayfası).
    All,
}

impl PanelClick {
    pub fn code(self) -> usize {
        match self {
            PanelClick::Row(id) => id as usize * 3,
            PanelClick::Primary(id) => id as usize * 3 + 1,
            PanelClick::Cancel(id) => id as usize * 3 + 2,
            PanelClick::All => usize::MAX,
        }
    }

    pub fn from_code(code: usize) -> PanelClick {
        if code == usize::MAX {
            return PanelClick::All;
        }
        let id = (code / 3) as u64;
        match code % 3 {
            0 => PanelClick::Row(id),
            1 => PanelClick::Primary(id),
            _ => PanelClick::Cancel(id),
        }
    }
}

/// Geçerli sayfanın bağlantısı (adres çubuğundaki simge).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Security {
    /// Tarayıcının kendi sayfası ya da boş sekme: simge yok.
    #[default]
    None,
    /// `https://`, geçerli sertifika.
    Secure,
    /// `http://`: bağlantı şifresiz.
    Insecure,
    /// Geçersiz sertifika (kullanıcı yine de devam etti) ya da sertifika
    /// hatası sayfası.
    Danger,
}

/// Bir sitenin bir izni.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermState {
    /// Site isteyince sorulur (varsayılan).
    Ask,
    Allow,
    Block,
}

/// Hata ve uyarı rengi (temanın açık/koyu hâline göre).
pub fn danger_color(theme: &Theme) -> Color {
    if theme.dark {
        Color::hex(0xff99a4)
    } else {
        Color::hex(0xc42b1c)
    }
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
    /// Adres çubuğundaki kilit: site bilgisi paneli.
    Site,
    History,
    Settings,
    Downloads,
    /// Adres çubuğundaki yakınlaştırma yüzdesi: %100'e döner.
    Zoom,
    Bookmark(usize),
}

/// Fare durumu: üzerine gelinen ve basılı tutulan öğe.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pointer {
    pub hover: Option<Target>,
    pub pressed: Option<Target>,
    /// Sürüklenen sekme ve imleçle birlikte kayması (DIP).
    pub drag: Option<(usize, f32, f32)>,
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
        dragged: None,
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

/// Çizim parçalarını kaydırır (kırpma sınırları hariç).
fn shift_prims(prims: &mut [Prim], dx: f32, dy: f32) {
    for prim in prims {
        let rect = match prim {
            Prim::Fill { rect, .. }
            | Prim::Border { rect, .. }
            | Prim::Favicon { rect, .. }
            | Prim::Text { rect, .. }
            | Prim::Icon { rect, .. } => rect,
            Prim::PushClip(_) | Prim::PopClip => continue,
        };
        rect.x += dx;
        rect.y += dy;
    }
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

/// Sabitlenmiş sekmeler (hep başta, `pinned` tane) yalnızca kendi
/// aralarında, diğerleri de yalnızca kendi aralarında yer değiştirir.
pub fn clamp_move(pinned: usize, from: usize, to: usize) -> usize {
    if from < pinned {
        to.min(pinned - 1)
    } else {
        to.max(pinned)
    }
}

/// Sekme yer değiştirme animasyonunun süresi (ms).
pub const SLIDE_MS: f32 = 160.0;

/// Kayan sekmenin `elapsed` ms sonraki kayması: başta hızlı, sona doğru
/// yavaşlayarak sıfıra iner.
pub fn slide_offset(dx: f32, dy: f32, elapsed: f32) -> (f32, f32) {
    let t = (elapsed / SLIDE_MS).clamp(0.0, 1.0);
    let k = (1.0 - t).powi(3);
    (dx * k, dy * k)
}

/// `from` sekmesi `to` sırasına taşınınca aradaki sekmeler birer sıra
/// kayar. Her biri için yeni sırası ve eski yerinin yenisine göre farkı:
/// animasyon bu farktan sıfıra iner.
pub fn displaced(layout: &Layout, from: usize, to: usize) -> Vec<(usize, f32, f32)> {
    let rect = |i: usize| {
        layout
            .hits
            .iter()
            .find(|(_, t)| *t == Target::Tab(i))
            .map(|(r, _)| *r)
    };
    let pairs: Vec<(usize, usize)> = if to > from {
        (from + 1..=to).map(|j| (j, j - 1)).collect()
    } else {
        (to..from).map(|j| (j, j + 1)).collect()
    };
    pairs
        .into_iter()
        .filter_map(|(old, new)| {
            let (a, b) = (rect(old)?, rect(new)?);
            Some((new, a.x - b.x, a.y - b.y))
        })
        .collect()
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

/// Sayfada bul kutusundaki düğmeler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindTarget {
    Prev,
    Next,
    /// Büyük/küçük harfe duyarlı arama aç/kapat.
    Case,
    Close,
}

impl FindTarget {
    pub const ALL: [FindTarget; 4] = [Self::Case, Self::Prev, Self::Next, Self::Close];

    /// Düğmenin üstüne gelince çıkan ipucu.
    pub fn tip(self) -> &'static str {
        match self {
            Self::Prev => "Önceki eşleşme (Shift+Enter)",
            Self::Next => "Sonraki eşleşme (Enter)",
            Self::Case => "Büyük/küçük harfe duyarlı",
            Self::Close => "Kapat (Esc)",
        }
    }
}

/// Sayfada bul kutusunun durumu.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FindState {
    /// Seçili eşleşmenin sırası (1'den başlar) ve toplam eşleşme.
    pub active: i32,
    pub total: i32,
    /// Kutuda aranan bir şey yazılı mı (sayaç ancak o zaman görünür).
    pub has_text: bool,
    pub case_sensitive: bool,
    pub hover: Option<FindTarget>,
    pub pressed: Option<FindTarget>,
}

/// Sayfada bul kutusunun çizimi.
#[derive(Debug, Default)]
pub struct FindBar {
    pub prims: Vec<Prim>,
    pub hits: Vec<(Rect, FindTarget)>,
    /// Yazı kutusunun (Win32 EDIT) yeri.
    pub edit: Rect,
}

impl FindBar {
    pub fn hit(&self, x: f32, y: f32) -> Option<FindTarget> {
        self.hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, t)| *t)
    }
}

/// Bulma kutusunun boyutu (DIP). Her düzende aynı; yalnızca renkleri
/// temadan, yeri ayarlardan gelir.
pub const FIND_W: f32 = 400.0;
pub const FIND_H: f32 = 44.0;
/// Kutunun sayfa kenarlarına uzaklığı.
pub const FIND_MARGIN: f32 = 12.0;
/// Kutunun köşe yuvarlaklığı.
pub const FIND_RADIUS: f32 = 10.0;

/// Sayfada bul kutusunu çizer.
pub fn find_bar(theme: &Theme, state: &FindState) -> FindBar {
    let p = &theme.palette;
    let m = &theme.metrics;
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer: Pointer::default(),
        dragged: None,
    };
    let all = Rect::new(0.0, 0.0, FIND_W, FIND_H);
    b.fill(all, 0.0, p.surface);
    b.border(
        Rect::new(0.5, 0.5, FIND_W - 1.0, FIND_H - 1.0),
        FIND_RADIUS,
        p.border,
    );
    let mut hits = Vec::new();
    let size = 28.0;
    let y = (FIND_H - size) / 2.0;
    let mut right = FIND_W - 8.0;
    let mut button = |b: &mut Builder, target: FindTarget, w: f32, right: &mut f32| {
        let r = Rect::new(*right - w, y, w, size);
        if state.pressed == Some(target) {
            b.fill(r, m.radius, p.pressed);
        } else if state.hover == Some(target) {
            b.fill(r, m.radius, p.hover);
        }
        hits.push((r, target));
        *right = r.x - 2.0;
        r
    };
    let close = button(&mut b, FindTarget::Close, size, &mut right);
    b.icon(Icon::Close, close.centered_square(12.0), p.text_muted);
    let next = button(&mut b, FindTarget::Next, size, &mut right);
    b.icon(Icon::ChevronDown, next.centered_square(16.0), p.text_muted);
    let prev = button(&mut b, FindTarget::Prev, size, &mut right);
    b.icon(Icon::ChevronUp, prev.centered_square(16.0), p.text_muted);
    right -= 2.0;
    let case = Rect::new(right - 32.0, y, 32.0, size);
    if state.case_sensitive {
        b.fill(case, m.radius, p.accent.alpha(0.18));
    } else if state.pressed == Some(FindTarget::Case) {
        b.fill(case, m.radius, p.pressed);
    } else if state.hover == Some(FindTarget::Case) {
        b.fill(case, m.radius, p.hover);
    }
    let case_color = if state.case_sensitive {
        p.accent
    } else {
        p.text_muted
    };
    b.out.prims.push(Prim::Text {
        rect: case,
        text: "Aa".into(),
        size: m.font,
        bold: true,
        color: case_color,
        align: Align::Center,
    });
    hits.push((case, FindTarget::Case));
    right = case.x - 4.0;
    // Ayraç çizgisi.
    b.fill(Rect::new(right, y + 6.0, 1.0, size - 12.0), 0.0, p.divider);
    right -= 6.0;
    let count_w = 74.0;
    let count = Rect::new(right - count_w, 0.0, count_w, FIND_H);
    if state.has_text {
        let (text, color) = if state.total > 0 {
            (
                format!("{}/{}", state.active.max(1), state.total),
                p.text_muted,
            )
        } else {
            ("Sonuç yok".to_string(), p.text_faint)
        };
        b.out.prims.push(Prim::Text {
            rect: count,
            text,
            size: m.font_small,
            bold: false,
            color,
            align: Align::Center,
        });
    }
    let text_h = (m.font * 1.45).round();
    let edit = Rect::new(
        14.0,
        (FIND_H - text_h) / 2.0,
        (count.x - 14.0 - 4.0).max(0.0),
        text_h,
    );
    FindBar {
        prims: b.out.prims,
        hits,
        edit,
    }
}

/// Bulma kutusunun sayfa alanı (`content`) içindeki yeri.
pub fn find_rect(content: Rect, corner: super::theme::FindCorner) -> Rect {
    use super::theme::FindCorner as C;
    let w = FIND_W.min(content.w - FIND_MARGIN * 2.0).max(0.0);
    let left = content.x + FIND_MARGIN;
    let right = content.right() - FIND_MARGIN - w;
    let top = content.y + FIND_MARGIN;
    let bottom = content.bottom() - FIND_MARGIN - FIND_H;
    let (x, y) = match corner {
        C::TopRight => (right, top),
        C::TopLeft => (left, top),
        C::BottomRight => (right, bottom),
        C::BottomLeft => (left, bottom),
    };
    Rect::new(x, y, w, FIND_H)
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

/// Site bilgisi panelinde gösterilenler.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteInfo {
    pub host: String,
    pub security: Security,
    /// İzin adı ve durumu.
    pub permissions: Vec<(String, PermState)>,
    /// "Sertifikayı görüntüle" satırı gösterilsin (şifreli siteler).
    pub certificate: bool,
}

/// Site panelinde "Sertifikayı görüntüle" satırının tıklama kimliği
/// (izin satırları 0'dan başlar).
pub const SITE_CERT_ROW: usize = usize::MAX;

/// Site bilgisi panelinin çizimi.
#[derive(Debug, Default)]
pub struct SitePanel {
    pub prims: Vec<Prim>,
    /// İzin satırlarının tıklanabilir alanları (satır sırası).
    pub hits: Vec<(Rect, usize)>,
    pub height: f32,
}

impl SitePanel {
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, i)| *i)
    }
}

/// Sağ tık menüsündeki bir satır.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuEntry {
    Item {
        label: String,
        /// Sağda soluk yazılan kısayol ("Ctrl+W").
        shortcut: Option<String>,
        enabled: bool,
    },
    Separator,
}

impl MenuEntry {
    pub fn item(label: &str, shortcut: Option<&str>, enabled: bool) -> MenuEntry {
        MenuEntry::Item {
            label: label.to_string(),
            shortcut: shortcut.map(str::to_string),
            enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self, MenuEntry::Item { enabled: true, .. })
    }
}

pub const MENU_W: f32 = 310.0;
const MENU_ROW: f32 = 30.0;

/// Sağ tık menüsü (Julie'nin renkleriyle). Tıklama kimliği satırın sırası.
pub fn menu(theme: &Theme, entries: &[MenuEntry], hover: Option<usize>) -> SitePanel {
    let p = &theme.palette;
    let m = &theme.metrics;
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer: Pointer::default(),
        dragged: None,
    };
    let pad = 5.0;
    let mut hits = Vec::new();
    let mut y = pad;
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            MenuEntry::Separator => {
                b.fill(
                    Rect::new(pad + 6.0, y + 4.0, MENU_W - pad * 2.0 - 12.0, 1.0),
                    0.0,
                    p.divider,
                );
                y += 9.0;
            }
            MenuEntry::Item {
                label,
                shortcut,
                enabled,
            } => {
                let r = Rect::new(pad, y, MENU_W - pad * 2.0, MENU_ROW);
                if *enabled && hover == Some(i) {
                    b.fill(r, m.radius - 2.0, p.hover);
                }
                let color = if *enabled { p.text } else { p.text_faint };
                // Etiket kısayola yer bırakır.
                let keys_w = shortcut
                    .as_ref()
                    .map_or(0.0, |k| 12.0 + 6.0 * k.chars().count() as f32);
                b.text(
                    Rect::new(r.x + 12.0, r.y, r.w - 24.0 - keys_w, r.h),
                    label,
                    m.font,
                    false,
                    color,
                );
                if let Some(keys) = shortcut {
                    b.out.prims.push(Prim::Text {
                        rect: Rect::new(r.x + r.w * 0.5, r.y, r.w * 0.5 - 12.0, r.h),
                        text: keys.clone(),
                        size: m.font_small,
                        bold: false,
                        color: p.text_faint,
                        align: Align::Right,
                    });
                }
                if *enabled {
                    hits.push((r, i));
                }
                y += MENU_ROW;
            }
        }
    }
    y += pad;
    let mut prims = vec![
        Prim::Fill {
            rect: Rect::new(0.0, 0.0, MENU_W, y),
            radius: PANEL_RADIUS - 2.0,
            color: p.surface,
        },
        Prim::Border {
            rect: Rect::new(0.0, 0.0, MENU_W, y),
            radius: PANEL_RADIUS - 2.0,
            color: p.border,
        },
    ];
    prims.append(&mut b.out.prims);
    SitePanel {
        prims,
        hits,
        height: y,
    }
}

pub const DOWNLOADS_W: f32 = 360.0;
const DL_ROW: f32 = 64.0;

/// İndirme düğmesine basınca açılan panel: son indirmeler.
pub fn downloads_panel(theme: &Theme, rows: &[DownloadRow], hover: Option<usize>) -> SitePanel {
    let p = &theme.palette;
    let m = &theme.metrics;
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer: Pointer::default(),
        dragged: None,
    };
    let w = DOWNLOADS_W;
    let pad = 8.0;
    let mut hits = Vec::new();
    let mut y = 12.0;
    b.text(
        Rect::new(16.0, y, w - 32.0, 22.0),
        "İndirmeler",
        m.font + 1.0,
        true,
        p.text,
    );
    y += 30.0;
    if rows.is_empty() {
        b.text(
            Rect::new(16.0, y, w - 32.0, 20.0),
            "Bu oturumda indirme yapılmadı.",
            m.font_small,
            false,
            p.text_muted,
        );
        y += 28.0;
    }
    let danger = danger_color(theme);
    for row in rows {
        let r = Rect::new(pad, y, w - pad * 2.0, DL_ROW);
        let off = matches!(row.state, RowState::Failed { .. } | RowState::Cancelled);
        let row_code = PanelClick::Row(row.id).code();
        if hover.is_some_and(|h| h != usize::MAX && h / 3 == row.id as usize) {
            b.fill(r, m.radius, p.hover);
        }

        // Simge: Windows'un simgesi, yoksa türüne göre ikon.
        let tile = Rect::new(r.x + 10.0, r.y + (DL_ROW - 32.0) / 2.0, 32.0, 32.0);
        match &row.icon_key {
            Some(key) => b.out.prims.push(Prim::Favicon {
                rect: tile,
                key: Some(key.clone()),
                color: p.text_muted,
            }),
            None => {
                let color = if off { p.text_faint } else { p.accent };
                b.fill(tile, 8.0, color.alpha(0.12));
                b.icon(row.fallback, tile.centered_square(20.0), color);
            }
        }

        // Sağdaki düğmeler.
        let buttons: Vec<(Icon, PanelClick)> = match row.state {
            RowState::Active => vec![
                (Icon::Pause, PanelClick::Primary(row.id)),
                (Icon::Close, PanelClick::Cancel(row.id)),
            ],
            RowState::Paused => vec![
                (Icon::Play, PanelClick::Primary(row.id)),
                (Icon::Close, PanelClick::Cancel(row.id)),
            ],
            RowState::Done => vec![(Icon::Folder, PanelClick::Primary(row.id))],
            RowState::Failed { retry: true } => vec![(Icon::Reload, PanelClick::Primary(row.id))],
            _ => vec![],
        };
        let mut right = r.right() - 6.0;
        for (icon, click) in buttons.iter().rev() {
            let button = Rect::new(right - 30.0, r.y + (DL_ROW - 30.0) / 2.0, 30.0, 30.0);
            if hover == Some(click.code()) {
                b.fill(button, 6.0, p.pressed);
            }
            b.icon(*icon, button.centered_square(16.0), p.text_muted);
            hits.push((button, click.code()));
            right -= 32.0;
        }

        let text_x = tile.right() + 12.0;
        let text_w = right - text_x - 4.0;
        let name_color = if off { p.text_muted } else { p.text };
        let has_bar = row.progress.is_some();
        let top = if has_bar { r.y + 9.0 } else { r.y + 13.0 };
        b.text(
            Rect::new(text_x, top, text_w, 20.0),
            &row.name,
            m.font,
            false,
            name_color,
        );
        let status_color = if matches!(row.state, RowState::Failed { .. }) {
            danger
        } else {
            p.text_muted
        };
        b.text(
            Rect::new(text_x, top + 20.0, text_w, 18.0),
            &row.status,
            m.font_small,
            false,
            status_color,
        );
        if let Some(done) = row.progress {
            let bar = Rect::new(text_x, top + 42.0, text_w, 3.0);
            b.fill(bar, 1.5, p.divider);
            if done > 0.0 {
                let color = if row.state == RowState::Paused {
                    p.text_faint
                } else {
                    p.accent
                };
                b.fill(
                    Rect::new(bar.x, bar.y, bar.w * done.clamp(0.0, 1.0), bar.h),
                    1.5,
                    color,
                );
            }
        }
        hits.push((r, row_code));
        y += DL_ROW;
    }

    y += 6.0;
    b.fill(Rect::new(0.0, y, w, 1.0), 0.0, p.divider);
    y += 6.0;
    let all = Rect::new(pad, y, w - pad * 2.0, 36.0);
    if hover == Some(PanelClick::All.code()) {
        b.fill(all, m.radius, p.hover);
    }
    b.text(
        Rect::new(all.x + 10.0, all.y, all.w - 20.0, all.h),
        "Tüm indirmeler",
        m.font,
        false,
        p.text,
    );
    b.out.prims.push(Prim::Text {
        rect: Rect::new(all.x, all.y, all.w - 10.0, all.h),
        text: "Ctrl+J".to_string(),
        size: m.font_small,
        bold: false,
        color: p.text_faint,
        align: Align::Right,
    });
    hits.push((all, PanelClick::All.code()));
    y += all.h + 6.0;

    let mut prims = vec![
        Prim::Fill {
            rect: Rect::new(0.0, 0.0, w, y),
            radius: PANEL_RADIUS,
            color: p.surface,
        },
        Prim::Border {
            rect: Rect::new(0.0, 0.0, w, y),
            radius: PANEL_RADIUS,
            color: p.border,
        },
    ];
    prims.append(&mut b.out.prims);
    SitePanel {
        prims,
        hits,
        height: y,
    }
}

pub const SITE_W: f32 = 320.0;
const SITE_PAD: f32 = 16.0;
const SITE_ROW: f32 = 34.0;

/// Kilide tıklanınca açılan panel: bağlantı durumu ve izinler.
pub fn site_panel(theme: &Theme, info: &SiteInfo, hover: Option<usize>) -> SitePanel {
    let p = &theme.palette;
    let m = &theme.metrics;
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer: Pointer::default(),
        dragged: None,
    };
    let inner = SITE_W - SITE_PAD * 2.0;
    let mut y = SITE_PAD;
    b.text(
        Rect::new(SITE_PAD, y, inner, 22.0),
        &info.host,
        m.font + 1.0,
        true,
        p.text,
    );
    y += 32.0;

    let danger = danger_color(theme);
    let (icon, color, title, lines): (Icon, Color, &str, [&str; 2]) = match info.security {
        Security::Secure => (
            Icon::Lock,
            p.text_muted,
            "Bağlantı güvenli",
            [
                "Gönderdiğin bilgiler (şifre, kart) bu siteye",
                "şifreli olarak gider.",
            ],
        ),
        Security::Insecure => (
            Icon::Warning,
            p.text_muted,
            "Bağlantı güvenli değil",
            [
                "Bu sitede şifre ya da kart bilgisi girme;",
                "aynı ağdakiler görebilir.",
            ],
        ),
        Security::Danger => (
            Icon::Warning,
            danger,
            "Sertifika geçersiz",
            [
                "Site taklit olabilir. Şifre, kart ya da",
                "kişisel bilgi girme.",
            ],
        ),
        Security::None => (
            Icon::Globe,
            p.text_muted,
            "Julie sayfası",
            ["Bu sayfa tarayıcının kendi sayfası.", ""],
        ),
    };
    b.icon(icon, Rect::new(SITE_PAD, y + 1.0, 18.0, 18.0), color);
    let text_x = SITE_PAD + 28.0;
    let text_w = SITE_W - text_x - SITE_PAD;
    let title_color = if info.security == Security::Danger {
        danger
    } else {
        p.text
    };
    b.text(
        Rect::new(text_x, y, text_w, 20.0),
        title,
        m.font,
        true,
        title_color,
    );
    y += 22.0;
    for line in lines.iter().filter(|l| !l.is_empty()) {
        b.text(
            Rect::new(text_x, y, text_w, 18.0),
            line,
            m.font_small,
            false,
            p.text_muted,
        );
        y += 18.0;
    }
    y += 12.0;

    let mut hits = Vec::new();
    if info.certificate {
        let r = Rect::new(SITE_PAD - 8.0, y - 4.0, inner + 16.0, SITE_ROW);
        if hover == Some(SITE_CERT_ROW) {
            b.fill(r, m.radius, p.hover);
        }
        b.text(
            Rect::new(text_x, r.y, text_w, r.h),
            "Sertifikayı görüntüle",
            m.font,
            false,
            p.text,
        );
        b.out.prims.push(Prim::Text {
            rect: Rect::new(r.x + r.w * 0.5, r.y, r.w * 0.5 - 8.0, r.h),
            text: "›".to_string(),
            size: m.font + 2.0,
            bold: false,
            color: p.text_muted,
            align: Align::Right,
        });
        hits.push((r, SITE_CERT_ROW));
        y += SITE_ROW + 4.0;
    }
    if !info.permissions.is_empty() {
        b.fill(Rect::new(SITE_PAD, y, inner, 1.0), 0.0, p.divider);
        y += 12.0;
        b.text(
            Rect::new(SITE_PAD, y, inner, 20.0),
            "İzinler",
            m.font_small,
            true,
            p.text_muted,
        );
        y += 24.0;
        for (i, (name, state)) in info.permissions.iter().enumerate() {
            let r = Rect::new(SITE_PAD - 8.0, y, inner + 16.0, SITE_ROW);
            if hover == Some(i) {
                b.fill(r, m.radius, p.hover);
            }
            b.text(
                Rect::new(r.x + 8.0, r.y, r.w * 0.55, r.h),
                name,
                m.font,
                false,
                p.text,
            );
            let (label, color) = match state {
                PermState::Ask => ("Sor", p.text_faint),
                PermState::Allow => ("İzin verildi", p.accent),
                PermState::Block => ("Engellendi", danger),
            };
            b.out.prims.push(Prim::Text {
                rect: Rect::new(r.x + r.w * 0.5, r.y, r.w * 0.5 - 8.0, r.h),
                text: label.to_string(),
                size: m.font_small,
                bold: *state != PermState::Ask,
                color,
                align: Align::Right,
            });
            hits.push((r, i));
            y += SITE_ROW;
        }
        y += 4.0;
        b.text(
            Rect::new(SITE_PAD, y, inner, 18.0),
            "Değiştirmek için tıkla. Sayfayı yenileyince geçerli olur.",
            m.font_small,
            false,
            p.text_faint,
        );
        y += 18.0;
    }
    y += SITE_PAD - 4.0;

    let mut prims = vec![
        Prim::Fill {
            rect: Rect::new(0.0, 0.0, SITE_W, y),
            radius: PANEL_RADIUS,
            color: p.surface,
        },
        Prim::Border {
            rect: Rect::new(0.0, 0.0, SITE_W, y),
            radius: PANEL_RADIUS,
            color: p.border,
        },
    ];
    prims.append(&mut b.out.prims);
    SitePanel {
        prims,
        hits,
        height: y,
    }
}

/// İzin sorusunda gösterilenler.
#[derive(Debug, Clone, PartialEq)]
pub struct AskInfo {
    pub host: String,
    /// "Kameranı kullanmak istiyor" gibi.
    pub text: String,
    pub icon: Icon,
}

/// İzin sorusunun düğmeleri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskTarget {
    Allow,
    Block,
    /// Kapat (✕): bu seferlik engelle, kararı hatırlama.
    Close,
}

#[derive(Debug, Default)]
pub struct AskPanel {
    pub prims: Vec<Prim>,
    pub hits: Vec<(Rect, AskTarget)>,
    pub height: f32,
}

impl AskPanel {
    pub fn hit(&self, x: f32, y: f32) -> Option<AskTarget> {
        self.hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, t)| *t)
    }
}

pub const ASK_W: f32 = 340.0;

/// Site paneli ve izin sorusunun köşe yarıçapı (pencere de bu şekilde kesilir).
pub const PANEL_RADIUS: f32 = 10.0;

/// Vurgu renginin üstünde okunacak yazı rengi.
fn on_accent(accent: Color) -> Color {
    let luminance = 0.2126 * accent.r + 0.7152 * accent.g + 0.0722 * accent.b;
    if luminance > 0.6 {
        Color::hex(0x17171a)
    } else {
        Color::hex(0xffffff)
    }
}

/// Site bir izin isteyince kilidin altında açılan soru.
pub fn ask_panel(theme: &Theme, info: &AskInfo, hover: Option<AskTarget>) -> AskPanel {
    let p = &theme.palette;
    let m = &theme.metrics;
    let mut b = Builder {
        out: Layout::default(),
        theme,
        pointer: Pointer::default(),
        dragged: None,
    };
    let pad = 16.0;
    let mut hits = Vec::new();

    let close = Rect::new(ASK_W - pad - 20.0, 10.0, 28.0, 28.0);
    if hover == Some(AskTarget::Close) {
        b.fill(close, 4.0, p.hover);
    }
    b.icon(Icon::Close, close.centered_square(12.0), p.text_muted);
    hits.push((close, AskTarget::Close));

    let mut y = pad;
    b.icon(info.icon, Rect::new(pad, y + 1.0, 20.0, 20.0), p.accent);
    let text_x = pad + 32.0;
    let text_w = close.x - text_x - 4.0;
    b.text(
        Rect::new(text_x, y, text_w, 22.0),
        &info.host,
        m.font + 1.0,
        true,
        p.text,
    );
    y += 24.0;
    b.text(
        Rect::new(text_x, y, ASK_W - text_x - pad, 20.0),
        &info.text,
        m.font,
        false,
        p.text_muted,
    );
    y += 22.0;
    y += 14.0;

    let h = 32.0;
    let allow = Rect::new(ASK_W - pad - 96.0, y, 96.0, h);
    let block = Rect::new(allow.x - 8.0 - 92.0, y, 92.0, h);
    let accent = if hover == Some(AskTarget::Allow) {
        p.accent.mix(p.text, 0.12)
    } else {
        p.accent
    };
    b.fill(allow, m.radius, accent);
    b.fill(
        block,
        m.radius,
        if hover == Some(AskTarget::Block) {
            p.hover
        } else {
            p.surface
        },
    );
    b.border(block, m.radius, p.border);
    for (rect, label, color, bold) in [
        (allow, "İzin ver", on_accent(p.accent), true),
        (block, "Engelle", p.text, false),
    ] {
        b.out.prims.push(Prim::Text {
            rect,
            text: label.to_string(),
            size: m.font,
            bold,
            color,
            align: Align::Center,
        });
    }
    hits.push((allow, AskTarget::Allow));
    hits.push((block, AskTarget::Block));
    y += h + pad;

    let mut prims = vec![
        Prim::Fill {
            rect: Rect::new(0.0, 0.0, ASK_W, y),
            radius: PANEL_RADIUS,
            color: p.surface,
        },
        Prim::Border {
            rect: Rect::new(0.0, 0.0, ASK_W, y),
            radius: PANEL_RADIUS,
            color: p.border,
        },
    ];
    prims.append(&mut b.out.prims);
    AskPanel {
        prims,
        hits,
        height: y,
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
        dragged: None,
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
    /// Sürüklenen sekmenin çizim parçaları (başlangıç, bitiş).
    dragged: Option<(usize, usize)>,
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

    /// Geçmiş, ayarlar ve (indirme yapıldıysa) indirmeler düğmeleri; `step`
    /// ile ilerleyerek (yatay ya da dikey) yan yana dizer. İndirmeler en
    /// başta. Kullanılan düğme sayısını döndürür.
    fn trailing_buttons(&mut self, model: &Model, start: Rect, step: (f32, f32)) -> usize {
        let mut rect = start;
        let mut count = 0;
        if let Some(badge) = model.downloads {
            self.downloads_button(rect, badge);
            rect.x += step.0;
            rect.y += step.1;
            count += 1;
        }
        for (icon, target) in [
            (Icon::Clock, Target::History),
            (Icon::Settings, Target::Settings),
        ] {
            self.icon_button(rect, icon, target, true);
            rect.x += step.0;
            rect.y += step.1;
            count += 1;
        }
        count
    }

    /// Kaç düğme (indirmeler, geçmiş, ayarlar) çizilecek.
    fn trailing_count(model: &Model) -> f32 {
        if model.downloads.is_some() { 3.0 } else { 2.0 }
    }

    /// İndirme düğmesi: süren indirmede altında ince ilerleme çubuğu,
    /// görülmemiş biten indirmede vurgu renginde ikon.
    fn downloads_button(&mut self, rect: Rect, badge: DownloadBadge) {
        let p = self.theme.palette;
        self.hover_bg(rect, 4.0, Target::Downloads);
        self.hit(rect, Target::Downloads);
        let color = if badge.attention || badge.active {
            p.accent
        } else {
            p.text_muted
        };
        self.icon(Icon::Download, rect.centered_square(16.0), color);
        if badge.active {
            let bar = Rect::new(rect.x + 7.0, rect.bottom() - 6.0, rect.w - 14.0, 2.0);
            self.fill(bar, 1.0, p.divider);
            let done = badge.progress.unwrap_or(0.0).clamp(0.0, 1.0);
            if done > 0.0 {
                self.fill(Rect::new(bar.x, bar.y, bar.w * done, bar.h), 1.0, p.accent);
            }
        }
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
        // Yakınlaştırma yüzdesi: yıldızın solunda, tıklayınca %100.
        let mut text_end = star.x;
        if let Some(zoom) = model.zoom {
            let label = format!("%{zoom}");
            let w = 14.0 + 7.0 * label.chars().count() as f32;
            let chip = Rect::new(star.x - w - 2.0, star.y, w, star.h);
            self.hover_bg(chip, 4.0, Target::Zoom);
            self.hit(chip, Target::Zoom);
            self.out.prims.push(Prim::Text {
                rect: chip,
                text: label,
                size: self.theme.metrics.font_small,
                bold: true,
                color: p.accent,
                align: Align::Center,
            });
            text_end = chip.x;
        }
        let mut text_x = rect.x + 10.0;
        if model.security != Security::None {
            let lock = Rect::new(rect.x + 4.0, rect.y + (rect.h - 26.0) / 2.0, 26.0, 26.0);
            self.hover_bg(lock, 4.0, Target::Site);
            self.hit(lock, Target::Site);
            let (icon, color) = match model.security {
                Security::Secure => (Icon::Lock, p.text_muted),
                Security::Insecure => (Icon::Warning, p.text_muted),
                _ => (Icon::Warning, danger_color(self.theme)),
            };
            self.icon(icon, lock.centered_square(15.0), color);
            text_x = lock.right() + 4.0;
        }
        let text_h = (self.theme.metrics.font * 1.7).round();
        self.out.address = Rect::new(
            text_x,
            rect.y + (rect.h - text_h) / 2.0,
            (text_end - text_x - 4.0).max(0.0),
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
    /// `start`tan beri çizilen parçalar `i`. sekmeye ait; sürüklenen sekme
    /// buysa yeri hatırlanır.
    fn tab_drawn(&mut self, i: usize, start: usize, model: &Model) {
        let end = self.out.prims.len();
        if self.pointer.drag.is_some_and(|(d, _, _)| d == i) {
            self.dragged = Some((start, end));
        } else if let Some(&(_, dx, dy)) = model.tab_shift.iter().find(|(t, _, _)| *t == i) {
            shift_prims(&mut self.out.prims[start..end], dx, dy);
        }
    }

    /// Sürüklenen sekmenin çizimini imleç kadar kaydırıp öne alır. Tıklama
    /// alanları yerinde kalır (yeni sıra onlara göre hesaplanır).
    fn lift_dragged_tab(&mut self) {
        let (Some((start, end)), Some((_, dx, dy))) = (self.dragged.take(), self.pointer.drag)
        else {
            return;
        };
        let mut moved: Vec<Prim> = self.out.prims.drain(start..end).collect();
        shift_prims(&mut moved, dx, dy);
        self.out.prims.extend(moved);
    }

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
        if tab.pinned {
            let pin = Rect::new(right - 18.0, r.y + (r.h - 12.0) / 2.0, 12.0, 12.0);
            self.icon(Icon::Pin, pin, p.text_faint);
            right = pin.x - 4.0;
        } else if tab.active || self.hovered(Target::Tab(i)) || self.hovered(Target::CloseTab(i)) {
            let close = Rect::new(right - 22.0, r.y + (r.h - 22.0) / 2.0, 22.0, 22.0);
            self.hover_bg(close, 4.0, Target::CloseTab(i));
            self.icon(Icon::Close, close.centered_square(12.0), p.text_faint);
            self.hit(close.intersect(&area), Target::CloseTab(i));
            right = close.x - 2.0;
        }
        if tab.audio || tab.muted {
            let icon = Rect::new(right - 15.0, r.y + (r.h - 13.0) / 2.0, 13.0, 13.0);
            let kind = if tab.muted {
                Icon::Muted
            } else {
                Icon::Speaker
            };
            self.icon(kind, icon, p.text_muted);
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

    // Alt satır: indirmeler, geçmiş ve ayarlar.
    let foot = Rect::new(side.x, side.bottom() - m.button, side.w, m.button);
    b.trailing_buttons(
        model,
        Rect::new(foot.x, foot.y, m.button, m.button),
        (m.button, 0.0),
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
            let start = b.out.prims.len();
            b.tab_row(r, area, i, tab);
            b.tab_drawn(i, start, model);
        }
    }
    b.lift_dragged_tab();
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
    let foot_h = Builder::trailing_count(model) * m.button;
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
        let start = b.out.prims.len();
        if tab.active {
            b.fill(r, m.radius, p.selected);
            b.fill(Rect::new(rail.x, r.y + 8.0, 3.0, r.h - 16.0), 1.5, p.accent);
        } else {
            b.hover_bg(r, m.radius, Target::Tab(i));
        }
        b.hit(r.intersect(&area), Target::Tab(i));
        b.favicon(r.centered_square(16.0), &tab.icon, tab.sleeping);
        if tab.audio && !tab.muted {
            b.fill(
                Rect::new(r.right() - 9.0, r.bottom() - 9.0, 6.0, 6.0),
                3.0,
                p.accent,
            );
        }
        if !tab.pinned && (b.hovered(Target::Tab(i)) || b.hovered(Target::CloseTab(i))) {
            let close = Rect::new(r.right() - 14.0, r.y - 1.0, 15.0, 15.0);
            b.fill(close, 7.5, p.surface);
            b.border(close, 7.5, p.border);
            b.icon(Icon::Close, close.centered_square(9.0), p.text_muted);
            b.hit(close.intersect(&area), Target::CloseTab(i));
        }
        b.tab_drawn(i, start, model);
    }
    b.lift_dragged_tab();
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
    b.trailing_buttons(
        model,
        Rect::new(fx, hy, m.button, m.button),
        (0.0, m.button),
    );
}

// ------------------------------------------------------------ C: tek satır

/// Yatay şeritte sabitlenmiş sekmenin genişliği (yalnızca site ikonu).
const PINNED_TAB_W: f32 = 40.0;

/// Yatay sekmeler; her sekmenin genişliği alana göre daralır.
/// Sabitlenmiş sekmeler başta ve dar.
fn tab_strip(b: &mut Builder, model: &Model, area: Rect, max_w: f32) {
    let m = b.theme.metrics;
    let p = b.theme.palette;
    let plus = m.button;
    let pinned = model.tabs.iter().filter(|t| t.pinned).count();
    let pin_w = PINNED_TAB_W;
    let n = (model.tabs.len() - pinned).max(1) as f32;
    let free = area.w - plus - 4.0 - pinned as f32 * (pin_w + 2.0);
    let tab_w = (free / n - 2.0).clamp(36.0, max_w);
    let mut x = area.x;
    b.out.prims.push(Prim::PushClip(area));
    for (i, tab) in model.tabs.iter().enumerate() {
        let w = if tab.pinned { pin_w } else { tab_w };
        let r = Rect::new(x, area.y, w, area.h);
        if r.x > area.right() {
            break;
        }
        let start = b.out.prims.len();
        if tab.active {
            b.fill(r, m.radius, p.surface);
            b.border(r, m.radius, p.border);
        } else {
            b.hover_bg(r, m.radius, Target::Tab(i));
        }
        b.hit(r.intersect(&area), Target::Tab(i));
        let compact = tab.pinned || tab_w < 80.0;
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
            if tab.audio || tab.muted {
                let a = Rect::new(right - 14.0, r.y + (r.h - 12.0) / 2.0, 12.0, 12.0);
                let kind = if tab.muted {
                    Icon::Muted
                } else {
                    Icon::Speaker
                };
                b.icon(kind, a, p.text_muted);
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
        } else if tab.audio && !tab.muted {
            b.fill(
                Rect::new(r.right() - 8.0, r.bottom() - 8.0, 5.0, 5.0),
                2.5,
                p.accent,
            );
        }
        b.tab_drawn(i, start, model);
        x += w + 2.0;
    }
    b.lift_dragged_tab();
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

    let buttons = Builder::trailing_count(model) * m.button;
    let tabs = Rect::new(
        div_x + 7.0,
        y,
        w - pad - buttons - 6.0 - (div_x + 7.0),
        m.button,
    );
    tab_strip(b, model, tabs, 200.0);
    let hx = w - pad - buttons;
    b.trailing_buttons(model, Rect::new(hx, y, m.button, m.button), (m.button, 0.0));
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
    let buttons = Builder::trailing_count(model) * m.button;
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
    b.trailing_buttons(
        model,
        Rect::new(right, y, m.button, m.button),
        (m.button, 0.0),
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
            tab_shift: Vec::new(),
            security: Security::Secure,
            downloads: None,
            zoom: None,
        }
    }

    #[test]
    fn lock_sits_before_address_text() {
        let theme = theme();
        for kind in LayoutKind::ALL.iter().copied() {
            let l = layout(
                kind,
                &theme,
                &sample_model(),
                1280.0,
                800.0,
                Pointer::default(),
            );
            let lock = l
                .hits
                .iter()
                .find(|(_, t)| *t == Target::Site)
                .map(|(r, _)| *r)
                .expect("kilit tıklanabilir olmalı");
            assert!(lock.right() <= l.address.x, "{kind:?}");
            let none = Model {
                security: Security::None,
                ..sample_model()
            };
            let l = layout(kind, &theme, &none, 1280.0, 800.0, Pointer::default());
            assert!(l.hits.iter().all(|(_, t)| *t != Target::Site));
        }
    }

    #[test]
    fn ask_panel_buttons() {
        let info = AskInfo {
            host: "meet.example.com".into(),
            text: "Kameranı kullanmak istiyor".into(),
            icon: Icon::Camera,
        };
        let panel = ask_panel(&theme(), &info, None);
        for target in [AskTarget::Allow, AskTarget::Block, AskTarget::Close] {
            let (r, _) = panel.hits.iter().find(|(_, t)| *t == target).unwrap();
            assert_eq!(panel.hit(r.x + 5.0, r.y + 5.0), Some(target));
            assert!(r.right() <= ASK_W && r.bottom() <= panel.height);
        }
        assert_eq!(on_accent(Color::hex(0x0078d4)), Color::hex(0xffffff));
        assert_eq!(on_accent(Color::hex(0xc8c0ff)), Color::hex(0x17171a));
    }

    pub fn sample_rows() -> Vec<DownloadRow> {
        let row = |id, name: &str, status: &str, progress, state, icon: Option<&str>| DownloadRow {
            id,
            name: name.into(),
            icon_key: icon.map(Into::into),
            fallback: crate::downloads::kind_icon(name),
            status: status.into(),
            progress,
            state,
        };
        vec![
            row(
                1,
                "linuxmint-22-cinnamon-64bit (3).iso",
                "15,2 MB / 2,7 GB · 4,6 MB/sn · 11 dk kaldı",
                Some(0.06),
                RowState::Active,
                None,
            ),
            row(
                2,
                "rapor.pdf",
                "Duraklatıldı · 293 KB / 879 KB",
                Some(0.33),
                RowState::Paused,
                Some("file:.pdf"),
            ),
            row(
                3,
                "7z2409-x64.exe",
                "1,5 MB",
                None,
                RowState::Done,
                Some("file:C:\\7z.exe"),
            ),
            row(
                4,
                "video.mp4",
                "İnternet bağlantısı kesildi",
                None,
                RowState::Failed { retry: true },
                None,
            ),
            row(
                5,
                "eski.zip",
                "İptal edildi",
                None,
                RowState::Cancelled,
                None,
            ),
        ]
    }

    #[test]
    fn menu_skips_disabled_items() {
        let theme = Theme::new(&Appearance::default(), false, Color::hex(0x0078d4));
        let entries = vec![
            MenuEntry::item("Yeni sekme", Some("Ctrl+T"), true),
            MenuEntry::Separator,
            MenuEntry::item("Sağdaki sekmeleri kapat", None, false),
            MenuEntry::item("Kapat", Some("Ctrl+W"), true),
        ];
        let panel = menu(&theme, &entries, Some(0));
        let ids: Vec<_> = panel.hits.iter().map(|(_, i)| *i).collect();
        assert_eq!(ids, vec![0, 3]);
        assert!(panel.height > 3.0 * 30.0);
        let (r, _) = panel.hits[1];
        assert_eq!(panel.hit(r.x + 5.0, r.y + 5.0), Some(3));
    }

    #[test]
    fn downloads_panel_buttons() {
        let theme = Theme::new(&Appearance::default(), false, Color::hex(0x0078d4));
        let panel = downloads_panel(&theme, &sample_rows(), None);
        let codes: Vec<_> = panel
            .hits
            .iter()
            .map(|(_, c)| PanelClick::from_code(*c))
            .collect();
        for click in [
            PanelClick::Primary(1),
            PanelClick::Cancel(1),
            PanelClick::Row(1),
            PanelClick::Primary(2),
            PanelClick::Primary(3),
            PanelClick::Primary(4),
            PanelClick::Row(5),
            PanelClick::All,
        ] {
            assert!(codes.contains(&click), "{click:?}");
        }
        assert!(!codes.contains(&PanelClick::Primary(5)));
        // Düğme satırın üstünde: önce düğme bulunur.
        let (button, _) = panel
            .hits
            .iter()
            .find(|(_, c)| *c == PanelClick::Cancel(1).code())
            .unwrap();
        let hit = panel
            .hit(button.x + 5.0, button.y + 5.0)
            .map(PanelClick::from_code);
        assert_eq!(hit, Some(PanelClick::Cancel(1)));
        assert_eq!(
            PanelClick::from_code(PanelClick::Row(7).code()),
            PanelClick::Row(7)
        );
    }

    #[test]
    fn site_panel_rows_are_clickable() {
        let info = SiteInfo {
            host: "example.com".into(),
            security: Security::Secure,
            permissions: vec![
                ("Kamera".into(), PermState::Allow),
                ("Konum".into(), PermState::Ask),
            ],
            certificate: true,
        };
        let panel = site_panel(&theme(), &info, Some(1));
        assert_eq!(panel.hits.len(), 3);
        assert_eq!(panel.hits[0].1, SITE_CERT_ROW);
        let (r, i) = panel.hits[2];
        assert_eq!(panel.hit(r.x + 10.0, r.y + 10.0), Some(i));
        assert!(panel.height > r.bottom());
        let empty = site_panel(
            &theme(),
            &SiteInfo {
                permissions: vec![],
                certificate: false,
                ..info
            },
            None,
        );
        assert!(empty.hits.is_empty() && empty.height < panel.height);
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
            // İndirme yapılmadıysa düğme yok; yapıldıysa var ve ötekiler kalır.
            assert!(!has(Target::Downloads), "{kind:?}");
            let with = Model {
                downloads: Some(DownloadBadge {
                    progress: Some(0.4),
                    active: true,
                    attention: false,
                }),
                ..model.clone()
            };
            let l2 = layout(*kind, &t, &with, 1280.0, 800.0, Pointer::default());
            for target in [Target::Downloads, Target::History, Target::Settings] {
                assert!(
                    l2.hits.iter().any(|(_, x)| *x == target),
                    "{kind:?}: {target:?} yok"
                );
            }
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
    fn pinned_tabs_are_narrow_and_cannot_be_closed() {
        let model = Model {
            tabs: (0..4)
                .map(|i| TabInfo {
                    title: format!("Sekme {i}"),
                    active: true,
                    pinned: i < 2,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let t = theme();
        for kind in LayoutKind::ALL {
            let l = layout(*kind, &t, &model, 1280.0, 800.0, Pointer::default());
            for i in 0..4 {
                let closable = l.hits.iter().any(|(_, t)| *t == Target::CloseTab(i));
                // İnce şeritte kapatma düğmesi yalnızca fare üstündeyken çıkar.
                if i < 2 || *kind != LayoutKind::Rail {
                    assert_eq!(closable, i >= 2, "{kind:?}: sekme {i}");
                }
            }
            if matches!(kind, LayoutKind::Classic | LayoutKind::SingleRow) {
                let tab = |i| tab_rect_of(&l, i);
                assert_eq!(tab(0).w, PINNED_TAB_W);
                assert!(tab(2).w > PINNED_TAB_W);
                assert!(tab(1).x < tab(2).x);
            }
        }
        // Sürüklerken gruplar karışmaz.
        assert_eq!(clamp_move(2, 0, 3), 1);
        assert_eq!(clamp_move(2, 3, 0), 2);
        assert_eq!(clamp_move(2, 2, 3), 3);
        assert_eq!(clamp_move(0, 1, 0), 0);
    }

    fn tab_rect_of(l: &Layout, i: usize) -> Rect {
        l.hits
            .iter()
            .find(|(_, t)| *t == Target::Tab(i))
            .map(|(r, _)| *r)
            .unwrap()
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

    #[test]
    fn dragged_tab_follows_the_cursor_on_top() {
        let theme = Theme::new(&Default::default(), false, Color::hex(0x0078d4));
        let model = sample_model();
        let title = tab_title(&model.tabs[1]).to_string();
        for (kind, dx, dy) in [
            (LayoutKind::Classic, 60.0, 0.0),
            (LayoutKind::Sidebar, 0.0, 40.0),
        ] {
            let still = layout(kind, &theme, &model, 1280.0, 800.0, Pointer::default());
            let pointer = Pointer {
                drag: Some((1, dx, dy)),
                ..Pointer::default()
            };
            let moved = layout(kind, &theme, &model, 1280.0, 800.0, pointer);
            let text_rect = |l: &Layout| {
                l.prims
                    .iter()
                    .rposition(|p| matches!(p, Prim::Text { text, .. } if *text == title))
                    .map(|i| match &l.prims[i] {
                        Prim::Text { rect, .. } => (i, *rect),
                        _ => unreachable!(),
                    })
                    .unwrap()
            };
            let (_, before) = text_rect(&still);
            let (at, after) = text_rect(&moved);
            assert_eq!(
                (after.x - before.x, after.y - before.y),
                (dx, dy),
                "{kind:?}"
            );
            // Öteki sekmelerin üstünde çizilir: ardından başka sekme yazısı gelmez.
            let later_tabs = moved.prims[at + 1..]
                .iter()
                .filter(|p| matches!(p, Prim::Text { text, .. } if model.tabs.iter().any(|t| tab_title(t) == text)))
                .count();
            assert_eq!(later_tabs, 0, "{kind:?}");
            assert_eq!(still.hits, moved.hits, "tıklama alanları yerinde kalır");
        }
    }

    #[test]
    fn neighbours_slide_into_their_new_places() {
        let mut layout = Layout::default();
        for i in 0..4 {
            layout.hits.push((
                Rect::new(i as f32 * 100.0, 0.0, 100.0, 30.0),
                Target::Tab(i),
            ));
        }
        // 1 → 3: 2 ve 3. sekmeler birer sola kayar; eski yerleri 100 sağda.
        assert_eq!(displaced(&layout, 1, 3), [(1, 100.0, 0.0), (2, 100.0, 0.0)]);
        // 2 → 0: 0 ve 1. sekmeler birer sağa; eski yerleri 100 solda.
        assert_eq!(
            displaced(&layout, 2, 0),
            [(1, -100.0, 0.0), (2, -100.0, 0.0)]
        );
        assert_eq!(slide_offset(100.0, 0.0, 0.0), (100.0, 0.0));
        assert_eq!(slide_offset(100.0, 0.0, SLIDE_MS), (0.0, 0.0));
        let (mid, _) = slide_offset(100.0, 0.0, SLIDE_MS / 2.0);
        assert!(mid > 0.0 && mid < 50.0, "sona doğru yavaşlar: {mid}");
    }

    #[test]
    fn shifted_tabs_are_drawn_offset() {
        let theme = Theme::new(&Default::default(), false, Color::hex(0x0078d4));
        let mut model = sample_model();
        let title = tab_title(&model.tabs[2]).to_string();
        let find = |l: &Layout| {
            l.prims
                .iter()
                .find_map(|p| match p {
                    Prim::Text { text, rect, .. } if *text == title => Some(*rect),
                    _ => None,
                })
                .unwrap()
        };
        let still = layout(
            LayoutKind::Classic,
            &theme,
            &model,
            1280.0,
            800.0,
            Pointer::default(),
        );
        model.tab_shift = vec![(2, -30.0, 0.0)];
        let moved = layout(
            LayoutKind::Classic,
            &theme,
            &model,
            1280.0,
            800.0,
            Pointer::default(),
        );
        assert_eq!(find(&moved).x - find(&still).x, -30.0);
        assert_eq!(still.hits, moved.hits);
    }

    #[test]
    fn find_bar_shows_count_and_buttons() {
        use crate::ui::theme::FindCorner;
        let theme = Theme::new(&Default::default(), false, Color::hex(0x0078d4));
        let state = FindState {
            active: 3,
            total: 12,
            has_text: true,
            ..FindState::default()
        };
        let bar = find_bar(&theme, &state);
        let texts: Vec<_> = bar
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["Aa", "3/12"]);
        let targets: Vec<_> = bar.hits.iter().map(|(_, t)| *t).collect();
        assert_eq!(
            targets,
            [
                FindTarget::Close,
                FindTarget::Next,
                FindTarget::Prev,
                FindTarget::Case
            ]
        );
        assert!(bar.edit.w > 150.0, "yazı alanı geniş kalır: {}", bar.edit.w);
        assert_eq!(
            bar.hit(FIND_W - 20.0, FIND_H / 2.0),
            Some(FindTarget::Close)
        );
        assert_eq!(bar.hit(30.0, FIND_H / 2.0), None, "yazı alanı düğme değil");

        let empty = find_bar(
            &theme,
            &FindState {
                has_text: true,
                ..FindState::default()
            },
        );
        assert!(
            empty
                .prims
                .iter()
                .any(|p| matches!(p, Prim::Text { text, .. } if text == "Sonuç yok"))
        );

        let content = Rect::new(100.0, 80.0, 1000.0, 600.0);
        assert_eq!(
            find_rect(content, FindCorner::TopRight),
            Rect::new(688.0, 92.0, FIND_W, FIND_H)
        );
        assert_eq!(
            find_rect(content, FindCorner::BottomLeft),
            Rect::new(112.0, 624.0, FIND_W, FIND_H)
        );
        // Dar pencerede kutu sayfaya sığacak kadar daralır.
        assert_eq!(
            find_rect(Rect::new(0.0, 0.0, 300.0, 400.0), FindCorner::TopLeft).w,
            276.0
        );
    }
}
