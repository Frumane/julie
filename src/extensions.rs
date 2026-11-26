//! Eklentiler: Chrome Web Mağazası'ndan indirilen paketi (CRX) açma,
//! kimliğini doğrulama, `manifest.json` okuma ve güncelleme denetimi.
//!
//! Eklentileri WebView2 çalıştırır (`AddBrowserExtension`); burada yalnızca
//! dosyalar hazırlanır. Mağazadan kurulanlar
//! `%LOCALAPPDATA%\Julie\extensions\<kimlik>\<sürüm>\` klasöründe durur;
//! WebView2 eklentiyi her açılışta oradan yükler, klasör silinmemeli.

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Chrome Web Mağazası.
pub const STORE_HOST: &str = "chromewebstore.google.com";
pub const STORE_URL: &str = "https://chromewebstore.google.com/category/extensions";

/// Bir paketin açılmış hâli en fazla bu kadar olabilir (sıkıştırma bombası).
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
/// İndirilen paket en fazla bu kadar olabilir.
pub const MAX_CRX: u64 = 256 * 1024 * 1024;

/// Eklenti kimliği: `a`–`p` harflerinden 32 karakter.
pub fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Açık anahtardan eklenti kimliği: SHA-256'nın ilk 16 baytı, her yarım
/// bayt `a`–`p` harfiyle.
pub fn id_from_key(key: &[u8]) -> String {
    id_from_hash(&Sha256::digest(key)[..16])
}

fn id_from_hash(hash: &[u8]) -> String {
    hash.iter()
        .flat_map(|b| [b >> 4, b & 15])
        .map(|n| (b'a' + n) as char)
        .collect()
}

/// Mağaza adresinden ya da yazılan metinden eklenti kimliği.
pub fn find_id(text: &str) -> Option<String> {
    let text = text.trim();
    let path = text.split(['?', '#']).next().unwrap_or(text);
    path.split(['/', '\\', ' '])
        .rev()
        .find(|part| is_id(part))
        .map(str::to_string)
}

/// Sayfa adresi mağazanın kendisi mi? (Yalnızca oradan gelen "ekle"
/// isteği kabul edilir.)
pub fn is_store_page(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .is_some_and(|host| host.eq_ignore_ascii_case(STORE_HOST))
}

/// Paketin indirme adresi. `browser`: Chromium sürümü (mağaza eklentinin
/// bu sürümde çalışan hâlini verir).
pub fn download_url(id: &str, browser: &str) -> String {
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={browser}&acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc"
    )
}

/// Güncelleme denetimi adresi: (kimlik, kurulu sürüm) listesi.
pub fn update_url(installed: &[(String, String)], browser: &str) -> String {
    let mut url = format!(
        "https://clients2.google.com/service/update2/crx?response=updatecheck&prodversion={browser}&acceptformat=crx2,crx3"
    );
    for (id, version) in installed {
        url.push_str(&format!("&x=id%3D{id}%26v%3D{version}%26uc"));
    }
    url
}

/// Güncelleme yanıtından (kimlik, yeni sürüm) listesi.
pub fn parse_update(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for app in xml.split("<app ").skip(1) {
        let Some(id) = attribute(app, "appid").filter(|id| is_id(id)) else {
            continue;
        };
        let Some(check) = app.split("<updatecheck").nth(1) else {
            continue;
        };
        let check = &check[..check.find('>').unwrap_or(check.len())];
        if attribute(check, "status").as_deref() != Some("ok") {
            continue;
        }
        if let Some(version) = attribute(check, "version").filter(|v| is_version(v)) {
            out.push((id, version));
        }
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        rest = &rest[at + name.len()..];
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let value = &after[1..];
        return Some(value[..value.find(quote)?].to_string());
    }
    None
}

/// Sürüm: noktayla ayrılmış en fazla dört sayı (`1.2.3.4`).
pub fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() <= 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `new`, `old` sürümünden yeni mi?
pub fn is_newer(new: &str, old: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(new), parse(old));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// Açılmış paket: eklentinin açık anahtarı, kimliği ve ZIP kısmı.
#[derive(Debug)]
pub struct Crx<'a> {
    pub key: Vec<u8>,
    pub id: String,
    pub zip: &'a [u8],
}

