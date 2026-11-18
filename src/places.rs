//! Geçmiş ve yer imleri (SQLite).
//!
//! Firefox'un `places.sqlite` dosyasına benzer: her adres bir kez `places`
//! tablosunda tutulur (başlık, ziyaret sayısı, son ziyaret), ziyaretlerin
//! kendisi `visits` tablosundadır. Yer imleri ayrı bir tablodadır.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

/// Geçmiş bu süreden eski kayıtları siler (Chrome ile aynı: 90 gün).
pub const HISTORY_DAYS: i64 = 90;

/// Şemanın sürümü; değişince `migrate` yeni adımları uygular.
const SCHEMA_VERSION: i32 = 2;

/// Saklanan en büyük site ikonu (bayt); daha büyükleri kaydedilmez.
pub const MAX_FAVICON_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub url: String,
    pub title: String,
    /// Unix zamanı (saniye): geçmişte son ziyaret, yer iminde eklenme anı.
    pub time: i64,
}

pub struct Places {
    conn: Connection,
}

impl Places {
    pub fn open(path: &Path) -> rusqlite::Result<Places> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Places::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Places> {
        Places::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Places> {
        // WAL: yazarken okumayı engellemez ve elektrik kesintisine dayanıklıdır.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let places = Places { conn };
        places.migrate()?;
        Ok(places)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS places (
                     id          INTEGER PRIMARY KEY,
                     url         TEXT NOT NULL UNIQUE,
                     title       TEXT NOT NULL DEFAULT '',
                     visit_count INTEGER NOT NULL DEFAULT 0,
                     last_visit  INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE INDEX IF NOT EXISTS places_last_visit ON places(last_visit);
                 CREATE TABLE IF NOT EXISTS visits (
                     id       INTEGER PRIMARY KEY,
                     place_id INTEGER NOT NULL REFERENCES places(id) ON DELETE CASCADE,
                     time     INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS visits_time ON visits(time);
                 CREATE INDEX IF NOT EXISTS visits_place ON visits(place_id);
                 CREATE TABLE IF NOT EXISTS bookmarks (
                     id      INTEGER PRIMARY KEY,
                     url     TEXT NOT NULL UNIQUE,
                     title   TEXT NOT NULL DEFAULT '',
                     created INTEGER NOT NULL
                 );",
            )?;
        }
        if version < 2 {
            // Site ikonları: alan adı başına bir PNG.
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS favicons (
                     host    TEXT PRIMARY KEY,
                     png     BLOB NOT NULL,
                     updated INTEGER NOT NULL
                 );",
            )?;
        }
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    // ---------------------------------------------------------------- geçmiş

    /// Bir sayfa ziyaretini kaydeder.
    pub fn record_visit(&self, url: &str, time: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO places (url, visit_count, last_visit) VALUES (?1, 1, ?2)
             ON CONFLICT(url) DO UPDATE SET
                 visit_count = visit_count + 1,
                 last_visit = max(last_visit, excluded.last_visit)",
            params![url, time],
        )?;
        tx.execute(
            "INSERT INTO visits (place_id, time) SELECT id, ?2 FROM places WHERE url = ?1",
            params![url, time],
        )?;
        tx.commit()
    }

