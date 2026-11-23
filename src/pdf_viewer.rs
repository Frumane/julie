//! Julie'nin PDF görüntüleyicisi (`julie://pdf`).
//!
//! Sayfalar hayro ile (Rust ile yazılmış PDF motoru) çizilir; görüntüleyicinin
//! ekranı `pdf_page.html`. Sekmede bir PDF açılınca motorun indirdiği dosya
//! yakalanır, burada açılır ve sekme görüntüleyiciye geçer. Görüntüleyici
//! sayfaları `http://julie.pdf/page?doc=..&n=..&s=..` adresinden PNG olarak
//! ister; çizim arka plandaki iş parçacıklarında yapılır.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use serde_json::{Value, json};

use crate::start_page::embed_state;

const TEMPLATE: &str = include_str!("pdf_page.html");

/// Görüntüleyicinin adresi (`julie://pdf`).
pub const VIEWER_URL: &str = "http://julie.pdf/";

/// Görüntüleyici sayfasının güvenlik kuralı: sayfa resimleri ve bilgiler
/// yalnızca görüntüleyicinin kendi adresinden yüklenir.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; \
     script-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; \
     frame-ancestors 'none'";

/// Bellekte tutulan en fazla belge sayısı (en eskisi atılır).
const MAX_DOCS: usize = 8;

/// Bir sayfanın çizileceği en büyük resim (piksel): çok büyük yakınlaştırmada
/// bellek dolmasın. vello'nun sınırı kenar başına 65 535 piksel.
const MAX_PIXELS: f32 = 36_000_000.0;
const MAX_SIDE: f32 = 16_000.0;

/// Açılmış bir PDF.
pub struct Doc {
    pub id: u64,
    /// PDF'in asıl adresi.
    pub src: String,
    /// Kaydederken kullanılan dosya adı (`.pdf` ile biter).
    pub name: String,
    /// Belgenin kendi başlığı (varsa).
    pub title: Option<String>,
    pdf: Pdf,
    /// Her sayfanın ekrandaki boyutu (punto; döndürme hesaba katılmış).
    sizes: Vec<(f32, f32)>,
}

/// PDF açılamadı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// Şifreli PDF (hayro henüz açamıyor).
    Encrypted,
    /// Bozuk ya da PDF değil.
    Invalid,
}

impl Doc {
    pub fn open(id: u64, src: &str, bytes: Vec<u8>) -> Result<Doc, OpenError> {
        let pdf = Pdf::new(bytes).map_err(|e| match e {
            LoadPdfError::Decryption(_) => OpenError::Encrypted,
            LoadPdfError::Invalid => OpenError::Invalid,
        })?;
        let sizes: Vec<_> = pdf.pages().iter().map(|p| p.render_dimensions()).collect();
        if sizes.is_empty() {
            return Err(OpenError::Invalid);
        }
        let title = pdf
            .metadata()
            .title
            .as_deref()
            .map(pdf_text)
            .filter(|t| !t.trim().is_empty());
        Ok(Doc {
            id,
            src: src.to_string(),
            name: file_name(src),
            title,
            pdf,
            sizes,
        })
    }

    #[cfg(test)]
    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    /// Dosyanın kendisi (kaydetmek için).
    pub fn bytes(&self) -> &[u8] {
        self.pdf.data().as_ref()
    }

    /// Görüntüleyicinin istediği bilgiler.
    pub fn info(&self) -> Value {
        json!({
            "doc": self.id,
            "name": self.name,
            "title": self.title,
            "pages": self.sizes.iter().map(|(w, h)| [w, h]).collect::<Vec<_>>(),
        })
    }

    /// `index`. sayfayı punto başına `scale` piksel olarak çizer; PNG döner.
    pub fn render_png(&self, index: usize, scale: f32) -> Option<Vec<u8>> {
        let (w, h) = *self.sizes.get(index)?;
        let scale = clamp_scale(w, h, scale);
        let page = self.pdf.pages().get(index)?;
        let cache = RenderCache::new();
        let pixmap = render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
            },
        );
        pixmap.into_png().ok()
    }
}

