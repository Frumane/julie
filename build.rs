fn main() {
    // Windows'ta modern görünümlü kontroller (Common Controls v6) ve
    // monitör başına DPI farkındalığı için uygulama manifestini göm.
    #[cfg(windows)]
    {
        use embed_manifest::{embed_manifest, new_manifest};
        if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
            embed_manifest(new_manifest("Julie.Browser")).expect("manifest gömülemedi");

            // Uygulama ikonu (kaynak kimliği 1; pencere ikonu da buradan
            // yüklenir) ve Dosya Gezgini'nde görünen sürüm bilgisi.
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/julie.ico")
                .set("ProductName", "Julie")
                .set("FileDescription", "Julie")
                .set("LegalCopyright", "GPL-3.0-or-later");
            res.compile().expect("ikon gömülemedi");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/julie.ico");
}