    /// Sayfanın başlığını günceller (yer imi başlığına dokunmaz).
    pub fn set_title(&self, url: &str, title: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE places SET title = ?2 WHERE url = ?1",
            params![url, title],
        )?;
        Ok(())
    }

    /// Geçmişi son ziyarete göre yeniden eskiye listeler. `query` boş
    /// değilse adreste ya da başlıkta geçenler süzülür.
    pub fn history(&self, query: &str, limit: usize) -> rusqlite::Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT url, title, last_visit FROM places
             WHERE visit_count > 0
               AND (?1 = '' OR url LIKE ?2 ESCAPE '\\' OR title LIKE ?2 ESCAPE '\\')
             ORDER BY last_visit DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![query, like_pattern(query), limit as i64], |r| {
            Ok(Entry {
                url: r.get(0)?,
                title: r.get(1)?,
                time: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// Bir adresi geçmişten tamamen siler.
    pub fn delete_from_history(&self, url: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM places WHERE url = ?1", params![url])?;
        Ok(())
    }

    /// Bütün geçmişi siler (yer imleri kalır).
    pub fn clear_history(&self) -> rusqlite::Result<()> {
        self.conn
            .execute_batch("DELETE FROM visits; DELETE FROM places;")
    }

    /// `before` zamanından eski geçmişi siler.
    pub fn expire(&self, before: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM visits WHERE time < ?1", params![before])?;
        tx.execute("DELETE FROM places WHERE last_visit < ?1", params![before])?;
        tx.commit()
    }

    // ---------------------------------------------------------- site ikonları

    /// Sitenin ikonunu kaydeder (aynıysa dokunmaz).
    pub fn set_favicon(&self, host: &str, png: &[u8], time: i64) -> rusqlite::Result<()> {
        if png.is_empty() || png.len() > MAX_FAVICON_BYTES {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO favicons (host, png, updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET png = excluded.png, updated = excluded.updated
             WHERE favicons.png != excluded.png",
            params![host, png, time],
        )?;
        Ok(())
    }

    pub fn favicon(&self, host: &str) -> rusqlite::Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT png FROM favicons WHERE host = ?1",
                params![host],
                |r| r.get(0),
            )
            .optional()
    }

    // ------------------------------------------------------------- yer imleri

    pub fn add_bookmark(&self, url: &str, title: &str, time: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO bookmarks (url, title, created) VALUES (?1, ?2, ?3)
             ON CONFLICT(url) DO UPDATE SET title = excluded.title",
            params![url, title, time],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, url: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM bookmarks WHERE url = ?1", params![url])?;
        Ok(())
    }

    pub fn is_bookmarked(&self, url: &str) -> rusqlite::Result<bool> {
        self.conn
            .query_row(
                "SELECT 1 FROM bookmarks WHERE url = ?1",
                params![url],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
    }

    /// Yer imleri, eklenme sırasıyla.
    pub fn bookmarks(&self) -> rusqlite::Result<Vec<Entry>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT url, title, created FROM bookmarks ORDER BY created, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Entry {
                url: r.get(0)?,
                title: r.get(1)?,
                time: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    // ------------------------------------------------------------- öneriler

    /// Adres çubuğu önerileri: adresinde ya da başlığında `text` geçen
    /// geçmiş kayıtları ve yer imleri, en uygun olan başta.
    pub fn suggest(&self, text: &str, limit: usize, now: i64) -> rusqlite::Result<Vec<Suggestion>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        // Önce çok ziyaret edilenlerden geniş bir aday kümesi al, sonra sırala.
        let mut stmt = self.conn.prepare_cached(
            "SELECT p.url, CASE WHEN p.title = '' THEN coalesce(b.title, '') ELSE p.title END,
                    p.visit_count, p.last_visit, b.id IS NOT NULL
             FROM places p LEFT JOIN bookmarks b ON b.url = p.url
             WHERE p.visit_count > 0
               AND (p.url LIKE ?1 ESCAPE '\\' OR p.title LIKE ?1 ESCAPE '\\')
             UNION ALL
             SELECT b.url, b.title, 0, b.created, 1
             FROM bookmarks b
             WHERE NOT EXISTS (SELECT 1 FROM places p WHERE p.url = b.url AND p.visit_count > 0)
               AND (b.url LIKE ?1 ESCAPE '\\' OR b.title LIKE ?1 ESCAPE '\\')
             ORDER BY 3 DESC, 4 DESC
             LIMIT 200",
        )?;
        let rows = stmt.query_map(params![like_pattern(text)], |r| {
            Ok(Candidate {
                url: r.get(0)?,
                title: r.get(1)?,
                visits: r.get(2)?,
                last: r.get(3)?,
                bookmarked: r.get(4)?,
            })
        })?;
        let candidates = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rank(text, candidates, limit, now))
    }
}

/// Adres çubuğunda gösterilecek bir geçmiş kaydı ya da yer imi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    pub bookmarked: bool,
}

struct Candidate {
    url: String,
    title: String,
    visits: i64,
    last: i64,
    bookmarked: bool,
}

/// Adaylara puan verip en iyi `limit` tanesini döndürür.
///
/// Yazılan metinle başlayan alan adları ("you" → youtube.com) en üste çıkar;
/// sonra başlıktaki kelime başları, sonra yalnızca içinde geçenler gelir.
/// Sık ve yakın zamanda ziyaret edilenler ile yer imleri öne alınır.
fn rank(text: &str, candidates: Vec<Candidate>, limit: usize, now: i64) -> Vec<Suggestion> {
    let query = text.to_lowercase();
    let mut scored: Vec<(i64, Candidate)> = candidates
        .into_iter()
        .map(|c| (score(&query, &c, now), c))
        .collect();
    // Eşit puanda kısa adres (ana sayfa) önce.
    scored.sort_by(|(a, ca), (b, cb)| b.cmp(a).then(ca.url.len().cmp(&cb.url.len())));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, c)| Suggestion {
            url: c.url,
            title: c.title,
            bookmarked: c.bookmarked,
        })
        .collect()
}