/// CRX paketinin başlığını okur. Mağazanın imzalı başlığındaki kimliğe
/// uyan anahtar seçilir. (İmzanın kendisi denetlenmez; paket Google'dan
/// HTTPS ile gelir ve kimlik istenenle karşılaştırılır.)
pub fn parse_crx(bytes: &[u8]) -> Result<Crx<'_>, String> {
    let bad = || "eklenti paketi bozuk".to_string();
    if bytes.get(..4) != Some(b"Cr24") {
        return Err("indirilen dosya eklenti paketi değil".into());
    }
    let u32_at = |at: usize| -> Option<usize> {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    match u32_at(4).ok_or_else(bad)? {
        2 => {
            let key_len = u32_at(8).ok_or_else(bad)?;
            let sig_len = u32_at(12).ok_or_else(bad)?;
            let key = bytes.get(16..16 + key_len).ok_or_else(bad)?.to_vec();
            let zip = bytes.get(16 + key_len + sig_len..).ok_or_else(bad)?;
            Ok(Crx {
                id: id_from_key(&key),
                key,
                zip,
            })
        }
        3 => {
            let header_len = u32_at(8).ok_or_else(bad)?;
            let header = bytes.get(12..12 + header_len).ok_or_else(bad)?;
            let zip = bytes.get(12 + header_len..).ok_or_else(bad)?;
            let mut keys = Vec::new();
            let mut crx_id = None;
            for (field, value) in proto_fields(header).ok_or_else(bad)? {
                match field {
                    // sha256_with_rsa / sha256_with_ecdsa: AsymmetricKeyProof
                    2 | 3 => {
                        if let Some((_, key)) = proto_fields(value)
                            .ok_or_else(bad)?
                            .into_iter()
                            .find(|(f, _)| *f == 1)
                        {
                            keys.push(key.to_vec());
                        }
                    }
                    // signed_header_data: SignedData { crx_id = 1 }
                    10000 => {
                        crx_id = proto_fields(value)
                            .ok_or_else(bad)?
                            .into_iter()
                            .find(|(f, _)| *f == 1)
                            .map(|(_, id)| id_from_hash(id));
                    }
                    _ => {}
                }
            }
            let key = match &crx_id {
                Some(id) => keys.into_iter().find(|k| id_from_key(k) == *id),
                None => keys.into_iter().next(),
            }
            .ok_or_else(|| "eklenti paketinin anahtarı yok".to_string())?;
            Ok(Crx {
                id: id_from_key(&key),
                key,
                zip,
            })
        }
        _ => Err("eklenti paketinin sürümü tanınmıyor".into()),
    }
}

/// Protobuf iletisinin uzunluklu alanları (alan numarası, içerik). Diğer
/// türler atlanır.
fn proto_fields(mut data: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    fn varint(data: &mut &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&b, rest) = data.split_first()?;
            *data = rest;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    let mut out = Vec::new();
    while !data.is_empty() {
        let tag = varint(&mut data)?;
        match tag & 7 {
            0 => {
                varint(&mut data)?;
            }
            1 => data = data.get(8..)?,
            2 => {
                let len = usize::try_from(varint(&mut data)?).ok()?;
                out.push((tag >> 3, data.get(..len)?));
                data = data.get(len..)?;
            }
            5 => data = data.get(4..)?,
            _ => return None,
        }
    }
    Some(out)
}

