# Julie

Hafif ve hızlı bir web tarayıcısı. Rust ile yazıldı ve Windows'un yerleşik
Chromium motoru **WebView2**'yi kullanıyor.

## Neden hafif?

- **Motoru biz taşımıyoruz:** WebView2 Windows 10/11'de zaten yüklü ve
  Microsoft tarafından güncel tutuluyor. Uygulama birkaç MB.
- **Yerel arayüz:** Sekmeler, butonlar ve adres çubuğu Win32 kontrolleri.
  Arayüz için ayrı bir web sayfası, yani ek bir renderer süreci yok.
- **Arka plan sekmeleri:** Görünmeyen sekmeler gizlenir ve WebView2'nin
  düşük bellek moduna alınır.

## Gereksinimler

- Windows 10 veya 11
- [Rust](https://rustup.rs) (MSVC araç zinciri, `rustup default stable-msvc`)
- Visual Studio Build Tools ("C++ ile masaüstü geliştirme" bileşeni)
- WebView2 Runtime (Windows 11'de ve güncel Windows 10'da zaten yüklü)

## Çalıştırma

```bash
cargo run
```

Hız ve bellek ölçümü yaparken her zaman sürüm derlemesini kullanın:

```bash
cargo run --release
```

## Kullanım

| İşlem | Nasıl |
|---|---|
| Adrese git / ara | Adres kutusuna yazıp Enter |
| Sayfaya dön | Adres kutusunda Esc |
| Yeni sekme | `+` butonu |
| Sekmeyi kapat | `✕` butonu (son sekme kapanınca tarayıcı kapanır) |
| Geri / İleri / Yenile | `←` `→` `⟳` butonları |

Adres kutusuna alan adı olmayan bir şey yazılırsa DuckDuckGo'da aranır.

## Proje yapısı

| Dosya | Görevi |
|---|---|
| `src/main.rs` | Giriş noktası ve olay türleri |
| `src/browser.rs` | Pencere, sekmeler, olay döngüsü |
| `src/toolbar.rs` | Win32 araç çubuğu ve sekme şeridi |
| `src/address.rs` | Adres çubuğu metnini URL'ye çevirme (testli) |

## Yol haritası

- [ ] Klavye kısayolları (Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+Tab)
- [ ] Dahili reklam ve izleyici engelleyici (WebView2 istek yakalama)
- [ ] Sekme uyutma: uzun süre kullanılmayan sekmeleri bellekten atma
- [ ] Bellek ve CPU için WebView2 başlangıç ayarları
- [ ] Oturumu kaydetme ve tembel geri yükleme
- [ ] Yükleniyor göstergesi, geri/ileri butonlarının durumu
