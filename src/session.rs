//! Oturum: tarayıcı kapanırken açık sekmeleri kaydeder, açılışta geri getirir.
//!
//! Dosya biçimi (`session.txt`):
//!
//! ```text
//! active=1
//! tab=https://example.com/<TAB>Örnek sayfa
//! pin=https://mail.example/<TAB>Posta
//! tab=https://example.com/<TAB>Örnek sayfa
//! tab=https://www.youtube.com/<TAB>YouTube
//! ```
//!
//! Her sekme satırında adres ve başlık bir sekme karakteriyle (`<TAB>`,
//! `\t`) ayrılır. `pin=` satırları sabitlenmiş sekmelerdir.

use std::path::Path;

/// Kaydedilen en fazla sekme sayısı (bozuk ya da devasa dosyalara karşı).
const MAX_TABS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    /// Sabitlenmiş sekme.
    pub pinned: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    /// Öndeki sekmenin sırası.
    pub active: usize,
}

impl Session {
    pub fn parse(text: &str) -> Session {
        let mut session = Session::default();
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("active=") {
                session.active = value.trim().parse().unwrap_or(0);
            } else if let Some((value, pinned)) = line
                .strip_prefix("tab=")
                .map(|v| (v, false))
                .or_else(|| line.strip_prefix("pin=").map(|v| (v, true)))
            {
                let (url, title) = value.split_once('\t').unwrap_or((value, ""));
                let url = url.trim();
                if is_restorable(url) && session.tabs.len() < MAX_TABS {
                    session.tabs.push(SavedTab {
                        url: url.to_string(),
                        title: title.to_string(),
                        pinned,
                    });
                }
            }
        }
        if session.active >= session.tabs.len() {
            session.active = 0;
        }
        session
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("active={}\n", self.active);
        for tab in self.tabs.iter().take(MAX_TABS) {
            if !is_restorable(&tab.url) {
                continue;
            }
            out.push_str(if tab.pinned { "pin=" } else { "tab=" });
            out.push_str(&clean(&tab.url));
            out.push('\t');
            out.push_str(&clean(&tab.title));
            out.push('\n');
        }
        out
    }

    /// Dosya yoksa ya da okunamazsa boş oturum döner.
    pub fn load(path: &Path) -> Session {
        std::fs::read_to_string(path)
            .map(|text| Session::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Yazma yarıda kalırsa (elektrik kesintisi) eski oturum bozulmasın.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.serialize())?;
        std::fs::rename(tmp, path)
    }
}

/// Geri yüklenebilecek adresler: web sayfaları, yerel dosyalar ve
/// başlangıç sayfası. `about:blank`, `data:`, `javascript:` gibi adresler
/// kaydedilmez.
fn is_restorable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "file://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        && url.len() < 8192
}

/// Satır yapısını bozacak karakterleri temizler.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str, title: &str) -> SavedTab {
        SavedTab {
            url: url.into(),
            title: title.into(),
            pinned: false,
        }
    }

    #[test]
    fn round_trip() {
        let session = Session {
            tabs: vec![
                SavedTab {
                    pinned: true,
                    ..tab("https://mail.example/", "Posta")
                },
                tab("https://example.com/", "Örnek"),
                tab("http://julie.start/", "Yeni sekme"),
                tab("https://www.youtube.com/watch?v=1", ""),
            ],
            active: 2,
        };
        assert_eq!(Session::parse(&session.serialize()), session);
    }

    #[test]
    fn cleans_titles_with_tabs_and_newlines() {
        let session = Session {
            tabs: vec![tab("https://a.example/", "kötü\tbaşlık\nsatır")],
            active: 0,
        };
        let parsed = Session::parse(&session.serialize());
        assert_eq!(parsed.tabs[0].title, "kötü başlık satır");
        assert_eq!(parsed.tabs[0].url, "https://a.example/");
    }

    #[test]
    fn skips_unrestorable_urls_and_fixes_active() {
        let parsed = Session::parse(
            "active=7\ntab=javascript:alert(1)\ttitle\ntab=about:blank\t\ntab=https://ok.example/\tOK\n",
        );
        assert_eq!(parsed.tabs, vec![tab("https://ok.example/", "OK")]);
        assert_eq!(parsed.active, 0);
    }

    #[test]
    fn empty_or_missing_file_is_an_empty_session() {
        assert_eq!(Session::parse(""), Session::default());
        let missing = std::env::temp_dir().join("julie-yok/session.txt");
        assert_eq!(Session::load(&missing), Session::default());
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("julie-session-{}", std::process::id()));
        let path = dir.join("session.txt");
        let session = Session {
            tabs: vec![tab("https://example.com/", "Örnek")],
            active: 0,
        };
        session.save(&path).unwrap();
        assert_eq!(Session::load(&path), session);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
