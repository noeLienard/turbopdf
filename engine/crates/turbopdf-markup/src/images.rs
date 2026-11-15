//! Image sources: file paths, `data:` URLs, `qr:` codes and inline SVG.

use base64::Engine as _;
use turbopdf_tree::{ImageFormat, ImageSource, QrCode};

fn format_of(mime_or_ext: &str) -> Option<ImageFormat> {
    Some(match mime_or_ext.to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpeg" | "jpg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::Webp,
        "gif" => ImageFormat::Gif,
        "svg" | "svg+xml" => ImageFormat::Svg,
        _ => return None,
    })
}

/// `%41` → `A` (for `data:image/svg+xml;utf8,<svg …>` URLs).
fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// A `data:` URL → its bytes, base64-encoded for the tree, and its declared type.
pub fn data_url(src: &str) -> Option<(String, String)> {
    let rest = src.trim().strip_prefix("data:").or_else(|| {
        let t = src.trim();
        (t.len() > 5 && t[..5].eq_ignore_ascii_case("data:")).then(|| &t[5..])
    })?;
    let (meta, payload) = rest.split_once(',')?;
    let mut params = meta.split(';');
    let mime = params.next().unwrap_or("").trim().to_ascii_lowercase();
    let base64 = params.any(|p| p.trim().eq_ignore_ascii_case("base64"));
    let data = if base64 {
        payload.split_whitespace().collect()
    } else {
        base64::engine::general_purpose::STANDARD.encode(percent_decode(payload))
    };
    Some((mime, data))
}

/// A `src` (path, `file://`, `data:` URL or `qr:` text) → tree source and format. Remote URLs are kept:
/// the engine reports them (it never uses the network).
pub fn source(src: &str) -> (ImageSource, Option<ImageFormat>) {
    let t = src.trim();
    if t.len() >= 3 && t[..3].eq_ignore_ascii_case("qr:") {
        return (
            ImageSource::Qr(QrCode {
                qr: t[3..].to_string(),
                ecc: None,
                color: None,
                background: None,
            }),
            None,
        );
    }
    if let Some((mime, data)) = data_url(t)
        && let Some(kind) = mime.strip_prefix("image/")
    {
        return (ImageSource::Data { data }, format_of(kind));
    }
    let path = src.split(['?', '#']).next().unwrap_or(src);
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphabetic()));
    (
        ImageSource::File {
            file: src.to_string(),
        },
        ext.and_then(format_of),
    )
}

/// Inline `<svg>` source → tree source.
pub fn svg(xml: &str) -> ImageSource {
    ImageSource::Data {
        data: base64::engine::general_purpose::STANDARD.encode(xml),
    }
}