fn score(query: &str, c: &Candidate, now: i64) -> i64 {
    let url = c.url.to_lowercase();
    let bare = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
    let bare = bare.strip_prefix("www.").unwrap_or(bare);
    let host = &bare[..bare.find(['/', '?', '#']).unwrap_or(bare.len())];
    let title = c.title.to_lowercase();

    let mut s = if bare.starts_with(query) {
        1000
    } else if host.split('.').any(|label| label.starts_with(query)) {
        600
    } else if title
        .split(|ch: char| !ch.is_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        300
    } else {
        50
    };
    // Sitenin ana sayfası, içindeki tek tek sayfalardan önce gelsin.
    if bare.len() <= host.len() + 1 {
        s += 150;
    }
    s += c.visits.min(50) * 8;
    let days = (now - c.last).max(0) / 86_400;
    s += match days {
        0 => 120,
        1..=6 => 80,
        7..=29 => 40,
        _ => 0,
    };
    if c.bookmarked {
        s += 250;
    }
    s
}

/// `LIKE` deseni: aranan metin içinde geçsin; `%` ve `_` özel anlam taşımasın.
fn like_pattern(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + 2);
    out.push('%');
    for c in query.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// Geçmişe kaydedilecek adresler: yalnızca web sayfaları.
pub fn is_recordable(url: &str) -> bool {
    (url.starts_with("http://") || url.starts_with("https://"))
        && !crate::address::is_internal_page(url)
}

