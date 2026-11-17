//! Tarayıcının kendi arayüzü (sekmeler, adres çubuğu, butonlar).
//!
//! Tema ve yerleşim platformdan bağımsızdır ve test edilir; Windows'ta
//! Direct2D ile çizilir.

#[cfg(windows)]
pub mod d2d;
pub mod layout;
pub mod scene;
#[cfg(test)]
pub mod svg;
pub mod theme;
