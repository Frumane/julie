//! Sertifika görüntüleyici (`julie://certificate`). Firefox'un
//! `about:certificate` sayfası gibi: zincirdeki her sertifika ayrı bir
//! sekmede, bölüm bölüm. Sertifikalar adresin içinde taşınır
//! (`?cert=...&cert=...`), böylece sayfa hiçbir yerde saklanmaz.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use x509_parser::extensions::{DistributionPointName, GeneralName, ParsedExtension};
use x509_parser::prelude::{FromDer, X509Certificate, X509Name};
use x509_parser::public_key::PublicKey;

/// Sayfanın adresi; zincir yaprak sertifikadan köke doğru PEM metinleri.
pub fn url(chain: &[String]) -> String {
    let certs: Vec<String> = chain
        .iter()
        .filter_map(|pem| der_of(pem))
        .map(|der| format!("cert={}", URL_SAFE_NO_PAD.encode(der)))
        .collect();
    format!("http://julie.certificate/?{}", certs.join("&"))
}

/// PEM metninden sertifikanın ikili hâli.
fn der_of(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .map(str::trim)
        .collect();
    STANDARD.decode(body).ok().filter(|d| !d.is_empty())
}

/// İkili hâlden PEM metni (indirme bağlantısı için).
fn pem_of(der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    let lines: Vec<&str> = body
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect();
    format!(
        "-----BEGIN CERTIFICATE-----\r\n{}\r\n-----END CERTIFICATE-----\r\n",
        lines.join("\r\n")
    )
}

/// Sayfanın HTML'i. `query`, adresin `?` sonrası.
pub fn render(query: Option<&str>) -> String {
    let ders: Vec<Vec<u8>> = query
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.strip_prefix("cert="))
        .take(10)
        .filter_map(|b64| URL_SAFE_NO_PAD.decode(b64).ok())
        .collect();
    let certs: Vec<Value> = ders.iter().filter_map(|der| describe(der)).collect();
    let chain: String = ders.iter().map(|d| pem_of(d)).collect();
    crate::start_page::embed_state(
        include_str!("cert_page.html"),
        &json!({ "certs": certs, "chain": chain }),
    )
}

/// Bölümdeki bir satır: ad, değer; `mono` uzun onaltılı değerler için,
/// `time` tarih (Unix saniyesi; sayfa yerel saate çevirir).
fn row(name: &str, value: impl Into<String>) -> Value {
    json!({ "k": name, "v": value.into() })
}

fn mono(name: &str, value: impl Into<String>) -> Value {
    json!({ "k": name, "v": value.into(), "mono": true })
}

fn time(name: &str, secs: i64) -> Value {
    json!({ "k": name, "time": secs })
}

fn section(title: &str, rows: Vec<Value>) -> Option<Value> {
    (!rows.is_empty()).then(|| json!({ "title": title, "rows": rows }))
}

/// `AB:CD:...` biçiminde onaltılı.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Bir sertifikanın sekmesi: etiket, bölümler ve PEM.
fn describe(der: &[u8]) -> Option<Value> {
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let mut sections = Vec::new();
    sections.push(section("Konu adı", name_rows(cert.subject())));
    sections.push(section("Veren adı", name_rows(cert.issuer())));
    let validity = cert.validity();
    sections.push(section(
        "Geçerlilik",
        vec![
            time("Başlangıç", validity.not_before.timestamp()),
            time("Bitiş", validity.not_after.timestamp()),
        ],
    ));

    let mut alt_names = Vec::new();
    let mut extensions = Vec::new();
    for ext in cert.iter_extensions() {
        match ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => {
                alt_names.extend(san.general_names.iter().filter_map(general_name));
            }
            parsed => extensions.extend(extension(parsed)),
        }
    }
    sections.push(section("Konu alternatif adları", alt_names));
    sections.push(section("Genel anahtar bilgisi", key_rows(&cert)));

    let mut misc = vec![
        mono("Seri numarası", hex(cert.raw_serial())),
        row(
            "İmza algoritması",
            signature_name(&cert.signature_algorithm.algorithm.to_id_string()),
        ),
        row("Sürüm", (cert.version().0 + 1).to_string()),
    ];
    misc.push(json!({ "k": "İndir", "download": true }));
    sections.push(section("Çeşitli", misc));
    sections.push(section(
        "Parmak izleri",
        vec![
            mono("SHA-256", hex(&Sha256::digest(der))),
            mono("SHA-1", hex(&Sha1::digest(der))),
        ],
    ));
    sections.extend(extensions.into_iter().map(Some));

    let label = cert
        .subject()
        .iter_common_name()
        .chain(cert.subject().iter_organization())
        .find_map(|a| a.as_str().ok())
        .unwrap_or("Sertifika")
        .to_string();
    Some(json!({
        "label": label,
        "sections": sections.into_iter().flatten().collect::<Vec<_>>(),
        "pem": pem_of(der),
    }))
}

