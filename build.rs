fn main() {
    // Windows'ta modern görünümlü kontroller (Common Controls v6) ve
    // monitör başına DPI farkındalığı için uygulama manifestini göm.
    #[cfg(windows)]
    {
        use embed_manifest::{embed_manifest, new_manifest};
        if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
            embed_manifest(new_manifest("Julie.Browser")).expect("manifest gömülemedi");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