/// ZIP arşivini `dest` klasörüne açar. Klasörün dışına çıkan ya da mutlak
/// yollu girdiler reddedilir.
pub fn unzip(zip: &[u8], dest: &Path) -> Result<(), String> {
    let bad = || "eklenti arşivi bozuk".to_string();
    let u16_at = |at: usize| -> Option<usize> {
        let b = zip.get(at..at + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]) as usize)
    };
    let u32_at = |at: usize| -> Option<usize> {
        let b = zip.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    // Arşiv sonu kaydı: sondan geriye doğru aranır (yorum olabilir).
    let lowest = zip.len().saturating_sub(22 + 65535);
    let end = (lowest..=zip.len().saturating_sub(22))
        .rev()
        .find(|&at| zip.get(at..at + 4) == Some(&[0x50, 0x4b, 5, 6][..]))
        .ok_or_else(bad)?;
    let count = u16_at(end + 10).ok_or_else(bad)?;
    let mut at = u32_at(end + 16).ok_or_else(bad)?;
    let mut total = 0u64;
    std::fs::create_dir_all(dest).map_err(|e| format!("klasör açılamadı: {e}"))?;
    for _ in 0..count {
        if zip.get(at..at + 4) != Some(&[0x50, 0x4b, 1, 2][..]) {
            return Err(bad());
        }
        let method = u16_at(at + 10).ok_or_else(bad)?;
        let packed = u32_at(at + 20).ok_or_else(bad)?;
        let size = u32_at(at + 24).ok_or_else(bad)? as u64;
        let name_len = u16_at(at + 28).ok_or_else(bad)?;
        let extra_len = u16_at(at + 30).ok_or_else(bad)?;
        let comment_len = u16_at(at + 32).ok_or_else(bad)?;
        let local = u32_at(at + 42).ok_or_else(bad)?;
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?;
        let name = String::from_utf8_lossy(name).replace('\\', "/");
        at += 46 + name_len + extra_len + comment_len;

        let Some(relative) = safe_path(&name) else {
            return Err(format!("eklenti arşivinde geçersiz yol: {name}"));
        };
        let target = dest.join(&relative);
        if name.ends_with('/') {
            std::fs::create_dir_all(&target).map_err(|e| format!("klasör açılamadı: {e}"))?;
            continue;
        }
        if zip.get(local..local + 4) != Some(&[0x50, 0x4b, 3, 4][..]) {
            return Err(bad());
        }
        let start = local
            + 30
            + u16_at(local + 26).ok_or_else(bad)?
            + u16_at(local + 28).ok_or_else(bad)?;
        let data = zip.get(start..start + packed).ok_or_else(bad)?;
        total += size;
        if total > MAX_UNPACKED {
            return Err("eklenti çok büyük".into());
        }
        let mut bytes = Vec::with_capacity(size.min(MAX_UNPACKED) as usize);
        match method {
            0 => bytes.extend_from_slice(data),
            8 => {
                flate2::read::DeflateDecoder::new(data)
                    .take(size + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| bad())?;
            }
            _ => return Err("eklenti arşivi desteklenmeyen biçimde sıkıştırılmış".into()),
        }
        if bytes.len() as u64 != size {
            return Err(bad());
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("klasör açılamadı: {e}"))?;
        }
        std::fs::write(&target, &bytes).map_err(|e| format!("dosya yazılamadı: {e}"))?;
    }
    Ok(())
}

/// Arşivdeki yol klasörün içinde kalıyorsa göreli yolu.
fn safe_path(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            _ if part.contains(':') => return None,
            _ => out.push(part),
        }
    }
    (!out.as_os_str().is_empty() && !name.starts_with('/')).then_some(out)
}

/// `manifest.json`'dan Julie'nin gösterdikleri.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    /// En uygun simgenin klasöre göre yolu.
    pub icon: Option<String>,
    /// Kullanıcıya gösterilecek izin uyarıları.
    pub warnings: Vec<String>,
}

/// Klasördeki `manifest.json`'u okur.
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let value = manifest_value(dir)?;
    let text = |key: &str| {
        let raw = value.get(key).and_then(Value::as_str).unwrap_or_default();
        localized(dir, &value, raw)
    };
    let name = text("name");
    if name.trim().is_empty() {
        return Err("eklentinin adı yok".into());
    }
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(Manifest {
        name: name.chars().take(80).collect(),
        description: text("description").chars().take(300).collect(),
        version,
        icon: best_icon(&value),
        warnings: warnings(&value),
    })
}

fn manifest_value(dir: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(dir.join("manifest.json"))
        .map_err(|_| "eklentide manifest.json yok".to_string())?;
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    serde_json::from_slice(bytes).map_err(|_| "manifest.json okunamadı".to_string())
}

/// `__MSG_ad__` biçimindeki metni eklentinin dil dosyasından çevirir.
fn localized(dir: &Path, manifest: &Value, raw: &str) -> String {
    let Some(key) = raw
        .strip_prefix("__MSG_")
        .and_then(|k| k.strip_suffix("__"))
    else {
        return raw.to_string();
    };
    let default = manifest
        .get("default_locale")
        .and_then(Value::as_str)
        .unwrap_or("en");
    for locale in ["tr", default, "en", "en_US"] {
        if locale.contains(['/', '\\', '.']) {
            continue;
        }
        let path = dir.join("_locales").join(locale).join("messages.json");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
        let Ok(Value::Object(messages)) = serde_json::from_slice::<Value>(bytes) else {
            continue;
        };
        if let Some(message) = messages
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.get("message")?.as_str())
        {
            return message.to_string();
        }
    }
    raw.to_string()
}