/// Çizim ölçeğini sınırlar: resim ne çok küçük ne de bellek dolduracak
/// kadar büyük olsun.
fn clamp_scale(w: f32, h: f32, scale: f32) -> f32 {
    let mut s = if scale.is_finite() { scale } else { 1.0 };
    s = s.clamp(0.1, 8.0);
    let (w, h) = (w.max(1.0), h.max(1.0));
    s = s.min(MAX_SIDE / w).min(MAX_SIDE / h);
    let pixels = w * h * s * s;
    if pixels > MAX_PIXELS {
        s *= (MAX_PIXELS / pixels).sqrt();
    }
    s
}

/// PDF metni: `FE FF` ile başlıyorsa UTF-16, değilse Latin-1'e yakın.
fn pdf_text(raw: &[u8]) -> String {
    if let Some(rest) = raw.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    raw.iter().map(|&b| b as char).collect()
}

/// Adresin son parçasından dosya adı; `.pdf` ile biter.
pub fn file_name(src: &str) -> String {
    let path = src.split(['?', '#']).next().unwrap_or_default();
    let last = path.rsplit(['/', '\\']).next().unwrap_or_default();
    let name = crate::history_page::percent_decode(last);
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_matches('.').to_string();
    let name = if name.is_empty() {
        "belge".to_string()
    } else {
        name
    };
    if name.to_ascii_lowercase().ends_with(".pdf") {
        name
    } else {
        format!("{name}.pdf")
    }
}

/// Açık belgeler; en yenisi sonda.
#[derive(Default)]
pub struct Docs {
    next: u64,
    list: VecDeque<Arc<Doc>>,
}

impl Docs {
    /// PDF'i açıp listeye ekler.
    pub fn add(&mut self, src: &str, bytes: Vec<u8>) -> Result<Arc<Doc>, OpenError> {
        self.next += 1;
        let doc = Arc::new(Doc::open(self.next, src, bytes)?);
        // Aynı adres yeniden açıldıysa eskisi gitsin.
        self.list.retain(|d| d.src != src);
        self.list.push_back(doc.clone());
        while self.list.len() > MAX_DOCS {
            self.list.pop_front();
        }
        Ok(doc)
    }

    pub fn get(&self, id: u64) -> Option<Arc<Doc>> {
        self.list.iter().find(|d| d.id == id).cloned()
    }
}

/// Görüntüleyicinin adresi; `doc` açık belgenin kimliği (yoksa görüntüleyici
/// belgeyi yeniden ister).
pub fn viewer_url(src: &str, doc: Option<u64>) -> String {
    let mut url = format!("{VIEWER_URL}?src={}", crate::address::encode_query(src));
    if let Some(doc) = doc {
        url.push_str(&format!("&doc={doc}"));
    }
    url
}

/// Görüntüleyici adresindeki asıl PDF adresi.
pub fn src_of(viewer: &str) -> Option<String> {
    let query = viewer.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or_default();
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("src="))
        .map(crate::history_page::percent_decode)
        .filter(|src| is_viewable_src(src))
}

/// Görüntüleyicide açılabilecek adresler: web ve yerel dosyalar.
pub fn is_viewable_src(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    ["http://", "https://", "file://"]
        .iter()
        .any(|s| lower.starts_with(s))
        && !crate::address::is_internal_page(src)
}

/// Yanıt sekmede gösterilecek bir PDF mi (indirme değil)?
pub fn is_pdf_response(content_type: &str, disposition: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    (mime == "application/pdf" || mime == "application/x-pdf")
        && !disposition
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("attachment")
}

/// Bilgisayardaki bir PDF mi (`file:///C:/.../x.pdf`)?
pub fn is_local_pdf(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or_default();
    lower.starts_with("file://") && path.ends_with(".pdf")
}

