# Julie v0.1.0

İlk sürüm. Rust ve Windows'un yerleşik Chromium motoru WebView2 ile
yazılmış, hafif ve gizlilik odaklı bir tarayıcı.

**Gereksinimler:** Windows 10 veya 11 (WebView2 Runtime; Windows 11'de ve
güncel Windows 10'da zaten yüklü).

## Özellikler

- **Sekmeler** ve yerel (Win32) arayüz; arayüz için ayrı bir web süreci yok.
- **Gömülü başlangıç sayfası:** ağdan hiçbir şey indirmez, anında açılır.
- **Arama motoru seçimi:** Google, DuckDuckGo, Bing, Brave Search,
  Startpage, Ecosia, Yandex.
- **Reklam ve izleyici engelleyici:** EasyList, EasyPrivacy ve uBlock Origin
  listeleri; kozmetik filtreler; uBlock Origin betikleriyle **YouTube video
  reklamlarının** engellenmesi. Site başına açıp kapatma.
- **Sekme uyutma:** arka plandaki sekmeler 5 dakikada dondurulur,
  30 dakikada bellekten atılır. Ses çalan sekmelere dokunulmaz.
- **Bellek ayarları:** aynı sitenin sekmeleri tek süreci paylaşır,
  Chromecast taraması kapalı.
- **Zararlı site koruması:** Microsoft SmartScreen (varsayılan açık,
  başlangıç sayfasından kapatılabilir).
- **Klavye kısayolları:** Ctrl+T/W/Tab/L/Shift+T, F5, Alt+←/→, F12 ve diğerleri.

## Bilinen sınırlamalar

- Tarayıcı kapanınca sekmeler geri gelmez; geçmiş ve yer imleri yok.
- Geliştirici araçları (F12) ayrı pencerede açılır.
- Kod imzası yok: Windows ilk çalıştırmada "Bilinmeyen yayımcı" uyarısı
  verebilir.

## Lisans

GPL-3.0-or-later. uBlock Origin betikleri ve kaynakları GPL-3.0-or-later,
Raymond Hill ve katkıda bulunanlar.