/// 48 piksele en yakın (tercihen büyük) simge.
fn best_icon(manifest: &Value) -> Option<String> {
    let icons = manifest.get("icons")?.as_object()?;
    icons
        .iter()
        .filter_map(|(size, path)| Some((size.parse::<u32>().ok()?, path.as_str()?)))
        .min_by_key(|(size, _)| {
            if *size >= 48 {
                size - 48
            } else {
                1000 + 48 - size
            }
        })
        .map(|(_, path)| path.trim_start_matches('/').to_string())
}

/// Eklentinin istediği izinler, Chrome'un kurulumda sorduğu gibi.
pub fn warnings(manifest: &Value) -> Vec<String> {
    let list = |key: &str| -> Vec<String> {
        manifest
            .get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut permissions = list("permissions");
    let mut hosts = list("host_permissions");
    // Manifest V2'de site izinleri de `permissions` içindedir.
    permissions.retain(|p| {
        if p.contains("://") || p == "<all_urls>" {
            hosts.push(p.clone());
            false
        } else {
            true
        }
    });
    if let Some(scripts) = manifest.get("content_scripts").and_then(Value::as_array) {
        for script in scripts {
            if let Some(matches) = script.get("matches").and_then(Value::as_array) {
                hosts.extend(matches.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
    }

    let mut out = Vec::new();
    let all = hosts.iter().any(|h| {
        h == "<all_urls>"
            || h.starts_with("*://*/")
            || h.starts_with("http://*/")
            || h.starts_with("https://*/")
    });
    if all {
        out.push("Ziyaret ettiğin bütün sitelerdeki verilerini okuyup değiştirebilir".to_string());
    } else {
        let mut names: Vec<String> = hosts
            .iter()
            .filter_map(|h| {
                let host = h.split("://").nth(1)?.split('/').next()?;
                let host = host.trim_start_matches("*.");
                (!host.is_empty() && host != "*").then(|| host.to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        match names.len() {
            0 => {}
            1..=3 => out.push(format!(
                "Şu sitelerdeki verilerini okuyup değiştirebilir: {}",
                names.join(", ")
            )),
            n => out.push(format!(
                "{}, {} ve {} başka sitedeki verilerini okuyup değiştirebilir",
                names[0],
                names[1],
                n - 2
            )),
        }
    }
    const TEXTS: [(&str, &str); 16] = [
        ("debugger", "Sayfaların içine erişip onları denetleyebilir"),
        ("history", "Gezinme geçmişini okuyup değiştirebilir"),
        ("tabs", "Sekmelerini ve gezinme etkinliğini görebilir"),
        (
            "webNavigation",
            "Sekmelerini ve gezinme etkinliğini görebilir",
        ),
        ("bookmarks", "Yer imlerini okuyup değiştirebilir"),
        ("downloads", "İndirmelerini yönetebilir"),
        ("clipboardRead", "Panoya kopyaladıklarını okuyabilir"),
        ("clipboardWrite", "Panoya yazabilir"),
        ("cookies", "Sitelerin çerezlerini okuyup değiştirebilir"),
        ("geolocation", "Konumunu öğrenebilir"),
        ("management", "Diğer eklentileri yönetebilir"),
        (
            "nativeMessaging",
            "Bilgisayardaki başka programlarla konuşabilir",
        ),
        ("notifications", "Bildirim gösterebilir"),
        ("privacy", "Gizlilik ayarlarını değiştirebilir"),
        ("proxy", "Bağlantı ayarlarını (proxy) değiştirebilir"),
        ("topSites", "En çok ziyaret ettiğin siteleri görebilir"),
    ];
    for (permission, text) in TEXTS {
        if permissions.iter().any(|p| p == permission) && !out.iter().any(|o| o == text) {
            out.push(text.to_string());
        }
    }
    out
}

/// Mağaza paketindeki açık anahtarı `manifest.json`'a yazar: eklenti klasörden
/// yüklense de mağazadaki kimliğini korur (ayarları, bağlantıları bozulmaz).
pub fn add_key(dir: &Path, key: &[u8]) -> Result<(), String> {
    let mut value = manifest_value(dir)?;
    let Some(object) = value.as_object_mut() else {
        return Err("manifest.json okunamadı".into());
    };
    object.insert(
        "key".into(),
        Value::String(base64::engine::general_purpose::STANDARD.encode(key)),
    );
    // Mağazanın güncelleme adresi WebView2'de işe yaramaz; Julie denetler.
    object.remove("update_url");
    std::fs::write(dir.join("manifest.json"), value.to_string())
        .map_err(|e| format!("manifest.json yazılamadı: {e}"))
}

/// İndirilen paketi `staging` klasörüne açar; kimlik istenenle aynı olmalı.
pub fn unpack(bytes: &[u8], expected: &str, staging: &Path) -> Result<Manifest, String> {
    let crx = parse_crx(bytes)?;
    if crx.id != expected {
        return Err("indirilen paket istenen eklentiye ait değil".into());
    }
    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|e| format!("eski dosyalar silinemedi: {e}"))?;
    }
    let result = unzip(crx.zip, staging)
        .and_then(|()| add_key(staging, &crx.key))
        .and_then(|()| read_manifest(staging));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(staging);
    }
    let manifest = result?;
    if !is_version(&manifest.version) {
        let _ = std::fs::remove_dir_all(staging);
        return Err("eklentinin sürümü geçersiz".into());
    }
    Ok(manifest)
}

/// Mağazadan kurulmuş eklentinin en yeni sürüm klasörü.
pub fn installed_dir(root: &Path, id: &str) -> Option<(String, PathBuf)> {
    if !is_id(id) {
        return None;
    }
    std::fs::read_dir(root.join(id))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            (is_version(&name) && path.join("manifest.json").is_file()).then_some((name, path))
        })
        .reduce(|best, next| {
            if is_newer(&next.0, &best.0) {
                next
            } else {
                best
            }
        })
}

/// Eklentinin eski sürüm klasörlerini siler.
pub fn remove_old_versions(root: &Path, id: &str, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root.join(id)) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.file_name().to_string_lossy() != keep {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Klasörden yüklenen eklentiler: `folders.txt`, her satırda kimlik ve yol.
pub fn read_folders(root: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_to_string(root.join("folders.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (id, path) = line.split_once('\t')?;
            Some((id.to_string(), PathBuf::from(path)))
        })
        .collect()
}

pub fn write_folders(root: &Path, folders: &[(String, PathBuf)]) {
    let text: String = folders
        .iter()
        .map(|(id, path)| format!("{id}\t{}\n", path.display()))
        .collect();
    let _ = std::fs::create_dir_all(root);
    let _ = std::fs::write(root.join("folders.txt"), text);
}

/// Eklenti listesinin bir satırı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub version: String,
    pub description: String,
    /// `data:` adresi.
    pub icon: Option<String>,
    /// Mağazadan mı kuruldu (güncellenebilir).
    pub from_store: bool,
    /// Mağazadaki yeni sürüm.
    pub update: Option<String>,
}

/// Kurulmadan önce onay bekleyen eklenti.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    pub manifest: Manifest,
    pub icon: Option<String>,
    /// Açıldığı geçici klasör.
    pub dir: PathBuf,
    /// Kurulu eklentinin güncellemesi mi?
    pub update: bool,
}

/// Eklentiler sayfasının durumu.
#[derive(Debug, Default)]
pub struct State {
    pub entries: Vec<Entry>,
    pub pending: Option<Pending>,
    /// "İndiriliyor…" gibi süren işin metni.
    pub busy: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Gizli pencere: eklentiler çalışmaz.
    pub private: bool,
    /// Bu oturumda güncellemeler denetlendi mi?
    pub checked: bool,
}

impl State {
    pub fn json(&self) -> Value {
        json!({
            "private": self.private,
            "busy": self.busy,
            "error": self.error,
            "notice": self.notice,
            "checked": self.checked,
            "storeUrl": STORE_URL,
            "pending": self.pending.as_ref().map(|p| json!({
                "id": p.id,
                "name": p.manifest.name,
                "version": p.manifest.version,
                "description": p.manifest.description,
                "warnings": p.manifest.warnings,
                "icon": p.icon,
                "update": p.update,
            })),
            "items": self.entries.iter().map(|e| json!({
                "id": e.id,
                "name": e.name,
                "enabled": e.enabled,
                "version": e.version,
                "description": e.description,
                "icon": e.icon,
                "fromStore": e.from_store,
                "update": e.update,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Eklentiler sayfasından gelen istek.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Onay bekleyen eklentiyi kur.
    Install,
    /// Onay bekleyen eklentiden vazgeç.
    Cancel,
    Toggle(String),
    Remove(String),
    /// Klasör seçip açılmış eklenti yükle.
    Folder,
    /// Yazılan mağaza adresindeki ya da kimlikteki eklentiyi indir.
    Add(String),
    /// Mağazadan kurulanların yeni sürümünü sor.
    Check,
    Update(String),
    /// Mağazayı yeni sekmede aç.
    Store,
    /// Sayfa açıldı.
    Opened,
}

impl Command {
    pub fn parse(raw: &str) -> Option<Command> {
        let id = |rest: &str| is_id(rest).then(|| rest.to_string());
        Some(match raw {
            "ext-install" => Command::Install,
            "ext-cancel" => Command::Cancel,
            "ext-folder" => Command::Folder,
            "ext-check" => Command::Check,
            "ext-store" => Command::Store,
            "ext-opened" => Command::Opened,
            _ => {
                if let Some(rest) = raw.strip_prefix("ext-toggle:") {
                    Command::Toggle(id(rest)?)
                } else if let Some(rest) = raw.strip_prefix("ext-remove:") {
                    Command::Remove(id(rest)?)
                } else if let Some(rest) = raw.strip_prefix("ext-update:") {
                    Command::Update(id(rest)?)
                } else {
                    Command::Add(raw.strip_prefix("ext-add:")?.chars().take(2000).collect())
                }
            }
        })
    }
}

const TEMPLATE: &str = include_str!("extensions_page.html");

/// `julie://extensions` sayfası.
pub fn render(state: &State) -> String {
    crate::start_page::embed_state(TEMPLATE, &state.json())
}

/// Klasördeki simgeyi sayfada gösterilecek `data:` adresine çevirir.
pub fn icon_data(dir: &Path, icon: Option<&str>) -> Option<String> {
    let relative = safe_path(icon?)?;
    let bytes = std::fs::read(dir.join(relative)).ok()?;
    if bytes.len() > 256 * 1024 {
        return None;
    }
    let mime = if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") {
        "image/webp"
    } else {
        return None;
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// Mağaza sayfasına "Julie'ye ekle" düğmesini koyan betik. İstek yalnızca
/// mağazanın kendi adresinden geldiyse kabul edilir (`is_store_page`).
pub const STORE_SCRIPT: &str = r#"(() => {
  if (location.hostname !== "chromewebstore.google.com" || window !== window.top) return;
  let host = null;
  const idOf = () => {
    const parts = location.pathname.split("/").filter(Boolean);
    return parts[0] === "detail" && /^[a-p]{32}$/.test(parts[parts.length - 1]) ? parts[parts.length - 1] : null;
  };
  const update = () => {
    const id = idOf();
    if (!id) { if (host) host.hidden = true; return; }
    if (!host) {
      host = document.createElement("julie-extension");
      const root = host.attachShadow({ mode: "closed" });
      root.innerHTML = `<style>
        button { position: fixed; z-index: 2147483647; right: 24px; bottom: 24px;
          font: 600 15px "Segoe UI", system-ui, sans-serif; color: #fff; background: #2f6fde;
          border: 0; border-radius: 999px; padding: 12px 22px; cursor: pointer;
          box-shadow: 0 6px 20px rgba(0,0,0,.25); }
        button:hover { filter: brightness(1.08); }
      </style><button type="button">Julie'ye ekle</button>`;
      root.querySelector("button").addEventListener("click", () => {
        const now = idOf();
        if (now && window.ipc) window.ipc.postMessage("julie-extension:" + now);
      });
      (document.body || document.documentElement).append(host);
    }
    host.hidden = false;
  };
  const start = () => { update(); setInterval(update, 1000); };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();
})();"#;

/// Ağ işleri (yalnızca Windows'ta: ureq orada).
#[cfg(windows)]
pub mod net {
    use std::time::Duration;

    fn agent() -> ureq::Agent {
        use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
        ureq::Agent::config_builder()
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into()
    }

    /// Eklenti paketini indirir.
    pub fn download(id: &str, browser: &str) -> Result<Vec<u8>, String> {
        agent()
            .get(&super::download_url(id, browser))
            .call()
            .map_err(|e| match e {
                ureq::Error::StatusCode(404 | 204) => "eklenti mağazada bulunamadı".to_string(),
                e => format!("eklenti indirilemedi: {e}"),
            })?
            .body_mut()
            .with_config()
            .limit(super::MAX_CRX)
            .read_to_vec()
            .map_err(|e| format!("eklenti indirilemedi: {e}"))
    }

    /// Mağazaya kurulu sürümleri sorar: yeni sürümü olanlar.
    pub fn check_updates(
        installed: &[(String, String)],
        browser: &str,
    ) -> Result<Vec<(String, String)>, String> {
        let xml = agent()
            .get(&super::update_url(installed, browser))
            .call()
            .map_err(|e| format!("güncellemeler denetlenemedi: {e}"))?
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| format!("güncellemeler denetlenemedi: {e}"))?;
        Ok(super::parse_update(&xml)
            .into_iter()
            .filter(|(id, version)| {
                installed
                    .iter()
                    .any(|(i, old)| i == id && super::is_newer(version, old))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn field(n: u64, data: &[u8], out: &mut Vec<u8>) {
        varint(n << 3 | 2, out);
        varint(data.len() as u64, out);
        out.extend_from_slice(data);
    }

    /// Küçük bir ZIP: biri sıkıştırılmış, biri düz iki dosya.
    fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in files {
            let packed = if *deflate {
                let mut e =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 3, 4, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // saat, crc
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);

            central.extend_from_slice(&[0x50, 0x4b, 1, 2, 20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]);
            central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crx3(keys: &[&[u8]], id_key: &[u8], zip: &[u8]) -> Vec<u8> {
        let mut header = Vec::new();
        for key in keys {
            let mut proof = Vec::new();
            field(1, key, &mut proof);
            field(2, b"imza", &mut proof);
            field(2, &proof, &mut header);
        }
        let mut signed = Vec::new();
        field(1, &Sha256::digest(id_key)[..16], &mut signed);
        field(10000, &signed, &mut header);
        let mut out = b"Cr24".to_vec();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(zip);
        out
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("julie-ext-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn ids() {
        assert_eq!(id_from_hash(&[0x01, 0xfa]), "abpk");
        assert!(is_id(&id_from_key(b"anahtar")));
        assert!(!is_id("abc"));
        assert!(!is_id(&"z".repeat(32)));
        let id = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
        assert_eq!(
            find_id(&format!(
                "https://chromewebstore.google.com/detail/ublock-origin/{id}?hl=tr"
            )),
            Some(id.to_string())
        );
        assert_eq!(find_id(&format!("  {id} ")), Some(id.to_string()));
        assert_eq!(find_id("https://example.com/x"), None);
        assert!(is_store_page(
            "https://chromewebstore.google.com/detail/x/y"
        ));
        assert!(!is_store_page(
            "https://chromewebstore.google.com.evil.com/"
        ));
        assert!(!is_store_page("http://chromewebstore.google.com/"));
    }

    #[test]
    fn versions() {
        assert!(is_newer("1.10", "1.9"));
        assert!(is_newer("2", "1.9.9"));
        assert!(!is_newer("1.0", "1"));
        assert!(!is_newer("1.2", "1.3"));
        assert!(is_version("1.2.3.4"));
        assert!(!is_version("1.2.3.4.5"));
        assert!(!is_version("1..2"));
        assert!(!is_version("../x"));
    }

    #[test]
    fn crx3_picks_the_signed_key_and_unzips() {
        let manifest =
            br#"{"manifest_version":3,"name":"__MSG_ad__","version":"1.2","default_locale":"en",
            "update_url":"https://clients2.google.com/service/update2/crx",
            "icons":{"16":"i16.png","128":"/i128.png","48":"img/i48.png"},
            "permissions":["tabs","storage"],"host_permissions":["<all_urls>"]}"#;
        let archive = zip(&[
            ("manifest.json", manifest, true),
            (
                "_locales/en/messages.json",
                br#"{"AD":{"message":"Deneme"}}"#,
                false,
            ),
            ("img/", b"", false),
            ("img/i48.png", b"\x89PNG....", false),
        ]);
        let bytes = crx3(&[b"baska", b"anahtar"], b"anahtar", &archive);
        let crx = parse_crx(&bytes).unwrap();
        assert_eq!(crx.key, b"anahtar");
        assert_eq!(crx.id, id_from_key(b"anahtar"));

        let dir = temp("crx");
        assert!(unpack(&bytes, &id_from_key(b"baska"), &dir).is_err());
        let m = unpack(&bytes, &crx.id, &dir).unwrap();
        assert_eq!(m.name, "Deneme");
        assert_eq!(m.version, "1.2");
        assert_eq!(m.icon.as_deref(), Some("img/i48.png"));
        assert_eq!(
            m.warnings,
            vec![
                "Ziyaret ettiğin bütün sitelerdeki verilerini okuyup değiştirebilir".to_string(),
                "Sekmelerini ve gezinme etkinliğini görebilir".to_string()
            ]
        );
        let written: Value =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            written["key"],
            base64::engine::general_purpose::STANDARD.encode(b"anahtar")
        );
        assert!(written.get("update_url").is_none());
        assert!(
            icon_data(&dir, m.icon.as_deref())
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crx2_and_bad_files() {
        let archive = zip(&[("manifest.json", br#"{"name":"A","version":"1"}"#, false)]);
        let mut bytes = b"Cr24".to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(b"keysg");
        bytes.extend_from_slice(&archive);
        let crx = parse_crx(&bytes).unwrap();
        assert_eq!(crx.key, b"key");
        assert_eq!(crx.zip, &archive[..]);
        assert!(parse_crx(b"PK\x03\x04").is_err());
        assert!(parse_crx(b"Cr24\x03\0\0\0\xff\0\0\0").is_err());
    }

    #[test]
    fn zip_paths_stay_inside() {
        let dir = temp("zip");
        let evil = zip(&[("../disari.txt", b"x", false)]);
        assert!(unzip(&evil, &dir).is_err());
        let evil = zip(&[("C:/disari.txt", b"x", false)]);
        assert!(unzip(&evil, &dir).is_err());
        assert!(!dir.parent().unwrap().join("disari.txt").exists());
        assert_eq!(safe_path("a/./b"), Some(PathBuf::from("a").join("b")));
        assert_eq!(safe_path("/etc/x"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_warnings() {
        let m =
            json!({"content_scripts":[{"matches":["https://*.github.com/*","https://a.com/*"]}]});
        assert_eq!(
            warnings(&m),
            vec!["Şu sitelerdeki verilerini okuyup değiştirebilir: a.com, github.com".to_string()]
        );
        let m = json!({"permissions":["https://a.com/","https://b.com/","https://c.com/","https://d.com/"]});
        assert_eq!(
            warnings(&m),
            vec!["a.com, b.com ve 2 başka sitedeki verilerini okuyup değiştirebilir".to_string()]
        );
    }

    #[test]
    fn update_response() {
        let xml = r#"<?xml version="1.0"?><gupdate protocol="2.0">
          <app appid="cjpalhdlnbpafiamejdnhcphjbkeiagm" status="ok">
            <updatecheck codebase="https://x/y.crx" version="1.60.0" status="ok"/>
          </app>
          <app appid="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" status="ok"><updatecheck status="noupdate"/></app>
          <app appid="bad" status="ok"><updatecheck version="1" status="ok"/></app>
        </gupdate>"#;
        assert_eq!(
            parse_update(xml),
            vec![("cjpalhdlnbpafiamejdnhcphjbkeiagm".into(), "1.60.0".into())]
        );
        let url = update_url(&[("a".repeat(32), "1.0".into())], "130.0");
        assert!(url.contains("x=id%3Daaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa%26v%3D1.0%26uc"));
    }

    #[test]
    fn commands() {
        let id = "a".repeat(32);
        assert_eq!(Command::parse("ext-install"), Some(Command::Install));
        assert_eq!(
            Command::parse(&format!("ext-toggle:{id}")),
            Some(Command::Toggle(id.clone()))
        );
        assert_eq!(Command::parse("ext-remove:../x"), None);
        assert_eq!(Command::parse("ext-add:x"), Some(Command::Add("x".into())));
        assert_eq!(Command::parse("ext-nope"), None);
    }

    #[test]
    fn page_embeds_state() {
        let state = State {
            entries: vec![Entry {
                id: "a".repeat(32),
                name: "</script>".into(),
                enabled: true,
                version: "1".into(),
                description: String::new(),
                icon: None,
                from_store: true,
                update: Some("2".into()),
            }],
            ..State::default()
        };
        let html = render(&state);
        assert!(!html.contains("/*STATE*/null"));
        assert!(!html.contains("\"name\":\"</script>"));
    }

    /// Gerçek mağaza paketi: `JULIE_CRX=<dosya> JULIE_CRX_ID=<kimlik> cargo test real_crx -- --ignored`
    #[test]
    #[ignore]
    fn real_crx() {
        let bytes = std::fs::read(std::env::var("JULIE_CRX").unwrap()).unwrap();
        let id = std::env::var("JULIE_CRX_ID").unwrap();
        let dir = temp("real");
        let m = unpack(&bytes, &id, &dir).unwrap();
        println!("{m:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folders_round_trip() {
        let dir = temp("folders");
        let list = vec![("a".repeat(32), PathBuf::from("C:/eklenti"))];
        write_folders(&dir, &list);
        assert_eq!(read_folders(&dir), list);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
