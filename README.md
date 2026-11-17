# Julie

Hafif ve hızlı bir web tarayıcısı. Rust ile yazıldı ve Windows'un yerleşik
Chromium motoru **WebView2**'yi kullanıyor.

## Neden hafif?

- **Motoru biz taşımıyoruz:** WebView2 Windows 10/11'de zaten yüklü ve
  Microsoft tarafından güncel tutuluyor. Uygulama birkaç MB.
- **Yerel arayüz:** Sekmeler, butonlar ve adres çubuğu Win32 kontrolleri.
  Arayüz için ayrı bir web sayfası, yani ek bir renderer süreci yok.
- **Gömülü başlangıç sayfası:** Yeni sekme programın içinden gelir, ağdan
  hiçbir şey indirmez ve anında açılır.
- **Dahili reklam ve izleyici engelleyici:** Reklam ve izleme istekleri ağa
  hiç çıkmadan durdurulur; indirilmeyen betik CPU ve bellek de harcamaz.
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

| İşlem | Kısayol |
|---|---|
| Yeni sekme | `Ctrl+T` ya da `+` butonu |
| Sekmeyi kapat | `Ctrl+W`, `Ctrl+F4` ya da `✕` butonu (son sekme kapanınca tarayıcı kapanır) |
| Kapatılan sekmeyi geri aç | `Ctrl+Shift+T` |
| Sonraki / önceki sekme | `Ctrl+Tab` / `Ctrl+Shift+Tab` ya da `Ctrl+PgDn` / `Ctrl+PgUp` |
| 1.–8. sekmeye git | `Ctrl+1` … `Ctrl+8` |
| Son sekmeye git | `Ctrl+9` |
| Adres çubuğuna git | `Ctrl+L`, `Alt+D` ya da `F6` |
| Sayfaya dön | Adres kutusunda `Esc` |
| Yenile | `F5`, `Ctrl+R` ya da `⟳` |
| Geri / İleri | `Alt+←` / `Alt+→` ya da `←` `→` butonları |
| Yakınlaştır / uzaklaştır | `Ctrl` + `+` / `-`, `Ctrl` + fare tekerleği |
| Sayfada bul | `Ctrl+F` |

## Arama motoru

İlk açılışta başlangıç sayfası arama motoru seçtirir: Google, DuckDuckGo,
Bing, Brave Search, Startpage, Ecosia veya Yandex. Seçim daha sonra
başlangıç sayfasının altındaki menüden değiştirilebilir. Adres kutusuna alan
adı olmayan bir şey yazılırsa seçilen motorda aranır.

Ayarlar `%LOCALAPPDATA%\Julie\settings.ini` dosyasında, tarayıcı verileri
(çerezler, önbellek) `%LOCALAPPDATA%\Julie\WebView2` klasöründe tutulur.

## Reklam engelleyici

[EasyList](https://easylist.to) ve EasyPrivacy listelerini, Brave tarayıcısının
kullandığı [adblock-rust](https://github.com/brave/adblock-rust) motoruyla
uygular.

- İlk açılışta listeler arka planda indirilir (~3,5 MB), derlenip
  `%LOCALAPPDATA%\Julie\filters\engine.dat` dosyasına kaydedilir. Sonraki
  açılışlarda kurallar bu önbellekten ~5 ms'de yüklenir.
- Listeler 4 günden eskiyse arka planda yenilenir.
- Her istek ~2 µs'de kontrol edilir.
- Adres çubuğunun sağındaki buton o sayfada kaç isteğin engellendiğini
  gösterir. Butona tıklamak engelleyiciyi **o sitede** kapatır/açar (bozulan
  siteler için). Bu tercih ayarlara kaydedilir.

## Proje yapısı

| Dosya | Görevi |
|---|---|
| `src/main.rs` | Giriş noktası ve olay türleri |
| `src/browser.rs` | Pencere, sekmeler, olay döngüsü |
| `src/toolbar.rs` | Win32 araç çubuğu ve sekme şeridi |
| `src/address.rs` | Adres çubuğu metnini URL'ye çevirme (testli) |
| `src/blocking.rs` | Filtre listeleri, kural derleme ve önbellek (testli) |
| `src/request_filter.rs` | WebView2 isteklerini yakalayıp engelleyiciden geçirme |
| `src/shortcuts.rs` | Klavye kısayolu tablosu (testli) |
| `src/settings.rs` | Arama motorları ve ayar dosyası (testli) |
| `src/start_page.rs`, `src/start_page.html` | Gömülü başlangıç sayfası |

## Yol haritası

- [x] Klavye kısayolları
- [x] Dahili reklam ve izleyici engelleyici
- [ ] Görsel engelleme: reklamlardan kalan boş kutuları gizleme (kozmetik filtreler)
- [ ] Sekme uyutma: uzun süre kullanılmayan sekmeleri bellekten atma
- [ ] Bellek ve CPU için WebView2 başlangıç ayarları
- [ ] Oturumu kaydetme ve tembel geri yükleme
- [ ] Yükleniyor göstergesi, geri/ileri butonlarının durumu
