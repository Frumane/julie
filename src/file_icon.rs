//! Dosyaların Windows'taki simgesi (Gezgin'de görünen), indirmeler sayfası
//! için PNG `data:` adresi olarak. Simgeler uzantıya göre bir kez çıkarılıp
//! saklanır; programların (`.exe`) kendi simgesi indirme bitince dosyadan
//! okunur.

use std::cell::RefCell;
use std::collections::HashMap;

use base64::Engine as _;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, IWICBitmapFrameEncode, IWICImagingFactory,
    WICBitmapEncoderNoCache,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{
    SHCreateMemStream, SHFILEINFOW, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
    SHGetImageList, SHIL_EXTRALARGE,
};
use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
use windows::core::HSTRING;

/// Simgesi her dosyada farklı olan uzantılar.
const OWN_ICON: [&str; 2] = ["exe", "ico"];

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    static GENERIC: std::cell::OnceCell<Option<i32>> = const { std::cell::OnceCell::new() };
}

/// `name` adlı dosyanın simgesi. `path` diskte bitmiş dosyaysa ve kendi
/// simgesi varsa (program) o kullanılır; yoksa uzantının simgesi.
pub fn data_url(name: &str, path: Option<&str>) -> Option<String> {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| !ext.is_empty() && ext.len() <= 8 && !ext.contains(['\\', '/', ' ']))
        .unwrap_or_default();
    let real =
        path.filter(|p| OWN_ICON.contains(&ext.as_str()) && std::path::Path::new(p).is_file());
    let key = match real {
        Some(path) => path.to_string(),
        None => format!(".{ext}"),
    };
    CACHE.with(|cache| {
        if let Some(found) = cache.borrow().get(&key) {
            return found.clone();
        }
        // Uzantısız dosya için genel dosya simgesi.
        let query = match real {
            Some(path) => path.to_string(),
            None if ext.is_empty() => "dosya".to_string(),
            None => format!("dosya.{ext}"),
        };
        // Windows'un "bilinmeyen dosya" simgesiyse sayfa kendi simgesini çizer.
        let generic = GENERIC.with(|g| *g.get_or_init(|| unsafe { icon_index("dosya", true) }));
        let png = match unsafe { icon_index(&query, real.is_none()) } {
            Some(index) if real.is_some() || Some(index) != generic => {
                unsafe { icon_png(index) }.ok()
            }
            _ => None,
        };
        let url = png.map(|png| {
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            )
        });
        cache.borrow_mut().insert(key, url.clone());
        url
    })
}

/// Dosyanın sistem simge listesindeki yeri. `by_extension`: dosya diskte
/// aranmaz, yalnızca adının uzantısına bakılır.
unsafe fn icon_index(file: &str, by_extension: bool) -> Option<i32> {
    unsafe {
        let mut info = SHFILEINFOW::default();
        let mut flags = SHGFI_SYSICONINDEX;
        if by_extension {
            flags |= SHGFI_USEFILEATTRIBUTES;
        }
        let found = SHGetFileInfoW(
            &HSTRING::from(file),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        (found != 0).then_some(info.iIcon)
    }
}

/// Sistem simge listesindeki simgeyi 48 piksel PNG olarak çıkarır.
unsafe fn icon_png(index: i32) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let images: IImageList = SHGetImageList(SHIL_EXTRALARGE as i32)?;
        let icon = images.GetIcon(index, ILD_TRANSPARENT.0)?;
        let png = encode(icon);
        let _ = DestroyIcon(icon);
        png
    }
}

unsafe fn encode(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bitmap = wic.CreateBitmapFromHICON(icon)?;
        let stream = SHCreateMemStream(None).ok_or_else(windows::core::Error::empty)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut props: Option<IPropertyBag2> = None;
        encoder.CreateNewFrame(&mut frame, &mut props)?;
        let frame = frame.ok_or_else(windows::core::Error::empty)?;
        frame.Initialize(props.as_ref())?;
        frame.WriteSource(&bitmap, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;

        let mut size = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut size))?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut png = vec![0u8; size as usize];
        let mut read = 0u32;
        stream
            .Read(png.as_mut_ptr().cast(), png.len() as u32, Some(&mut read))
            .ok()?;
        png.truncate(read as usize);
        Ok(png)
    }
}
