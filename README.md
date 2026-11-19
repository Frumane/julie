<img src="assets/logo-256.png" width="96" alt="Julie logosu">

# Julie

Hafif ve hızlı bir web tarayıcısı. Rust ile yazıldı ve Windows'un yerleşik
Chromium motoru **WebView2**'yi kullanıyor.

## Neden hafif?

- **Motoru biz taşımıyoruz:** WebView2 Windows 10/11'de zaten yüklü ve
  Microsoft tarafından güncel tutuluyor. Uygulama birkaç MB.
- **Direct2D arayüz:** Sekmeler, butonlar ve kenar çubuğu doğrudan ekran
  kartıyla çizilir. Arayüz için web sayfası, yani ek bir renderer süreci yok.
- **Gömülü başlangıç sayfası:** Yeni sekme programın içinden gelir, ağdan
  hiçbir şey indirmez ve anında açılır.
- **Dahili reklam ve izleyici engelleyici:** Reklam ve izleme istekleri ağa
  hiç çıkmadan durdurulur; indirilmeyen betik CPU ve bellek de harcamaz.
- **Sekme uyutma:** Arka plandaki sekmeler önce düşük bellek moduna alınır,
  5 dakika sonra dondurulur (CPU sıfır), 30 dakika sonra bellekten tamamen
  atılır. Ses/video çalan sekmelere dokunulmaz.

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
| Gizli pencere | `Ctrl+Shift+N` ya da başlangıç sayfasındaki bağlantı |
| Sayfada bul | `Ctrl+F`; sonraki/önceki `Enter`/`Shift+Enter` ya da `F3`/`Shift+F3`; kapat `Esc` |
| Sekmeyi kapat | `Ctrl+W`, `Ctrl+F4` ya da `✕` butonu (son sekme kapanınca yerine yeni sekme açılır; Ayarlar › Sekmeler'den "son sekmeyle tarayıcıyı da kapat" seçilebilir) |
| Kapatılan sekmeyi geri aç | `Ctrl+Shift+T` |
| Sonraki / önceki sekme | `Ctrl+Tab` / `Ctrl+Shift+Tab` ya da `Ctrl+PgDn` / `Ctrl+PgUp` |
| 1.–8. sekmeye git | `Ctrl+1` … `Ctrl+8` |
| Son sekmeye git | `Ctrl+9` |
| Adres çubuğuna git | `Ctrl+L`, `Alt+D` ya da `F6` |
| Sayfaya dön | Adres kutusunda `Esc` (öneri listesi açıksa önce onu kapatır) |
| Önerilerde gezin | Adres kutusunda `↑` / `↓`, sonra `Enter` |
| Yenile | `F5`, `Ctrl+R` ya da `⟳` |
| Geri / İleri | `Alt+←` / `Alt+→` ya da `←` `→` butonları |
| Yakınlaştır / uzaklaştır | `Ctrl` + `+` / `-`, `Ctrl` + fare tekerleği |
| Sayfada bul | `Ctrl+F` |
| Geliştirici araçları | `F12` ya da `Ctrl+Shift+I` |
| Yer imine ekle / çıkar | `Ctrl+D` ya da `☆` butonu |
| Geçmiş | `Ctrl+H` |
| Görünüm ve ayarlar | `Ctrl+,` |
| Sekmeyi kapat (fareyle) | Sekmede `✕` ya da orta tık |
| Sekmelerin sırasını değiştir | Sekmeyi basılı tutup sürükle |

## Görünüm

Arayüz Direct2D ve DirectWrite ile çizilir (`src/ui/`). Ayarlar sayfasından
(`Ctrl+,` ya da `julie://settings`) seçilebilir:

| Ayar | Seçenekler |
|---|---|
| Hazır temalar | Odak, Gece, Klasik, Minimal, Orman |
| Düzen | Kenar çubuğu (sekmeler solda), İnce şerit (ikonlar solda), Tek satır, Klasik |
| Renk modu | Açık, Koyu, Windows'a göre |
| Vurgu rengi | 6 renk ya da Windows'un vurgu rengi |
| Pencere tonu | Gri, Lavanta, Taş, Nane, Gökyüzü, Şeftali |
| Yoğunluk | Normal, Kompakt |

Hazır temalar: Odak (açık, lavanta), Gece (koyu, çivit), Klasik (açık,
nötr gri), Minimal (açık, taş, kompakt), Orman (koyu, yeşil). Varsayılan: Odak.

Butonlar Windows'un kendi ikon yazı tipiyle çizilir (Windows 11: Segoe
Fluent Icons, Windows 10: Segoe MDL2 Assets). Sekmelerde sitelerin kendi
ikonları görünür; ikonlar `places.sqlite`'ta saklandığı için uyuyan sekmeler
ve yer imleri de ikonlarıyla gösterilir. İkonu olmayan sitelerde küre ikonu
çıkar.
Başlık çubuğunun rengi temaya uyar; sayfalar ve sitelerin koyu tema
desteği seçilen renk moduna göre çalışır.