/// Ad alanları (Ülke, Kuruluş, Ortak ad ...).
fn name_rows(name: &X509Name) -> Vec<Value> {
    name.iter_attributes()
        .map(|attr| {
            let oid = attr.attr_type().to_id_string();
            let label = match oid.as_str() {
                "2.5.4.6" => "Ülke",
                "2.5.4.8" => "İl",
                "2.5.4.7" => "Şehir",
                "2.5.4.9" => "Adres",
                "2.5.4.17" => "Posta kodu",
                "2.5.4.10" => "Kuruluş",
                "2.5.4.11" => "Birim",
                "2.5.4.3" => "Ortak ad",
                "2.5.4.5" => "Seri numarası",
                "2.5.4.15" => "İş kategorisi",
                "2.5.4.97" => "Kuruluş kimliği",
                "1.2.840.113549.1.9.1" => "E-posta",
                "1.3.6.1.4.1.311.60.2.1.3" => "Kuruluş ülkesi",
                "1.3.6.1.4.1.311.60.2.1.2" => "Kuruluş ili",
                _ => "",
            };
            let value = attr.as_str().map(str::to_string).unwrap_or_default();
            if label.is_empty() {
                row(&oid, value)
            } else {
                row(label, value)
            }
        })
        .collect()
}

fn general_name(name: &GeneralName) -> Option<Value> {
    Some(match name {
        GeneralName::DNSName(dns) => row("DNS adı", *dns),
        GeneralName::RFC822Name(mail) => row("E-posta", *mail),
        GeneralName::URI(uri) => row("Adres", *uri),
        GeneralName::IPAddress(ip) => row("IP adresi", ip_text(ip)),
        _ => return None,
    })
}

fn ip_text(bytes: &[u8]) -> String {
    match bytes.len() {
        4 => std::net::Ipv4Addr::from(<[u8; 4]>::try_from(bytes).unwrap_or_default()).to_string(),
        16 => std::net::Ipv6Addr::from(<[u8; 16]>::try_from(bytes).unwrap_or_default()).to_string(),
        _ => hex(bytes),
    }
}

fn key_rows(cert: &X509Certificate) -> Vec<Value> {
    let info = cert.public_key();
    let oid = info.algorithm.algorithm.to_id_string();
    let mut rows = vec![row(
        "Algoritma",
        match oid.as_str() {
            "1.2.840.113549.1.1.1" => "RSA".to_string(),
            "1.2.840.10045.2.1" => "Eliptik eğri".to_string(),
            "1.3.101.112" => "Ed25519".to_string(),
            "1.3.101.113" => "Ed448".to_string(),
            _ => oid.clone(),
        },
    )];
    let parsed = info.parsed().ok();
    if let Some(size) = parsed.as_ref().map(PublicKey::key_size).filter(|s| *s > 0) {
        rows.push(row("Anahtar boyutu", format!("{size} bit")));
    }
    match parsed {
        Some(PublicKey::RSA(rsa)) => {
            if let Ok(exponent) = rsa.try_exponent() {
                rows.push(row("Üs", exponent.to_string()));
            }
            let modulus = rsa.modulus.strip_prefix(&[0]).unwrap_or(rsa.modulus);
            rows.push(mono("Modulus", hex(modulus)));
        }
        Some(PublicKey::EC(point)) => {
            let curve = info
                .algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.as_oid().ok())
                .map(|o| o.to_id_string());
            if let Some(curve) = curve {
                let name = match curve.as_str() {
                    "1.2.840.10045.3.1.7" => "P-256".to_string(),
                    "1.3.132.0.34" => "P-384".to_string(),
                    "1.3.132.0.35" => "P-521".to_string(),
                    _ => curve,
                };
                rows.push(row("Eğri", name));
            }
            rows.push(mono("Genel değer", hex(point.data())));
        }
        _ => rows.push(mono("Genel değer", hex(&info.subject_public_key.data))),
    }
    rows
}

