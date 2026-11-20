//! İndirmeler: bu oturumdaki indirmelerin listesi, indirmeler sayfası
//! (`julie://downloads`) ve araç çubuğundaki düğmenin durumu. Motorla
//! konuşan kısım (`DownloadStarting` vb.) `browser.rs`'de.

use serde_json::{Value, json};

use crate::start_page::embed_state;
use crate::ui::layout::DownloadBadge;

const TEMPLATE: &str = include_str!("downloads_page.html");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl State {
    fn id(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Paused => "paused",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }

    /// İndirme sürüyor mu (duraklatılmış da sürüyor sayılır).
    pub fn running(self) -> bool {
        matches!(self, State::Active | State::Paused)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    pub id: u64,
    pub url: String,
    /// Dosyanın diskteki yeri.
    pub path: String,
    pub received: u64,
    /// Toplam boyut; sunucu bildirmediyse `None`.
    pub total: Option<u64>,
    pub state: State,
    /// Başarısız olduysa nedeni (Türkçe).
    pub reason: String,
    /// Kaldığı yerden devam edebilir mi (başarısız ya da duraklatılmış).
    pub can_resume: bool,
    /// Başladığı an (Unix saniyesi).
    pub started: u64,
    /// Windows'taki dosya simgesi (PNG `data:` adresi).
    pub icon: Option<String>,
}

impl Download {
    /// Dosyanın adı (yolun son parçası; yoksa adresin son parçası).
    pub fn name(&self) -> String {
        let from_path = self.path.rsplit(['\\', '/']).next().unwrap_or_default();
        if !from_path.is_empty() {
            return from_path.to_string();
        }
        // Adresin yol kısmının son parçası (yalnızca web adreslerinde).
        let url = self.url.split(['?', '#']).next().unwrap_or_default();
        let web = url.starts_with("http://") || url.starts_with("https://");
        let rest = url.split_once("://").map_or("", |(_, rest)| rest);
        let last = match rest.trim_end_matches('/').split_once('/') {
            Some((_, path)) => path.rsplit('/').next().unwrap_or_default(),
            None => "",
        };
        if !web || last.is_empty() {
            "indirme".to_string()
        } else {
            crate::history_page::percent_decode(last)
        }
    }
}

/// Bu oturumdaki indirmeler (eskiden yeniye).
#[derive(Debug, Default)]
pub struct List {
    items: Vec<Download>,
    next: u64,
    /// İndirmeler sayfası en son açıldığından beri biten indirme yok.
    seen: bool,
}

impl List {
    /// Yeni indirme ekler; kimliğini döndürür.
    pub fn add(&mut self, url: &str, path: &str, total: Option<u64>, now: u64) -> u64 {
        self.next += 1;
        self.items.push(Download {
            id: self.next,
            url: url.to_string(),
            path: path.to_string(),
            received: 0,
            total: total.filter(|t| *t > 0),
            state: State::Active,
            reason: String::new(),
            can_resume: false,
            started: now,
            icon: None,
        });
        self.next
    }

    pub fn get(&self, id: u64) -> Option<&Download> {
        self.items.iter().find(|d| d.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Download> {
        self.items.iter_mut().find(|d| d.id == id)
    }

    /// Bir indirmenin durumu değişti; bittiyse düğme vurgulanır.
    pub fn set_state(&mut self, id: u64, state: State) {
        if let Some(d) = self.get_mut(id) {
            if d.state != state && !state.running() {
                self.seen = false;
            }
            if let Some(d) = self.get_mut(id) {
                d.state = state;
            }
        }
    }

    /// Süren indirmeye dokunmadan listeden çıkarır.
    pub fn remove(&mut self, id: u64) {
        self.items.retain(|d| d.id != id || d.state.running());
    }

    /// Biten (ya da başarısız, iptal edilen) indirmeleri listeden çıkarır.
    pub fn clear_finished(&mut self) {
        self.items.retain(|d| d.state.running());
    }

    /// İndirmeler sayfası açıldı: biten indirmeler görüldü.
    pub fn mark_seen(&mut self) {
        self.seen = true;
    }

    /// Araç çubuğundaki düğme; bu oturumda hiç indirme yoksa `None`.
    pub fn badge(&self) -> Option<DownloadBadge> {
        if self.items.is_empty() {
            return None;
        }
        let running: Vec<_> = self.items.iter().filter(|d| d.state.running()).collect();
        let progress = if !running.is_empty() && running.iter().all(|d| d.total.is_some()) {
            let total: u64 = running.iter().filter_map(|d| d.total).sum();
            let got: u64 = running.iter().map(|d| d.received).sum();
            (total > 0).then(|| (got as f64 / total as f64).min(1.0) as f32)
        } else {
            None
        };
        Some(DownloadBadge {
            progress,
            active: !running.is_empty(),
            attention: !self.seen && self.items.iter().any(|d| !d.state.running()),
        })
    }

    /// Sayfanın gösterdiği liste (yeniden eskiye).
    pub fn json(&self) -> Value {
        let items: Vec<_> = self
            .items
            .iter()
            .rev()
            .map(|d| {
                json!({
                    "id": d.id,
                    "name": d.name(),
                    "url": d.url,
                    "path": d.path,
                    "received": d.received,
                    "total": d.total,
                    "state": d.state.id(),
                    "reason": d.reason,
                    "canResume": d.can_resume,
                    "started": d.started,
                    "icon": d.icon,
                })
            })
            .collect();
        json!({ "items": items })
    }
}

/// İndirmeler sayfası.
pub fn render(list: &List) -> String {
    embed_state(TEMPLATE, &list.json())
}

/// İndirmeler sayfasından gelen istek.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Dosyayı aç.
    Open,
    /// Klasörde göster.
    Show,
    Pause,
    Resume,
    Cancel,
    /// Listeden kaldır (dosya silinmez).
    Remove,
}

impl Action {
    pub fn parse(name: &str) -> Option<Action> {
        Some(match name {
            "open" => Action::Open,
            "show" => Action::Show,
            "pause" => Action::Pause,
            "resume" => Action::Resume,
            "cancel" => Action::Cancel,
            "remove" => Action::Remove,
            _ => return None,
        })
    }
}

/// WebView2'nin kesinti nedeni kodunun Türkçesi
/// (`COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON`).
pub fn reason_text(code: i32) -> &'static str {
    match code {
        0 => "",
        1 => "Dosya yazılamadı",
        2 => "Klasöre yazma izni yok",
        3 => "Diskte yer yok",
        4 => "Dosya adı çok uzun",
        5 => "Dosya bu sürücü için çok büyük",
        6 => "Zararlı yazılım bulundu",
        7 => "Geçici bir dosya hatası",
        8 => "Sistem ilkesi engelledi",
        9 => "Güvenlik denetiminden geçmedi",
        10 => "Kaldığı yerden devam edilemiyor",
        11 => "Dosya bozuk indi",
        12 => "Ağ hatası",
        13 => "Ağ zaman aşımı",
        14 => "İnternet bağlantısı kesildi",
        15 => "Sunucu yanıt vermiyor",
        16 => "Sunucu isteği reddetti",
        17 => "Sunucu hatası",
        18 => "Sunucu devam etmeyi desteklemiyor",
        19 => "Sunucudan bozuk yanıt",
        20 => "Yetki gerekiyor",
        21 => "Sunucunun sertifikasında sorun var",
        22 => "Erişim yasak",
        23 => "Beklenmeyen yönlendirme",
        24 => "Dosya eksik indi",
        25 => "Başka siteye yönlendirildi",
        26 => "İptal edildi",
        27 => "Tarayıcı kapandı",
        28 => "Duraklatıldı",
        29 => "İndirme işlemi çöktü",
        _ => "Bilinmeyen hata",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let mut list = List::default();
        let a = list.add(
            "https://ornek.com/dosya.zip?x=1",
            r"C:\Users\a\Downloads\dosya (1).zip",
            Some(100),
            0,
        );
        assert_eq!(list.get(a).unwrap().name(), "dosya (1).zip");
        let b = list.add("https://ornek.com/belge%20son.pdf", "", None, 0);
        assert_eq!(list.get(b).unwrap().name(), "belge son.pdf");
        let c = list.add("data:text/plain,x", "", None, 0);
        assert_eq!(list.get(c).unwrap().name(), "indirme");
    }

    #[test]
    fn badge_follows_downloads() {
        let mut list = List::default();
        assert_eq!(list.badge(), None);
        let a = list.add("https://a/1", "1", Some(100), 0);
        let b = list.add("https://a/2", "2", Some(300), 0);
        list.get_mut(a).unwrap().received = 100;
        list.get_mut(b).unwrap().received = 100;
        let badge = list.badge().unwrap();
        assert!(badge.active && !badge.attention);
        assert_eq!(badge.progress, Some(0.5));
        // Boyutu bilinmeyen indirme varsa toplam ilerleme bilinmez.
        let c = list.add("https://a/3", "3", None, 0);
        assert_eq!(list.badge().unwrap().progress, None);
        list.set_state(c, State::Cancelled);
        list.set_state(a, State::Done);
        list.set_state(b, State::Done);
        let badge = list.badge().unwrap();
        assert!(!badge.active && badge.attention);
        list.mark_seen();
        assert!(!list.badge().unwrap().attention);
        // Süren indirme kaldırılamaz; bitenler temizlenir.
        let d = list.add("https://a/4", "4", None, 0);
        list.remove(d);
        assert!(list.get(d).is_some());
        list.clear_finished();
        assert_eq!(list.json()["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn page_and_actions() {
        let mut list = List::default();
        list.add("https://a/x.zip", r"C:\x.zip", Some(10), 5);
        let html = render(&list);
        assert!(html.contains(r#""name":"x.zip""#));
        assert!(!html.contains("/*STATE*/null"));
        assert_eq!(Action::parse("pause"), Some(Action::Pause));
        assert_eq!(Action::parse("sil"), None);
        assert_eq!(reason_text(3), "Diskte yer yok");
    }
}