/// `file:///C:/a%20b.pdf` → `C:\a b.pdf`.
pub fn local_path(url: &str) -> Option<std::path::PathBuf> {
    let rest = url.strip_prefix("file:///")?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let path = crate::history_page::percent_decode(&rest.replace('+', "%2B"));
    (!path.is_empty()).then(|| std::path::PathBuf::from(path.replace('/', "\\")))
}

/// Görüntüleyici sayfası.
pub fn render_viewer(query: Option<&str>) -> String {
    let url = format!("{VIEWER_URL}?{}", query.unwrap_or_default());
    let src = src_of(&url).unwrap_or_default();
    let doc = query.and_then(|q| {
        q.split('&')
            .find_map(|p| p.strip_prefix("doc="))
            .and_then(|d| d.parse::<u64>().ok())
    });
    embed_state(
        TEMPLATE,
        &json!({ "src": src, "doc": doc, "name": file_name(&src) }),
    )
}

/// Bir sorgu parametresi.
pub fn param<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query?.split('&').find_map(|pair| {
        pair.strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

/// Görüntüleyicinin bir isteğine verilen yanıt.
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    fn ok(content_type: &'static str, body: Vec<u8>) -> Reply {
        Reply {
            status: 200,
            content_type,
            body,
        }
    }

    fn json(value: &Value) -> Reply {
        Reply::ok("application/json", value.to_string().into_bytes())
    }

    fn not_found() -> Reply {
        Reply {
            status: 404,
            content_type: "text/plain",
            body: Vec::new(),
        }
    }
}

/// Yerel PDF'lerin en büyük boyutu (bayt).
pub const MAX_FILE: usize = 512 * 1024 * 1024;

fn open_error(err: OpenError) -> Value {
    json!({ "error": match err {
        OpenError::Encrypted => "encrypted",
        OpenError::Invalid => "invalid",
    }})
}

/// `http://julie.pdf/...` isteğini yanıtlar. Uzun işler (dosya okuma,
/// sayfa çizme) arka planda yapılır; `done` yanıt hazır olunca çağrılır.
pub fn handle(
    path: &str,
    query: Option<&str>,
    docs: &Arc<Mutex<Docs>>,
    done: impl FnOnce(Reply) + Send + 'static,
) {
    let doc = || {
        param(query, "doc")
            .and_then(|d| d.parse::<u64>().ok())
            .and_then(|id| docs.lock().ok()?.get(id))
    };
    match path {
        "/" => done(Reply::ok(
            "text/html; charset=utf-8",
            render_viewer(query).into_bytes(),
        )),
        "/info" => {
            let src = param(query, "src")
                .map(crate::history_page::percent_decode)
                .unwrap_or_default();
            if let Some(doc) = doc().filter(|d| d.src == src) {
                return done(Reply::json(&doc.info()));
            }
            let Some(path) = is_local_pdf(&src).then(|| local_path(&src)).flatten() else {
                return done(Reply::json(&json!({ "missing": true })));
            };
            let docs = Arc::clone(docs);
            spawn(move || {
                let reply = match read_file(&path) {
                    Some(bytes) => match docs.lock().map(|mut d| d.add(&src, bytes)) {
                        Ok(Ok(doc)) => doc.info(),
                        Ok(Err(err)) => open_error(err),
                        Err(_) => open_error(OpenError::Invalid),
                    },
                    None => json!({ "error": "unreadable" }),
                };
                done(Reply::json(&reply));
            });
        }
        "/page" => {
            let Some(doc) = doc() else {
                return done(Reply::not_found());
            };
            let index = param(query, "n").and_then(|n| n.parse::<usize>().ok());
            let scale = param(query, "s").and_then(|s| s.parse::<f32>().ok());
            let (Some(index), Some(scale)) = (index, scale) else {
                return done(Reply::not_found());
            };
            spawn(move || {
                done(match doc.render_png(index, scale) {
                    Some(png) => Reply::ok("image/png", png),
                    None => Reply::not_found(),
                });
            });
        }
        "/file" => done(match doc() {
            Some(doc) => Reply::ok("application/pdf", doc.bytes().to_vec()),
            None => Reply::not_found(),
        }),
        _ => done(Reply::not_found()),
    }
}

fn read_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE as u64 {
        return None;
    }
    std::fs::read(path).ok()
}