Yerleşim kodu platformdan bağımsızdır ve test edilir. Bütün düzenlerin SVG
önizlemelerini üretmek için:

```bash
JULIE_UI_PREVIEW=onizleme cargo test ui_previews -- --ignored
```

## Arama motoru

İlk açılışta başlangıç sayfası arama motoru seçtirir: Google, DuckDuckGo,
Bing, Brave Search, Startpage, Ecosia veya Yandex. Seçim daha sonra
Ayarlar › Arama'dan değiştirilebilir. Adres kutusuna alan
adı olmayan bir şey yazılırsa seçilen motorda aranır.

Ayarlar `%LOCALAPPDATA%\Julie\settings.ini` dosyasında, tarayıcı verileri
(çerezler, önbellek) `%LOCALAPPDATA%\Julie\WebView2` klasöründe tutulur.

## Bellek ve CPU ayarları

WebView2'ye başlangıçta verilen ayarlar (`src/browser.rs`, `BROWSER_ARGS`):

| Ayar | Etkisi |
|---|---|
| `--process-per-site` | Aynı sitenin sekmeleri tek süreci paylaşır; farklı siteler yine ayrı süreçlerde |
| `MediaRouter`, `DialMediaRouteProvider` kapalı | Ağda sürekli Chromecast arayan arka plan taraması durur |
| `AutofillServerCommunication` kapalı | Form bilgileri sunucuya gönderilmez (otomatik doldurma çalışır) |
| `SpareRendererForSitePerProcess` kapalı | Yeni sekme için boşta bekletilen yedek sayfa süreci açılmaz |
| `msWebOOUI`, `msPdfOOUI` kapalı | Edge'in metin seçince çıkan mini menüsü |

**Donanım hızlandırma** (Ayarlar → Sekmeler) varsayılan olarak açıktır;
sayfalar ekran kartıyla çizilir. Kapatılırsa `--disable-gpu` eklenir ve
Julie yeniden başlatılınca geçerli olur. Argümanlar aynı veri klasörünü
kullanan bütün pencerelerde aynı olmalı: ayar değiştikten sonra açılan
gizli pencere, WebView2 reddederse çalışan pencerenin ayarına uyar.

**Edge iç sayfaları kapalı:** `edge://`, `chrome://`, `devtools://` ve
`about:` (boş sayfa dışında) adresleri açılmaz; yerine "Bu sayfa Julie'de
açılmaz" sayfası gelir. Yalnızca bilgi veren `edge://gpu` (ekran kartı)
açıktır; `edge://version` WebView2'de zaten kapalı.

**Sinekkuşu oyunu:** İnternet yokken hata sayfasında Julie'nin sinekkuşuyla
çiçek sapları arasından uçulan küçük bir oyun çıkar (boşluk, yukarı ok ya da
tıklama). İnternet varken de `julie://fly` adresinden oynanır. Julie'nin ayarları `julie://settings`.

