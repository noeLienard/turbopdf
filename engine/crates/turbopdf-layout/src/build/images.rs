//! Image sources → decoded images: PNG / JPEG / WebP / GIF headers, SVG (usvg), QR codes.

use super::*;

pub(super) fn image_info(bytes: &[u8]) -> Result<(ImageKind, u32, u32), String> {
    if bytes.len() > 16 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))
            .map_err(|e| format!("invalid WebP: {e}"))?;
        let (w, h) = d.dimensions();
        return Ok((ImageKind::Webp, w, h));
    }
    if bytes.len() > 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        if w == 0 || h == 0 {
            return Err("GIF with a zero dimension".into());
        }
        return Ok((ImageKind::Gif, w, h));
    }
    if bytes.len() > 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if w == 0 || h == 0 {
            return Err("PNG with a zero dimension".into());
        }
        return Ok((ImageKind::Png, w, h));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let mut d = jpeg_decoder::Decoder::new(bytes);
        d.read_info().map_err(|e| format!("invalid JPEG: {e}"))?;
        let info = d.info().ok_or("invalid JPEG")?;
        return Ok((ImageKind::Jpeg, info.width as u32, info.height as u32));
    }
    Err("unsupported image format (use PNG, JPEG, WebP, GIF or SVG)".into())
}

pub(super) fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let head = String::from_utf8_lossy(head);
    let t = head.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg")
        || ((t.starts_with("<?xml") || t.starts_with("<!--") || t.starts_with("<!DOCTYPE"))
            && head.contains("<svg"))
}

/// Fonts for SVG text: the document's own fonts only (never the system's), Inter by default.
pub(super) fn svg_fonts(cx: &mut Cx) -> Arc<usvg::fontdb::Database> {
    if let Some(db) = &cx.svg_fonts {
        return db.clone();
    }
    let mut db = usvg::fontdb::Database::new();
    for i in 0..cx.db.len() {
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> = cx.db.face(i).data.clone();
        db.load_font_source(usvg::fontdb::Source::Binary(data));
    }
    db.set_sans_serif_family("Inter");
    db.set_serif_family("Inter");
    db.set_monospace_family("Inter");
    let db = Arc::new(db);
    cx.svg_fonts = Some(db.clone());
    db
}

/// A QR code as an SVG: one path for all the dark modules (crisp edges), a 4-module quiet zone.
/// 1 module = 3 px, so the natural size is readable; the image's `width` usually sets the size.
pub(super) fn qr_svg(q: &t::QrCode) -> Result<String, String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let ecc = match q.ecc.unwrap_or(t::QrEcc::M) {
        t::QrEcc::L => QrCodeEcc::Low,
        t::QrEcc::M => QrCodeEcc::Medium,
        t::QrEcc::Q => QrCodeEcc::Quartile,
        t::QrEcc::H => QrCodeEcc::High,
    };
    let code = QrCode::encode_text(&q.qr, ecc)
        .map_err(|_| "QR code text is too long for this error correction level".to_string())?;
    let n = code.size();
    let quiet = 4;
    let total = n + 2 * quiet;
    let hex = |c: t::Color| {
        if c.a == 255 {
            format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
        }
    };
    let fg = q.color.map_or_else(|| "#000000".to_string(), hex);
    let bg = q.background.map_or_else(|| "#ffffff".to_string(), hex);
    let mut d = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if code.get_module(x, y) {
                let start = x;
                while x < n && code.get_module(x, y) {
                    x += 1;
                }
                d.push_str(&format!(
                    "M{} {}h{}v1h-{}z",
                    start + quiet,
                    y + quiet,
                    x - start,
                    x - start
                ));
            } else {
                x += 1;
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{w}\" viewBox=\"0 0 {total} {total}\" shape-rendering=\"crispEdges\"><rect width=\"{total}\" height=\"{total}\" fill=\"{bg}\"/><path fill=\"{fg}\" d=\"{d}\"/></svg>",
        w = total * 3
    ))
}

pub(super) fn parse_svg(cx: &mut Cx, bytes: &[u8]) -> Result<usvg::Tree, String> {
    let opts = usvg::Options {
        font_family: "Inter".into(),
        fontdb: svg_fonts(cx),
        // no file and no network: only `data:` images inside the SVG are drawn
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    usvg::Tree::from_data(bytes, &opts).map_err(|e| format!("invalid SVG: {e}"))
}

pub(super) fn load_image(cx: &mut Cx, src: &t::ImageSource, path: &str) -> Option<usize> {
    let bytes = match src {
        t::ImageSource::Data { data } => base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .map_err(|e| format!("invalid base64: {e}")),
        t::ImageSource::File { file } => cx.loader.load(file),
        t::ImageSource::Buffer { buffer } => cx.loader.buffer(*buffer),
        t::ImageSource::Qr(q) => qr_svg(q).map(String::into_bytes),
    };
    let result = bytes.and_then(|b| {
        if looks_like_svg(&b) {
            let tree = parse_svg(cx, &b)?;
            let (w, h) = (tree.size().width(), tree.size().height());
            return Ok(ImageData {
                bytes: Arc::new(b),
                kind: ImageKind::Svg,
                width_px: w.ceil() as u32,
                height_px: h.ceil() as u32,
                svg: Some(Arc::new(tree)),
                natural: (w * 0.75, h * 0.75),
            });
        }
        image_info(&b).map(|(kind, w, h)| ImageData {
            bytes: Arc::new(b),
            kind,
            width_px: w,
            height_px: h,
            svg: None,
            natural: (w as f32 * 0.75, h as f32 * 0.75),
        })
    });
    match result {
        Ok(img) => {
            cx.images.push(img);
            Some(cx.images.len() - 1)
        }
        Err(reason) => {
            cx.warnings.push(t::Warning::ImageFailed {
                path: path.to_string(),
                reason,
            });
            None
        }
    }
}
