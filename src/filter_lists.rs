//! Filtre listeleri ve uBlock Origin listelerinin ön-işlemcisi.
//!
//! uBlock listeleri `!#if` (ortama göre koşullu kural) ve `!#include`
//! (başka dosyayı ekleme) yönergeleri kullanır. adblock-rust bunları
//! yorum satırı sayar; bu durumda yalnızca Firefox'a özel kurallar da
//! Chromium'da uygulanır ve siteleri bozabilir. Bu yüzden listeler motora
//! verilmeden önce burada işlenir.

/// Bir filtre listesi.
pub struct FilterList {
    pub name: &'static str,
    pub url: &'static str,
    /// uBlock'un kendi listeleri "güvenilir" sayılır: yalnızca bunlar
    /// sayfa içeriğini değiştirebilen `trusted-*` betiklerini kullanabilir.
    pub trusted: bool,
}

pub const LISTS: [FilterList; 7] = [
    FilterList {
        name: "EasyList",
        url: "https://easylist.to/easylist/easylist.txt",
        trusted: false,
    },
    FilterList {
        name: "EasyPrivacy",
        url: "https://easylist.to/easylist/easyprivacy.txt",
        trusted: false,
    },
    FilterList {
        name: "uBlock filters",
        url: "https://ublockorigin.github.io/uAssets/filters/filters.txt",
        trusted: true,
    },
    FilterList {
        name: "uBlock filters – Privacy",
        url: "https://ublockorigin.github.io/uAssets/filters/privacy.txt",
        trusted: true,
    },
    FilterList {
        name: "uBlock filters – Unbreak",
        url: "https://ublockorigin.github.io/uAssets/filters/unbreak.txt",
        trusted: true,
    },
    FilterList {
        name: "uBlock filters – Quick fixes",
        url: "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt",
        trusted: true,
    },
    FilterList {
        name: "uBlock filters – Badware risks",
        url: "https://ublockorigin.github.io/uAssets/filters/badware.txt",
        trusted: true,
    },
];

/// `!#if` koşullarında doğru kabul edilen ortam adları. Julie, Edge'in
/// motoru (Chromium) üzerinde çalışan bir masaüstü tarayıcısı; uBlock'un
/// Edge'de verdiği değerleri kullanıyoruz. Listede olmayan her ad yanlıştır
/// (`env_firefox`, `env_mobile`, `cap_html_filtering`, `adguard` ...).
const TRUE_TOKENS: [&str; 3] = ["env_chromium", "env_edge", "cap_user_stylesheet"];

/// `!#include` iç içe en fazla bu derinliğe kadar izlenir.
const MAX_INCLUDE_DEPTH: usize = 3;

/// Listeyi işler: koşulu yanlış olan blokları çıkarır, `!#include`
/// satırlarının yerine `include` ile alınan içeriği koyar.
pub fn preprocess(text: &str, include: &mut dyn FnMut(&str) -> Option<String>) -> String {
    preprocess_at(text, include, 0)
}

fn preprocess_at(
    text: &str,
    include: &mut dyn FnMut(&str) -> Option<String>,
    depth: usize,
) -> String {
    let mut out = String::with_capacity(text.len());
    // Her açık `!#if` için: (üst blok etkin mi, koşul doğru mu).
    let mut stack: Vec<(bool, bool)> = Vec::new();
    let active = |stack: &[(bool, bool)]| stack.last().is_none_or(|&(parent, cond)| parent && cond);

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(expr) = trimmed.strip_prefix("!#if ") {
            let parent = active(&stack);
            stack.push((parent, evaluate(expr)));
        } else if trimmed == "!#else" {
            if let Some(top) = stack.last_mut() {
                top.1 = !top.1;
            }
        } else if trimmed == "!#endif" {
            stack.pop();
        } else if let Some(name) = trimmed.strip_prefix("!#include ") {
            let name = name.trim();
            if active(&stack)
                && depth < MAX_INCLUDE_DEPTH
                && is_safe_include(name)
                && let Some(sub) = include(name)
            {
                out.push_str(&preprocess_at(&sub, include, depth + 1));
                out.push('\n');
            }
        } else if active(&stack) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Yalnızca aynı klasördeki düz dosya adları eklenebilir.
fn is_safe_include(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Liste adresine göre `!#include` edilen dosyanın adresi.
pub fn include_url(list_url: &str, name: &str) -> String {
    let base = list_url.rsplit_once('/').map_or(list_url, |(base, _)| base);
    format!("{base}/{name}")
}

/// `env_chromium && !env_mobile || (a && b)` gibi bir koşulu değerlendirir.
fn evaluate(expr: &str) -> bool {
    let tokens = tokenize(expr);
    let mut pos = 0;
    parse_or(&tokens, &mut pos)
}

#[derive(Debug, PartialEq)]
enum Token {
    Name(String),
    Not,
    And,
    Or,
    Open,
    Close,
}

fn tokenize(expr: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            '!' => {
                chars.next();
                tokens.push(Token::Not);
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '&' | '|' => {
                chars.next();
                if chars.peek() == Some(&c) {
                    chars.next();
                }
                tokens.push(if c == '&' { Token::And } else { Token::Or });
            }
            _ => {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    chars.next(); // tanınmayan karakteri atla
                } else {
                    tokens.push(Token::Name(name));
                }
            }
        }
    }
    tokens
}