## Güvenlik

**Zararlı site koruması (Microsoft SmartScreen)** varsayılan olarak açıktır ve
Ayarlar › Gizlilik ve güvenlik'ten kapatılabilir. Açıkken ziyaret edilen adresler kontrol
için Microsoft'a gönderilir. Değişiklik açık sekmelere hemen uygulanır.

Aşağıdakiler LibreWolf, Brave ve Firefox'un katı kipinden örnek alındı.
Hepsi varsayılan olarak açık ve aynı bölmeden değiştirilebilir:

| Koruma | Ne yapar |
|---|---|
| **HTTPS önce** | `http://` adresler önce şifreli (`https://`) açılır. Site desteklemiyorsa ya da şifreli adres yeniden şifresize yönlendiriyorsa bir uyarı sayfası çıkar; "Yine de devam et" denirse o site bu oturum boyunca şifresiz açılır. `localhost`, IP adresleri, `.local`/`.lan` gibi yerel ağ adları değiştirilmez. |
| **Takip parametresi temizleme** | `utm_*`, `fbclid`, `gclid`, `msclkid`, `yclid`, `mc_eid`, `_hsenc` gibi yalnızca takip için kullanılan parametreler sayfa açılmadan adresten silinir. Site temizlenen adresi yeniden parametreli adrese yönlendirirse ikinci kez dokunulmaz. |
| **İzleme önleme** | Edge'in izleyici listesi; seviye Kapalı / Temel / Dengeli (varsayılan) / Katı. Reklam engelleyiciyle birlikte çalışır. |
| **WebRTC IP gizleme** | Her zaman açık (`--force-webrtc-ip-handling-policy=default_public_interface_only`): görüntülü görüşme kullanan siteler yerel ağ adresini göremez. Ayar değildir, çünkü değişirse açık gizli pencereyle tarayıcı süreci argümanları uyuşmaz. |

### Kilit ve site bilgisi

Adres çubuğunun solundaki simge bağlantıyı gösterir: kilit (şifreli),
uyarı (şifresiz) ya da kırmızı uyarı (geçersiz sertifikaya rağmen devam
edildi). Simgeye tıklayınca açılan panelde bağlantının durumu ve sitenin
izinleri (kamera, mikrofon, konum, bildirimler; site başka izin kaydettiyse
onlar da) görünür. Satıra tıklamak izni **Sor → İzin verildi → Engellendi**
arasında değiştirir; sayfa yenilenince geçerli olur. Panel dışarı tıklayınca
ya da Esc ile kapanır.

### İzin soruları

Site kamera, mikrofon, konum, bildirim gibi bir izin isteyince Edge'in
kutusu yerine kilidin altında temaya uyan bir soru çıkar: **İzin ver** ya da
**Engelle**. Karar hatırlanır ve kilide tıklayınca açılan panelden değiştirilebilir. ✕ ile
kapatmak yalnızca bu seferlik engeller. Soru sayfayı kilitlemez; başka
siteye gidilince ya da sekme kapanınca düşer. Başka sekmeden gelen soru o
sekmeye dönünce görünür; sorusu bekleyen sekme uyutulmaz.

### Hata sayfaları

Sayfa açılamayınca WebView2'nin kendi sayfası yerine temaya uyan Türkçe bir
sayfa çıkar:

| Durum | Sayfa |
|---|---|
| Geçersiz sertifika (süresi dolmuş, başka siteye ait, iptal edilmiş, güvenilmeyen) | "Bağlantın güvenli değil". "Güvenli sayfaya dön" öne çıkar; "Gelişmiş" altında "Yine de devam et" o siteye bu oturum boyunca izin verir. |
| Site bulunamadı (DNS) | "Bu site bulunamadı" |
| İnternet yok | "İnternet bağlantısı yok" |
| Zaman aşımı, bağlantı reddedildi/kesildi | "Site yanıt vermedi" / "Siteye bağlanılamadı" |