fn signature_name(oid: &str) -> String {
    match oid {
        "1.2.840.113549.1.1.5" => "SHA-1 ile RSA",
        "1.2.840.113549.1.1.11" => "SHA-256 ile RSA",
        "1.2.840.113549.1.1.12" => "SHA-384 ile RSA",
        "1.2.840.113549.1.1.13" => "SHA-512 ile RSA",
        "1.2.840.113549.1.1.10" => "RSA-PSS",
        "1.2.840.10045.4.3.2" => "SHA-256 ile ECDSA",
        "1.2.840.10045.4.3.3" => "SHA-384 ile ECDSA",
        "1.2.840.10045.4.3.4" => "SHA-512 ile ECDSA",
        "1.3.101.112" => "Ed25519",
        _ => return oid.to_string(),
    }
    .to_string()
}

/// Uzantılar: her biri ayrı bölüm.
fn extension(parsed: &ParsedExtension) -> Option<Value> {
    match parsed {
        ParsedExtension::BasicConstraints(bc) => {
            let mut rows = vec![row(
                "Sertifika yetkilisi",
                if bc.ca { "Evet" } else { "Hayır" },
            )];
            if let Some(len) = bc.path_len_constraint {
                rows.push(row("En fazla ara sertifika", len.to_string()));
            }
            section("Temel kısıtlamalar", rows)
        }
        ParsedExtension::KeyUsage(ku) => {
            let uses = [
                (ku.digital_signature(), "Dijital imza"),
                (ku.non_repudiation(), "İnkâr edilemezlik"),
                (ku.key_encipherment(), "Anahtar şifreleme"),
                (ku.data_encipherment(), "Veri şifreleme"),
                (ku.key_agreement(), "Anahtar anlaşması"),
                (ku.key_cert_sign(), "Sertifika imzalama"),
                (ku.crl_sign(), "İptal listesi imzalama"),
                (ku.encipher_only(), "Yalnızca şifreleme"),
                (ku.decipher_only(), "Yalnızca şifre çözme"),
            ];
            let list: Vec<&str> = uses.iter().filter(|u| u.0).map(|u| u.1).collect();
            section(
                "Anahtar kullanımları",
                vec![row("Amaçlar", list.join(", "))],
            )
        }
        ParsedExtension::ExtendedKeyUsage(eku) => {
            let mut list: Vec<String> = [
                (eku.any, "Her amaç"),
                (eku.server_auth, "Sunucu kimlik doğrulaması"),
                (eku.client_auth, "İstemci kimlik doğrulaması"),
                (eku.code_signing, "Kod imzalama"),
                (eku.email_protection, "E-posta koruması"),
                (eku.time_stamping, "Zaman damgası"),
                (eku.ocsp_signing, "OCSP imzalama"),
            ]
            .iter()
            .filter(|u| u.0)
            .map(|u| u.1.to_string())
            .collect();
            list.extend(eku.other.iter().map(|o| o.to_id_string()));
            section(
                "Genişletilmiş anahtar kullanımları",
                vec![row("Amaçlar", list.join(", "))],
            )
        }
        ParsedExtension::SubjectKeyIdentifier(id) => section(
            "Konu anahtar kimliği",
            vec![mono("Anahtar kimliği", hex(id.0))],
        ),
        ParsedExtension::AuthorityKeyIdentifier(aki) => section(
            "Yetkili anahtar kimliği",
            aki.key_identifier
                .iter()
                .map(|id| mono("Anahtar kimliği", hex(id.0)))
                .collect(),
        ),
        ParsedExtension::CRLDistributionPoints(points) => section(
            "İptal listesi dağıtım noktaları",
            points
                .points
                .iter()
                .filter_map(|p| match &p.distribution_point {
                    Some(DistributionPointName::FullName(names)) => Some(names),
                    _ => None,
                })
                .flatten()
                .filter_map(general_name)
                .map(|mut r| {
                    r["k"] = "Dağıtım noktası".into();
                    r
                })
                .collect(),
        ),
        ParsedExtension::AuthorityInfoAccess(aia) => section(
            "Yetkili bilgi erişimi",
            aia.accessdescs
                .iter()
                .filter_map(|d| {
                    let method = match d.access_method.to_id_string().as_str() {
                        "1.3.6.1.5.5.7.48.1" => "OCSP",
                        "1.3.6.1.5.5.7.48.2" => "Veren sertifika",
                        _ => "Konum",
                    };
                    let mut r = general_name(&d.access_location)?;
                    r["k"] = method.into();
                    Some(r)
                })
                .collect(),
        ),
        ParsedExtension::CertificatePolicies(policies) => section(
            "Sertifika politikaları",
            policies
                .iter()
                .flat_map(|p| {
                    let oid = p.policy_id.to_id_string();
                    let name = match oid.as_str() {
                        "2.23.140.1.2.1" => format!("Alan adı doğrulamalı (DV) · {oid}"),
                        "2.23.140.1.2.2" => format!("Kuruluş doğrulamalı (OV) · {oid}"),
                        "2.23.140.1.2.3" => format!("Kişi doğrulamalı (IV) · {oid}"),
                        "2.23.140.1.1" => format!("Genişletilmiş doğrulama (EV) · {oid}"),
                        "2.5.29.32.0" => format!("Her politika · {oid}"),
                        _ => oid,
                    };
                    let mut rows = vec![row("Politika", name)];
                    for q in p.policy_qualifiers.iter().flatten() {
                        if q.policy_qualifier_id.to_id_string() == "1.3.6.1.5.5.7.2.1"
                            && let Some(cps) = ia5_string(q.qualifier)
                        {
                            rows.push(row("Uygulama beyanı", cps));
                        }
                    }
                    rows
                })
                .collect(),
        ),
        ParsedExtension::SCT(scts) => section(
            "Sertifika şeffaflığı kayıtları",
            scts.iter()
                .flat_map(|sct| {
                    [
                        mono("Kayıt kimliği", hex(sct.id.key_id)),
                        time("Zaman damgası", (sct.timestamp / 1000) as i64),
                    ]
                })
                .collect(),
        ),
        _ => None,
    }
}