fn parse_or(tokens: &[Token], pos: &mut usize) -> bool {
    let mut value = parse_and(tokens, pos);
    while tokens.get(*pos) == Some(&Token::Or) {
        *pos += 1;
        let rhs = parse_and(tokens, pos);
        value = value || rhs;
    }
    value
}

fn parse_and(tokens: &[Token], pos: &mut usize) -> bool {
    let mut value = parse_not(tokens, pos);
    while tokens.get(*pos) == Some(&Token::And) {
        *pos += 1;
        let rhs = parse_not(tokens, pos);
        value = value && rhs;
    }
    value
}

fn parse_not(tokens: &[Token], pos: &mut usize) -> bool {
    match tokens.get(*pos) {
        Some(Token::Not) => {
            *pos += 1;
            !parse_not(tokens, pos)
        }
        Some(Token::Open) => {
            *pos += 1;
            let value = parse_or(tokens, pos);
            if tokens.get(*pos) == Some(&Token::Close) {
                *pos += 1;
            }
            value
        }
        Some(Token::Name(name)) => {
            *pos += 1;
            TRUE_TOKENS.contains(&name.as_str())
        }
        _ => {
            *pos += 1;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> String {
        preprocess(text, &mut |name| match name {
            "part.txt" => Some("from-include\n!#include part.txt\n".to_string()),
            _ => None,
        })
    }

    #[test]
    fn evaluates_conditions() {
        assert!(evaluate("env_chromium"));
        assert!(!evaluate("env_firefox"));
        assert!(evaluate("!env_mobile"));
        assert!(evaluate("env_firefox || env_chromium"));
        assert!(!evaluate("env_chromium && env_mobile"));
        assert!(evaluate("!(env_firefox || env_safari) && env_edge"));
        assert!(!evaluate("false"));
        assert!(!evaluate("cap_html_filtering"));
    }

    #[test]
    fn drops_inactive_blocks() {
        let text = "\
a
!#if env_firefox
firefox-only
!#else
chromium
!#endif
!#if !env_mobile
desktop
!#if env_safari
nested-safari
!#endif
!#endif
b
";
        assert_eq!(run(text), "a\nchromium\ndesktop\nb\n");
    }

    #[test]
    fn resolves_includes_with_depth_limit() {
        let out = run("x\n!#include part.txt\ny\n");
        assert_eq!(out.matches("from-include").count(), MAX_INCLUDE_DEPTH);
        assert!(out.starts_with("x\n") && out.ends_with("y\n"));
        // Etkin olmayan bloktaki include izlenmez.
        assert_eq!(run("!#if env_firefox\n!#include part.txt\n!#endif\n"), "");
        // Güvensiz adlar izlenmez.
        assert_eq!(run("!#include ../secret.txt\n"), "");
    }

    #[test]
    fn builds_include_urls() {
        assert_eq!(
            include_url(
                "https://ublockorigin.github.io/uAssets/filters/filters.txt",
                "filters-2024.txt"
            ),
            "https://ublockorigin.github.io/uAssets/filters/filters-2024.txt"
        );
    }
}