/// Şu anki Unix zamanı (saniye).
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places() -> Places {
        Places::open_in_memory().unwrap()
    }

    #[test]
    fn records_visits_and_titles() {
        let p = places();
        p.record_visit("https://a.example/", 100).unwrap();
        p.record_visit("https://b.example/", 200).unwrap();
        p.record_visit("https://a.example/", 300).unwrap();
        p.set_title("https://a.example/", "A sayfası").unwrap();

        let history = p.history("", 10).unwrap();
        assert_eq!(
            history,
            vec![
                Entry {
                    url: "https://a.example/".into(),
                    title: "A sayfası".into(),
                    time: 300
                },
                Entry {
                    url: "https://b.example/".into(),
                    title: "".into(),
                    time: 200
                },
            ]
        );
        let visits: i64 = p
            .conn
            .query_row("SELECT count(*) FROM visits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(visits, 3);
    }

    #[test]
    fn searches_url_and_title_literally() {
        let p = places();
        p.record_visit("https://rust-lang.org/", 1).unwrap();
        p.set_title("https://rust-lang.org/", "Rust Programlama Dili")
            .unwrap();
        p.record_visit("https://example.com/%41", 2).unwrap();
        p.record_visit("https://example.com/xyz", 3).unwrap();

        let urls = |q: &str| -> Vec<String> {
            p.history(q, 10)
                .unwrap()
                .into_iter()
                .map(|e| e.url)
                .collect()
        };
        assert_eq!(urls("programlama"), vec!["https://rust-lang.org/"]);
        assert_eq!(urls("rust-lang"), vec!["https://rust-lang.org/"]);
        // `%` joker karakter olarak değil, harfiyen aranır.
        assert_eq!(urls("%41"), vec!["https://example.com/%41"]);
        assert_eq!(urls("_").len(), 0);
    }

    #[test]
    fn deletes_and_expires_history() {
        let p = places();
        p.record_visit("https://old.example/", 10).unwrap();
        p.record_visit("https://new.example/", 1000).unwrap();
        p.record_visit("https://gone.example/", 1000).unwrap();

        p.delete_from_history("https://gone.example/").unwrap();
        p.expire(500).unwrap();
        let urls: Vec<String> = p
            .history("", 10)
            .unwrap()
            .into_iter()
            .map(|e| e.url)
            .collect();
        assert_eq!(urls, vec!["https://new.example/"]);

        p.clear_history().unwrap();
        assert!(p.history("", 10).unwrap().is_empty());
    }

    #[test]
    fn bookmarks() {
        let p = places();
        assert!(!p.is_bookmarked("https://a.example/").unwrap());
        p.add_bookmark("https://a.example/", "A", 1).unwrap();
        p.add_bookmark("https://b.example/", "B", 2).unwrap();
        p.add_bookmark("https://a.example/", "A yeni", 3).unwrap();
        assert!(p.is_bookmarked("https://a.example/").unwrap());

        let marks = p.bookmarks().unwrap();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].title, "A yeni");
        assert_eq!(marks[0].time, 1, "yeniden eklemek sırayı değiştirmemeli");

        p.remove_bookmark("https://a.example/").unwrap();
        assert_eq!(p.bookmarks().unwrap().len(), 1);
        // Geçmişi temizlemek yer imlerine dokunmaz.
        p.clear_history().unwrap();
        assert_eq!(p.bookmarks().unwrap().len(), 1);
    }

    #[test]
    fn favicons() {
        let p = places();
        assert_eq!(p.favicon("example.com").unwrap(), None);
        p.set_favicon("example.com", b"png1", 1).unwrap();
        p.set_favicon("example.com", b"png2", 2).unwrap();
        assert_eq!(
            p.favicon("example.com").unwrap().as_deref(),
            Some(&b"png2"[..])
        );
        // Boş ya da çok büyük ikonlar saklanmaz.
        p.set_favicon("big.example", &vec![0u8; MAX_FAVICON_BYTES + 1], 3)
            .unwrap();
        p.set_favicon("empty.example", b"", 3).unwrap();
        assert_eq!(p.favicon("big.example").unwrap(), None);
        assert_eq!(p.favicon("empty.example").unwrap(), None);
    }

    #[test]
    fn upgrades_version_1_databases() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE places (id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE, title TEXT NOT NULL DEFAULT '', visit_count INTEGER NOT NULL DEFAULT 0, last_visit INTEGER NOT NULL DEFAULT 0); PRAGMA user_version = 1;").unwrap();
        let p = Places::init(conn).unwrap();
        p.set_favicon("example.com", b"png", 1).unwrap();
        assert!(p.favicon("example.com").unwrap().is_some());
    }

    #[test]
    fn persists_on_disk() {
        let dir = std::env::temp_dir().join(format!("julie-places-{}", std::process::id()));
        let path = dir.join("places.sqlite");
        {
            let p = Places::open(&path).unwrap();
            p.record_visit("https://a.example/", 5).unwrap();
            p.add_bookmark("https://a.example/", "A", 5).unwrap();
        }
        let p = Places::open(&path).unwrap();
        assert_eq!(p.history("", 10).unwrap().len(), 1);
        assert_eq!(p.bookmarks().unwrap().len(), 1);
        drop(p);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_web_pages_are_recorded() {
        assert!(is_recordable("https://example.com/"));
        assert!(!is_recordable("http://julie.start/"));
        assert!(!is_recordable("http://julie.history/?q=a"));
        assert!(!is_recordable("about:blank"));
        assert!(!is_recordable("file:///C:/x.html"));
    }

    #[test]
    fn suggests_matching_sites_best_first() {
        let p = places();
        let day = 86_400;
        let now = 100 * day;
        for _ in 0..3 {
            p.record_visit("https://www.youtube.com/", now - day)
                .unwrap();
        }
        p.set_title("https://www.youtube.com/", "YouTube").unwrap();
        p.record_visit("https://www.youtube.com/watch?v=abc", now - day)
            .unwrap();
        p.set_title("https://www.youtube.com/watch?v=abc", "Bir video")
            .unwrap();
        p.record_visit("https://example.com/yourself", now - 50 * day)
            .unwrap();
        p.record_visit("https://news.site/", now).unwrap();
        p.set_title("https://news.site/", "Your daily news")
            .unwrap();
        p.add_bookmark("https://tr.wikipedia.org/", "Vikipedi", now)
            .unwrap();

        let urls = |text: &str| -> Vec<String> {
            p.suggest(text, 10, now)
                .unwrap()
                .into_iter()
                .map(|s| s.url)
                .collect()
        };
        assert_eq!(
            urls("you"),
            vec![
                "https://www.youtube.com/",
                "https://www.youtube.com/watch?v=abc",
                "https://news.site/",
                "https://example.com/yourself",
            ]
        );
        // Ziyaret edilmemiş yer imi de bulunur; alt alan adının başı da eşleşir.
        assert_eq!(urls("wiki"), vec!["https://tr.wikipedia.org/"]);
        assert_eq!(urls("vikipedi"), vec!["https://tr.wikipedia.org/"]);
        assert!(urls("   ").is_empty());
        assert!(urls("100%").is_empty());
        assert_eq!(p.suggest("you", 2, now).unwrap().len(), 2);
    }

    #[test]
    fn suggestion_uses_bookmark_title_when_page_has_none() {
        let p = places();
        p.record_visit("https://a.example/", 10).unwrap();
        p.add_bookmark("https://a.example/", "Benim sitem", 10)
            .unwrap();
        let s = p.suggest("a.ex", 5, 20).unwrap();
        assert_eq!(
            s,
            vec![Suggestion {
                url: "https://a.example/".into(),
                title: "Benim sitem".into(),
                bookmarked: true,
            }]
        );
    }
}