type Job = Box<dyn FnOnce() + Send>;

/// Sayfaları arka planda çizen iş parçacıkları (işlemci çekirdeği kadar,
/// en çok 4). İşler sırayla dağıtılır.
pub fn spawn(job: impl FnOnce() + Send + 'static) {
    static WORKERS: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    let sender = WORKERS.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        let count = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 4);
        for i in 0..count {
            let rx = rx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("julie-pdf-{i}"))
                .spawn(move || {
                    loop {
                        let job = match rx.lock() {
                            Ok(rx) => rx.recv(),
                            Err(_) => return,
                        };
                        match job {
                            Ok(job) => job(),
                            Err(_) => return,
                        }
                    }
                });
        }
        Mutex::new(tx)
    });
    if let Ok(sender) = sender.lock() {
        let _ = sender.send(Box::new(job));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tek sayfalık, elle yazılmış en küçük PDF (A4, ortada bir kare).
    fn tiny_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>",
        ];
        let content = "0 0 1 rg 200 300 195 242 re f";
        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        offsets.push(out.len());
        out.push_str(&format!(
            "4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len()
        ));
        let xref = out.len();
        out.push_str("xref\n0 5\n0000000000 65535 f \n");
        for o in &offsets {
            out.push_str(&format!("{o:010} 00000 n \n"));
        }
        out.push_str(&format!(
            "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        out.into_bytes()
    }

    #[test]
    fn opens_and_renders() {
        let mut docs = Docs::default();
        let doc = docs
            .add(
                "https://ornek.example/belgeler/Rapor%202024.pdf?v=1",
                tiny_pdf(),
            )
            .unwrap();
        assert_eq!(doc.page_count(), 1);
        assert_eq!(doc.name, "Rapor 2024.pdf");
        assert_eq!(doc.info()["pages"][0], json!([595.0, 842.0]));
        let png = doc.render_png(0, 0.5).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert!(doc.render_png(1, 1.0).is_none());
        assert!(docs.get(doc.id).is_some());
        assert_eq!(doc.bytes(), tiny_pdf().as_slice());
        assert_eq!(
            docs.add("https://x.example/a.pdf", b"PDF degil".to_vec())
                .err(),
            Some(OpenError::Invalid)
        );
    }

    #[test]
    fn keeps_a_few_documents() {
        let mut docs = Docs::default();
        let first = docs.add("https://x.example/0.pdf", tiny_pdf()).unwrap();
        for i in 1..=MAX_DOCS {
            docs.add(&format!("https://x.example/{i}.pdf"), tiny_pdf())
                .unwrap();
        }
        assert!(docs.get(first.id).is_none());
        // Aynı adres yeniden açılınca eskisi gider.
        let a = docs.add("https://x.example/1.pdf", tiny_pdf()).unwrap();
        let b = docs.add("https://x.example/1.pdf", tiny_pdf()).unwrap();
        assert!(docs.get(a.id).is_none() && docs.get(b.id).is_some());
    }

    #[test]
    fn urls() {
        let src = "https://ornek.example/a b.pdf?x=1&y=2";
        let viewer = viewer_url(src, Some(7));
        assert!(viewer.starts_with(VIEWER_URL));
        assert!(viewer.ends_with("&doc=7"));
        assert_eq!(src_of(&viewer).as_deref(), Some(src));
        assert_eq!(crate::address::internal_page(&viewer), Some("pdf"));
        assert_eq!(src_of("http://julie.pdf/?src=javascript%3Aalert(1)"), None);
        assert_eq!(param(Some("doc=3&n=2"), "n"), Some("2"));
        assert_eq!(file_name("https://x.example/"), "belge.pdf");
        assert_eq!(file_name("https://x.example/indir?id=5"), "indir.pdf");
        assert_eq!(file_name("file:///C:/Users/a/ödev%3F.PDF"), "ödev_.PDF");
    }

    #[test]
    fn pdf_responses_and_local_files() {
        assert!(is_pdf_response("application/pdf", ""));
        assert!(is_pdf_response("Application/PDF; charset=binary", "inline"));
        assert!(!is_pdf_response(
            "application/pdf",
            "attachment; filename=a.pdf"
        ));
        assert!(!is_pdf_response("text/html", ""));
        assert!(is_local_pdf("file:///C:/Belgeler/Kitap.PDF"));
        assert!(!is_local_pdf("https://x.example/a.pdf"));
        assert_eq!(
            local_path("file:///C:/Belgeler/a%20b.pdf"),
            Some(std::path::PathBuf::from(r"C:\Belgeler\a b.pdf"))
        );
    }

    #[test]
    fn texts_and_scales() {
        assert_eq!(pdf_text(&[0xFE, 0xFF, 0x00, 0xD6, 0x00, 0x64]), "Öd");
        assert_eq!(pdf_text(b"Plain"), "Plain");
        assert_eq!(clamp_scale(600.0, 800.0, f32::NAN), 1.0);
        assert!(clamp_scale(600.0, 800.0, 100.0) <= 8.0);
        let s = clamp_scale(14_400.0, 14_400.0, 8.0);
        assert!(14_400.0 * s <= MAX_SIDE + 1.0);
        assert!(14_400.0 * 14_400.0 * s * s <= MAX_PIXELS * 1.01);
    }

    #[test]
    fn viewer_page_has_state() {
        let html = render_viewer(Some("src=https%3A%2F%2Fx.example%2Fa.pdf&doc=4"));
        assert!(html.contains(r#""doc":4"#));
        assert!(html.contains(r#""src":"https://x.example/a.pdf""#));
        assert!(!html.contains("/*STATE*/"));
    }

    #[test]
    fn answers_viewer_requests() {
        let docs = Arc::new(Mutex::new(Docs::default()));
        let doc = docs
            .lock()
            .unwrap()
            .add("https://a.com/x.pdf", tiny_pdf())
            .unwrap();
        let ask = |path: &str, query: &str| {
            let (tx, rx) = channel();
            handle(path, Some(query), &docs, move |r| tx.send(r).unwrap());
            rx.recv().unwrap()
        };
        let src = "src=https%3A%2F%2Fa.com%2Fx.pdf";
        let info = ask("/info", &format!("{src}&doc={}", doc.id));
        assert_eq!(info.content_type, "application/json");
        let value: Value = serde_json::from_slice(&info.body).unwrap();
        assert_eq!(value["doc"], doc.id);
        // Başka bir adres ya da bilinmeyen belge: yeniden yüklensin.
        let other = ask(
            "/info",
            &format!("src=https%3A%2F%2Fb.com%2Fy.pdf&doc={}", doc.id),
        );
        assert!(String::from_utf8(other.body).unwrap().contains("missing"));
        let page = ask("/page", &format!("doc={}&n=0&s=0.5", doc.id));
        assert_eq!(page.status, 200);
        assert!(page.body.starts_with(b"\x89PNG"));
        assert_eq!(ask("/page", &format!("doc={}&n=5&s=1", doc.id)).status, 404);
        assert_eq!(ask("/page", "doc=999&n=0&s=1").status, 404);
        let file = ask("/file", &format!("doc={}", doc.id));
        assert_eq!(file.body, tiny_pdf());
        assert_eq!(ask("/nope", "").status, 404);
        // Okunamayan yerel dosya: hata.
        let reply = ask("/info", "src=file%3A%2F%2F%2Fyok%2Fz.pdf");
        assert!(String::from_utf8(reply.body).unwrap().contains("error"));
    }
}
