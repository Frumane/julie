//! Sayfanın ekran görüntüsü: görünen alan ya da baştan sona tüm sayfa.
//!
//! Görüntüyü sayfa motoru çizer (`Page.captureScreenshot`); Julie onu
//! İndirilenler klasörüne PNG olarak kaydeder ve panoya kopyalar.

use std::path::{Path, PathBuf};

/// Tüm sayfa görüntüsünün en fazla yüksekliği (CSS piksel). Daha uzun
/// sayfalar burada kesilir; motor çok büyük görüntüleri çizemiyor.
pub const MAX_HEIGHT: f64 = 16_000.0;

/// Görünen alanın görüntüsü için `Page.captureScreenshot` parametreleri.
pub fn visible_params() -> String {
    r#"{"format":"png"}"#.to_string()
}

/// `Page.getLayoutMetrics` yanıtından tüm sayfanın görüntüsü için
/// `Page.captureScreenshot` parametreleri.
pub fn full_params(metrics: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(metrics).ok()?;
    let size = value
        .get("cssContentSize")
        .or_else(|| value.get("contentSize"))?;
    let width = size.get("width")?.as_f64()?.ceil();
    let height = size.get("height")?.as_f64()?.ceil().min(MAX_HEIGHT);
    if width < 1.0 || height < 1.0 {
        return None;
    }
    Some(
        serde_json::json!({
            "format": "png",
            "captureBeyondViewport": true,
            "clip": { "x": 0, "y": 0, "width": width, "height": height, "scale": 1 },
        })
        .to_string(),
    )
}

/// `Page.captureScreenshot` yanıtındaki PNG.
pub fn png_from_reply(reply: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let value: serde_json::Value = serde_json::from_str(reply).ok()?;
    let data = value.get("data")?.as_str()?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    png.starts_with(b"\x89PNG").then_some(png)
}

/// Dosya adı: "Ekran görüntüsü 2026-11-26 153012.png".
pub fn file_name(
    (year, month, day, hour, minute, second): (u16, u16, u16, u16, u16, u16),
) -> String {
    format!("Ekran görüntüsü {year:04}-{month:02}-{day:02} {hour:02}{minute:02}{second:02}.png")
}

/// Klasörde olmayan bir ad: varsa sonuna " (2)", " (3)" … eklenir.
pub fn unique_path(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..)
        .map(|n| {
            if ext.is_empty() {
                dir.join(format!("{stem} ({n})"))
            } else {
                dir.join(format!("{stem} ({n}).{ext}"))
            }
        })
        .find(|p| !exists(p))
        .expect("sonsuz dizi")
}

/// Windows'a özgü işler: klasör, saat, pano.
#[cfg(windows)]
pub mod win {
    use std::path::PathBuf;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    /// Kullanıcının İndirilenler klasörü.
    pub fn downloads_dir() -> Option<PathBuf> {
        unsafe {
            let path = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
            let text = path.to_string().ok();
            CoTaskMemFree(Some(path.0 as *const _));
            text.map(PathBuf::from)
        }
    }