Hata sayfasında adres çubuğu asıl adresi gösterir; yenilemek ya da
"Yeniden dene" sayfayı yeniden dener, "Geri dön" açılamayan sayfayı da atlar.
Şifreliye çevrilen bir adres hiç yoksa (DNS) şifresiz uyarısı yerine "site
bulunamadı" sayfası çıkar.

## Geçmiş ve yer imleri

Geçmiş ve yer imleri `%LOCALAPPDATA%\Julie\places.sqlite` dosyasında
(SQLite) tutulur.

- **Geçmiş** (`Ctrl+H` ya da `julie://history`): günlere göre gruplanmış
  liste, arama, tek tek silme ve "Geçmişi temizle". 90 günden eski kayıtlar
  açılışta silinir. Tek sayfalık sitelerde (YouTube'da video değiştirmek
  gibi) yeniden yükleme olmadan yapılan geçişler de kaydedilir.
- **Yer imleri** (`Ctrl+D` ya da adres çubuğunun yanındaki yıldız):
  başlangıç sayfasında kutucuklar olarak görünür, oradan silinebilir.
- **Adres çubuğu önerileri**: yazarken altında bir liste açılır (başlangıç
  sayfasının arama kutusunda da). İlk satır Enter'a basınca olacak şeydir
  (siteye git ya da seçili motorda ara); altında geçmişten ve yer
  imlerinden eşleşen siteler, sonra arama motorunun önerileri gelir.
  Yazılanla başlayan alan adları ("you" → youtube.com), sık ve yakın zamanda
  ziyaret edilenler ile yer imleri öne çıkar. `↑`/`↓` ile seçilir, Enter ya
  da tıklamayla açılır, `Esc` listeyi kapatır.
- **Otomatik tamamlama**: daha önce girilen bir sitenin başı yazılınca adres
  kutusu gerisini tamamlar ("you" → "you**tube.com**", eklenen kısım seçili).
  Yalnızca yerel geçmiş kullanılır.
- **Arama önerileri** seçili arama motorundan alınır; bunun için yazılan
  metin motora gönderilir. Site adresi gibi görünen metinler gönderilmez.
  İkisi de Ayarlar › Arama'dan kapatılabilir.

## Sayfada bul

`Ctrl+F` sayfanın bir köşesinde küçük bir bulma kutusu açar: yazdıkça
bütün eşleşmeler vurgulanır, "3/12" sayacı, önceki/sonraki ve büyük-küçük
harf (`Aa`) düğmeleri vardır. Kutu her düzende aynıdır; renkleri seçili
temadan gelir, yeri Ayarlar › Görünüm › Bulma kutusu'ndan seçilir (sağ üst,
sol üst, sağ alt, sol alt). WebView2'nin bulma API'si olmayan eski
sürümlerde WebView2'nin kendi bulma kutusu açılır.

## Gizli pencere

`Ctrl+Shift+N` ayrı bir gizli pencere açar (aynı programın `--private`
argümanıyla başlatılmış ikinci bir kopyası). Bu pencerede:

- Sayfalar WebView2'nin InPrivate modunda açılır: çerezler, önbellek ve site
  verileri bellekte tutulur, pencere kapanınca silinir.
- Geçmişe, site ikonlarına ve oturum dosyasına hiçbir şey yazılmaz;
  açılışta eski sekmeler gelmez.
- Yazılanlar öneri için arama motoruna gönderilmez (geçmişten öneriler
  ve otomatik tamamlama çalışır).
- Pencere her zaman koyu görünür, başlığında "Julie (Gizli)" yazar.
- Eklenen yer imleri ve ayar değişiklikleri kalıcıdır.

## Sıkı gizlilik

Ayarlar → Gizlilik → "Sıkı gizlilik" bölümünde iki seçenek var, ikisi de
kapalı başlar:

- **Kapanınca çerezleri ve site verilerini sil:** Julie kapanırken
  çerezler, yerel depolama, IndexedDB ve önbellek depoları silinir; bütün
  sitelerden çıkış yapılır. Geçmiş, yer imleri, sekmeler ve ayarlar kalır.
  Program Görev Yöneticisi'nden sonlandırılırsa silme çalışamaz.
- **Saat dilimini gizle:** Sayfalar saat dilimini UTC olarak görür
  (DevTools `Emulation.setTimezoneOverride`). Saat dilimi, parmak izi
  çıkarmada sık kullanılan bir bilgidir. Yeni sekmelerde ilk sayfadan
  itibaren geçerlidir; açık sayfalar yenileyince görür.

Dil ve tuval (canvas) gizleme bilerek yok: dil değiştirmek siteleri
bozuyor, tuval ayarı ise gizli pencereyle aynı anda açılmayı engelliyor.

## Oturum

Tarayıcı kapanırken açık sekmeler `%LOCALAPPDATA%\Julie\session.txt`
dosyasına kaydedilir ve sonraki açılışta geri gelir. Yalnızca öndeki sekme
hemen yüklenir; diğerleri tıklanınca yüklenir, böylece çok sekmeyle bile
açılış hızlı olur. Oturum çalışırken de periyodik olarak kaydedilir
(beklenmedik kapanmada en fazla son 30 saniye kaybolur).

## Sekme uyutma

| Arka planda geçen süre | Ne olur |
|---|---|
| Hemen | Sekme gizlenir, WebView2 düşük bellek moduna alınır |
| 5 dakika | Sayfa dondurulur: betikler ve zamanlayıcılar durur, CPU kullanımı sıfıra iner. Geri dönünce anında açılır. |
| 30 dakika | Sayfa bellekten atılır, yalnızca adresi tutulur. Sekmeye tıklanınca yeniden yüklenir. |

Süre Ayarlar › Sekmeler › Sekme uyutma'dan seçilir:

| Seçenek | Dondurma | Bellekten atma |
|---|---|---|
| 1 dakika | 1 dk | 10 dk |
| 5 dakika (varsayılan) | 5 dk | 30 dk |
| 15 dakika | 15 dk | 60 dk |
| Hiç | — | — |

Başlangıç sayfası dondurma süresinde doğrudan bellekten atılır. Ses ya da video
çalan sekmeler hiç uyutulmaz.

Denemek için süreleri kısaltın (20 saniye / 1 dakika):

```powershell
$env:JULIE_FAST_SLEEP=1; cargo run --release
```

## Reklam engelleyici

