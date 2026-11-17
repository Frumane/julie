//! Hata günlüğü: `%LOCALAPPDATA%\Julie\julie.log`.
//!
//! Sürüm derlemesinde konsol penceresi olmadığı için hatalar bu dosyaya
//! yazılır (hata ayıklama derlemesinde ayrıca konsola da yazılır).

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::settings;

/// Günlük bu boyutu aşarsa baştan başlanır.
const MAX_LOG_BYTES: u64 = 256 * 1024;

pub fn error(message: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = format!("[{}] {message}\n", format_utc(secs));
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    let Some(dir) = settings::app_dir() else {
        return;
    };
    let path = dir.join("julie.log");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Unix zamanını `YYYY-AA-GG SS:DD:ss UTC` biçimine çevirir.
fn format_utc(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Howard Hinnant'ın "civil_from_days" algoritması.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::format_utc;

    #[test]
    fn formats_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_utc(1_763_380_800), "2025-11-17 12:00:00 UTC");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00 UTC");
    }
}