    /// Şimdiki yerel saat: (yıl, ay, gün, saat, dakika, saniye).
    pub fn now() -> (u16, u16, u16, u16, u16, u16) {
        let mut t = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        let t: windows_sys::Win32::Foundation::SYSTEMTIME = t;
        (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }

    /// PNG'yi panoya koyar: resim olarak (CF_DIB; Paint, Word, sohbet
    /// uygulamaları) ve PNG olarak (saydamlığı koruyan uygulamalar için).
    pub fn copy_to_clipboard(png: &[u8]) -> Result<(), String> {
        let dib = dib(png).map_err(|e| format!("resim çözülemedi: {e}"))?;
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
            SetClipboardData,
        };
        use windows_sys::Win32::System::Ole::CF_DIB;
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return Err("pano açılamadı".into());
            }
            EmptyClipboard();
            let name: Vec<u16> = "PNG\0".encode_utf16().collect();
            let png_format = RegisterClipboardFormatW(name.as_ptr());
            let mut ok = false;
            for (format, bytes) in [(u32::from(CF_DIB), dib.as_slice()), (png_format, png)] {
                if format == 0 {
                    continue;
                }
                if let Some(memory) = global(bytes)
                    && !SetClipboardData(format, memory).is_null()
                {
                    ok = true;
                }
            }
            CloseClipboard();
            if ok {
                Ok(())
            } else {
                Err("panoya yazılamadı".into())
            }
        }
    }

    /// Baytları taşınabilir bellek bloğuna kopyalar (pano onun sahibi olur).
    unsafe fn global(bytes: &[u8]) -> Option<*mut std::ffi::c_void> {
        use windows_sys::Win32::Foundation::GlobalFree;
        use windows_sys::Win32::System::Memory::{
            GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
        };
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if memory.is_null() {
                return None;
            }
            let target = GlobalLock(memory);
            if target.is_null() {
                GlobalFree(memory);
                return None;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
            GlobalUnlock(memory);
            Some(memory)
        }
    }

    /// PNG'yi 32 bitlik DIB'e (BITMAPINFOHEADER + satırlar) çevirir.
    fn dib(png: &[u8]) -> windows::core::Result<Vec<u8>> {
        unsafe {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = wic.CreateStream()?;
            stream.InitializeFromMemory(png)?;
            let decoder = wic.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = wic.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )?;
            let (mut w, mut h) = (0u32, 0u32);
            converter.GetSize(&mut w, &mut h)?;
            let stride = w * 4;
            let mut pixels = vec![0u8; (stride * h) as usize];
            converter.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
            Ok(super::dib_bytes(w, h, &pixels))
        }
    }
}

/// 32 bitlik BGRA piksellerinden (yukarıdan aşağı satırlar) DIB baytları:
/// 40 baytlık BITMAPINFOHEADER ve ardından satırlar. Yükseklik eksi
/// verilir; satırlar olduğu gibi (yukarıdan aşağı) kalır.
pub fn dib_bytes(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(40 + pixels.len());
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]); // çözünürlük ve renk tablosu: yok
    out.extend_from_slice(pixels);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_page_params() {
        let metrics = r#"{"cssContentSize":{"x":0,"y":0,"width":1263.4,"height":5021.2},
            "contentSize":{"width":1579,"height":6276}}"#;
        let params: serde_json::Value =
            serde_json::from_str(&full_params(metrics).unwrap()).unwrap();
        assert_eq!(params["captureBeyondViewport"], true);
        assert_eq!(params["clip"]["width"], 1264.0);
        assert_eq!(params["clip"]["height"], 5022.0);

        // Çok uzun sayfa kesilir.
        let long = r#"{"cssContentSize":{"width":800,"height":90000}}"#;
        let params: serde_json::Value = serde_json::from_str(&full_params(long).unwrap()).unwrap();
        assert_eq!(params["clip"]["height"], MAX_HEIGHT);

        // Eski motor: yalnızca contentSize.
        assert!(full_params(r#"{"contentSize":{"width":10,"height":10}}"#).is_some());
        assert!(full_params(r#"{"cssContentSize":{"width":0,"height":0}}"#).is_none());
        assert!(full_params("bozuk").is_none());
    }

    #[test]
    fn reply() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let reply = format!(r#"{{"data":"{data}"}}"#);
        assert_eq!(png_from_reply(&reply).as_deref(), Some(&png[..]));
        assert!(png_from_reply(r#"{"data":"aGVsbG8="}"#).is_none());
        assert!(png_from_reply("{}").is_none());
    }

    #[test]
    fn names() {
        assert_eq!(
            file_name((2026, 11, 26, 9, 5, 3)),
            "Ekran görüntüsü 2026-11-26 090503.png"
        );
        let dir = Path::new("d");
        let taken = [dir.join("a.png"), dir.join("a (2).png")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        assert_eq!(unique_path(dir, "a.png", exists), dir.join("a (3).png"));
        assert_eq!(unique_path(dir, "b.png", exists), dir.join("b.png"));
    }

    #[test]
    fn dib() {
        let dib = dib_bytes(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(dib.len(), 48);
        assert_eq!(&dib[0..4], &40u32.to_le_bytes());
        assert_eq!(&dib[8..12], &(-1i32).to_le_bytes());
        assert_eq!(&dib[14..16], &32u16.to_le_bytes());
        assert_eq!(&dib[40..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