Brave tarayıcısının kullandığı [adblock-rust](https://github.com/brave/adblock-rust)
motoruyla şu listeleri uygular:

| Liste | İçerik |
|---|---|
| [EasyList](https://easylist.to) | Reklamlar |
| EasyPrivacy | İzleyiciler |
| [uBlock filters](https://github.com/uBlockOrigin/uAssets) | uBlock Origin'in kendi kuralları (YouTube dahil) |
| uBlock filters – Privacy | Ek izleyici kuralları |
| uBlock filters – Unbreak | Engelleme yüzünden bozulan siteler için istisnalar |
| uBlock filters – Quick fixes | Hızlı düzeltmeler (YouTube kuralları sık güncellenir) |
| uBlock filters – Badware risks | Dolandırıcılık ve zararlı yazılım siteleri |

- İlk açılışta listeler arka planda indirilir (~6,5 MB), derlenip
  `%LOCALAPPDATA%\Julie\filters\engine-v2.dat` dosyasına kaydedilir.
  Sonraki açılışlarda kurallar bu önbellekten ~10 ms'de yüklenir.
- uBlock listelerindeki `!#if` / `!#include` yönergeleri işlenir
  (`src/filter_lists.rs`): yalnızca Chromium için geçerli kurallar uygulanır.
- Listeler 4 günden eskiyse arka planda yenilenir.
- Her istek ~2 µs'de kontrol edilir.
- **Betikler (scriptlet):** uBlock Origin'in betikleri sayfanın kendi
  kodundan önce enjekte edilir. YouTube'un video reklamları bu yolla
  engellenir: oynatıcıya giden veriden reklam alanları silinir.
- **Yönlendirmeler:** Bazı siteler izleme betiği hiç gelmezse bozulur. Bu
  durumda istek engellenir ama yerine uBlock'un zararsız sahte sürümü verilir
  (ör. sahte Google Analytics).
- **Kozmetik filtreler:** Engellenen reklamlardan kalan boş kutular da
  gizlenir. Sayfadaki küçük bir betik sınıf/kimlik adlarını toplar, tarayıcı
  yalnızca sayfada gerçekten bulunan adlara uyan gizleme kurallarını geri
  gönderir; sonradan eklenen reklamlar da yakalanır.
- Adres çubuğunun sağındaki buton o sayfada kaç isteğin engellendiğini
  gösterir. Butona tıklamak engelleyiciyi **o sitede** kapatır/açar (bozulan
  siteler için). Bu tercih ayarlara kaydedilir.
- Listeler indirilemezse buton "Liste indirilemedi" der; hatanın ayrıntısı
  `%LOCALAPPDATA%\Julie\julie.log` dosyasına yazılır.

## Logo

Üçgen yüzeylerden kurulmuş, origami tarzı bir sinek kuşu: küçük, hafif ve hızlı.
Sinek kuşu saniyede onlarca kez kanat çırpar; az kaynakla hızlı çalışan bir tarayıcıyı anlatır.

| Dosya | Görevi |
|---|---|
| `assets/logo.svg` | Logonun kaynağı (48 piksel ve üstü) |
| `assets/logo-small.svg` | Küçük boyutlar için sadeleştirilmiş çizim (16–32 piksel) |
| `assets/julie.ico` | Uygulama ikonu (16–256 piksel; 16–32 küçük çizimden, 40 ve üstü asıl logodan); `build.rs` ile `julie.exe`'ye gömülür |
| `assets/logo-256.png` | Belgeler ve web için |

## Proje yapısı

| Dosya | Görevi |
|---|---|
| `src/main.rs` | Giriş noktası ve olay türleri |
| `src/browser.rs` | Pencere, sekmeler, olay döngüsü |
| `src/chrome.rs` | Arayüzün Windows tarafı: çizim, fare, adres kutusu, başlık çubuğu |
| `src/ui/theme.rs` | Görünüm ayarları, hazır temalar, renkler ve ölçüler (testli) |
| `src/ui/layout.rs` | Dört düzenin yerleşimi ve tıklama alanları (testli) |
| `src/ui/scene.rs` | Çizim listesi ve vektör ikonlar (testli) |
| `src/ui/d2d.rs` | Çizim listesini Direct2D/DirectWrite ile çizme |
| `src/ui/svg.rs` | Testler ve önizlemeler için SVG çizici |
| `src/settings_page.rs`, `src/settings_page.html` | Görünüm ayarları sayfası (testli) |
| `src/address.rs` | Adres çubuğu metnini URL'ye çevirme (testli) |
| `src/suggest.rs` | Öneri satırları, otomatik tamamlama, motor cevabını okuma (testli) |
| `src/search_suggest.rs` | Arama motorundan arka planda öneri alma |
| `src/blocking.rs` | Filtre listeleri, kural derleme ve önbellek (testli) |
| `src/cosmetic.rs`, `src/cosmetic.js` | Kozmetik filtreler: sayfa betiği ve mesajlar (testli) |
| `src/filter_lists.rs` | Liste tanımları, `!#if`/`!#include` ön-işlemcisi (testli) |
| `src/scriptlet_injector.rs` | uBlock betiklerini sayfa kodundan önce enjekte etme |
| `resources/ubo-resources.json` | uBlock Origin betikleri ve yönlendirme kaynakları |
| `tools/build-ubo-resources.mjs` | Yukarıdaki dosyayı uBlock kaynak kodundan üretir |
| `src/request_filter.rs` | WebView2 isteklerini yakalayıp engelleyiciden geçirme |
| `src/places.rs` | Geçmiş ve yer imleri veritabanı (SQLite, testli) |
| `src/history_page.rs`, `src/history_page.html` | Geçmiş sayfası (testli) |
| `src/session.rs` | Oturum dosyası (testli) |
| `src/sleep.rs` | Sekme uyutma politikası (testli) |
| `src/shortcuts.rs` | Klavye kısayolu tablosu (testli) |
| `src/settings.rs` | Arama motorları ve ayar dosyası (testli) |
| `tools/olc.ps1` | Julie ve Floorp'un bellek/işlemci ölçümü (`-Detay` ile süreç süreç) |
| `src/privacy.rs` | HTTPS önce, takip parametresi temizleme, izleme önleme seviyeleri (testli) |
| `src/site_info.rs` | Kilit durumu, site paneli izin satırları (testli) |
| `src/error_page.rs`, `src/error_page.html` | Sertifika ve bağlantı hata sayfaları (testli) |
| `src/cert_page.rs`, `src/cert_page.html` | Sertifika görüntüleyici `julie://certificate` (testli) |
| `src/devtools_window.rs` | Geliştirici araçları penceresine Julie'nin adı ve simgesi, yerleşimi (testli) |
| `src/start_page.rs`, `src/start_page.html` | Gömülü başlangıç sayfası |

## Yol haritası

- [x] Klavye kısayolları
- [x] Dahili reklam ve izleyici engelleyici
- [x] Kozmetik filtreler
- [x] YouTube video reklamları (uBlock Origin betikleri)
- [x] Sekme uyutma
- [x] Bellek ve CPU için WebView2 başlangıç ayarları
- [x] SmartScreen tercihi
- [x] HTTPS önce, takip parametresi temizleme, izleme önleme seviyesi, WebRTC IP gizleme
- [x] Adres çubuğunda kilit, site bilgisi ve izinler; temaya uygun hata sayfaları
- [x] Temaya uygun izin soruları
- [x] Sıkı gizlilik: kapanınca site verilerini silme, saat dilimini gizleme
- [x] Geliştirici araçları (F12, ayrı pencerede)
- [x] Oturumu kaydetme ve tembel geri yükleme
- [x] Geçmiş ve yer imleri
- [x] Adres çubuğu önerileri (geçmiş ve yer imlerinden)
- [ ] Yükleniyor göstergesi, geri/ileri butonlarının durumu
- [ ] Gizli pencere / ayrı profil
- [ ] Google Safe Browsing (SmartScreen yerine, gizlilik dostu)
- [x] Direct2D arayüz, 4 düzen ve tema seçenekleri
- [ ] Özel başlık çubuğu (pencere butonlarını arayüze katmak), Mica
- [x] Site ikonları (favicon)
- [ ] Sekmeleri sürükleyerek sıralama
- [ ] Tema dosyası yükleme/dışa aktarma
- [ ] Geliştirici araçlarını Julie'nin penceresine gömmek (Chrome gibi sayfanın yanında). Şimdilik ayrı pencere, Julie'nin adı ve simgesiyle.
- [ ] Kurulum programı, otomatik güncelleme, kod imzası

## Lisans

Julie [GPL-3.0-or-later](LICENSE) lisanslı özgür bir yazılımdır.

Kullanılan üçüncü taraf bileşenler:

- [uBlock Origin](https://github.com/gorhill/uBlock) betikleri ve kaynakları
  (`resources/ubo-resources.json`), GPL-3.0-or-later, Raymond Hill ve
  katkıda bulunanlar.
- [adblock-rust](https://github.com/brave/adblock-rust), MPL-2.0, Brave.
- Filtre listeleri çalışma anında indirilir ve programa gömülmez:
  EasyList/EasyPrivacy (GPLv3 / CC BY-SA 3.0), uBlock filters (GPLv3).