/// DER kodlu kısa IA5String (politika adresleri).
fn ia5_string(der: &[u8]) -> Option<String> {
    let (&tag, rest) = der.split_first()?;
    let (&len, body) = rest.split_first()?;
    (tag == 0x16 && len < 0x80 && body.len() == len as usize)
        .then(|| String::from_utf8_lossy(body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAIN: &str = include_str!("testdata/cert_chain.pem");

    fn pems() -> Vec<String> {
        CHAIN
            .split_inclusive("-----END CERTIFICATE-----")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    }

    #[test]
    fn chain_round_trip() {
        let url = url(&pems());
        assert!(url.starts_with("http://julie.certificate/?cert="));
        assert_eq!(url.matches("cert=").count(), 3);
        let html = render(url.split_once('?').map(|(_, q)| q));
        assert!(html.contains(r#""label":"*.badssl.com""#));
        assert!(html.contains(r#""label":"Ornek RSA Kok Sertifika Yetkilisi""#));
        assert!(html.contains(r#""v":"badssl.com""#));
        assert!(html.contains("Sunucu kimlik doğrulaması"));
        assert!(html.contains("P-256"));
        assert!(html.contains("SHA-256 ile RSA"));
        assert!(html.contains("https://sectigo.com/CPS"));
        assert!(html.contains("Alan adı doğrulamalı"));
        // 9 Nisan 2015 00:00 UTC
        assert!(html.contains(r#""time":1428537600"#));
        assert!(html.contains("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn broken_input_is_ignored() {
        let html = render(Some("cert=bozuk!!&cert=AAAA&x=1"));
        assert!(html.contains(r#""certs":[]"#));
        let html = render(None);
        assert!(html.contains(r#""certs":[]"#));
    }

    #[test]
    fn pem_round_trip() {
        let pem = &pems()[0];
        let der = der_of(pem).unwrap();
        assert_eq!(der_of(&pem_of(&der)).unwrap(), der);
    }
}
