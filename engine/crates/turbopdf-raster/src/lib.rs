//! PNG previews (tiny-skia), drawn from the same [`Layout`] as the PDF.
//!
//! Glyphs are filled from their outlines, so the preview shows exactly the glyphs the PDF embeds.
//! Note: the *PDF* is byte-for-byte reproducible; PNG pixels may differ in the last bit between CPU
//! architectures (tiny-skia selects SIMD code paths at runtime).

use rustybuzz::ttf_parser::{self, GlyphId, OutlineBuilder};
use std::collections::HashMap;
use tiny_skia::{
    FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    StrokeDash, Transform,
};
use turbopdf_fonts::{FaceId, FontDb};
use turbopdf_layout::{DrawOp, ImageKind, Layout, Visual};
use turbopdf_tree::{BorderStyle, ObjectFit};

#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    #[error("preview scale must be between 0.1 and 8 (got {0})")]
    Scale(f32),
    #[error("could not allocate a {0}x{1} image")]
    Size(u32, u32),
    #[error("could not decode image: {0}")]
    Image(String),
    #[error("could not encode PNG: {0}")]
    Encode(String),
}

struct Outline(PathBuilder);

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y)
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y)
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y)
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y)
    }
    fn close(&mut self) {
        self.0.close()
    }
}

fn paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

fn rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.0 {
        return PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?).into();
    }
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn pixmap_from_rgba(w: u32, h: u32, rgba: &[u8]) -> Result<Pixmap, RasterError> {
    let mut pm = Pixmap::new(w, h).ok_or(RasterError::Size(w, h))?;
    for (dst, src) in pm.pixels_mut().iter_mut().zip(rgba.chunks(4)) {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    Ok(pm)
}

fn decode_image(layout: &Layout, i: usize) -> Result<Pixmap, RasterError> {
    let d = &layout.images[i];
    match d.kind {
        ImageKind::Png => {
            Pixmap::decode_png(&d.bytes).map_err(|e| RasterError::Image(e.to_string()))
        }
        ImageKind::Svg => Err(RasterError::Image("SVGs are rendered, not decoded".into())),
        ImageKind::Webp => {
            let err = |e: image_webp::DecodingError| RasterError::Image(e.to_string());
            let mut dec =
                image_webp::WebPDecoder::new(std::io::Cursor::new(&d.bytes[..])).map_err(err)?;
            let (w, h) = dec.dimensions();
            let mut buf = vec![0; dec.output_buffer_size().ok_or(RasterError::Size(w, h))?];
            dec.read_image(&mut buf).map_err(err)?;
            let rgba: Vec<u8> = if dec.has_alpha() {
                buf
            } else {
                buf.chunks(3)
                    .flat_map(|c| [c[0], c[1], c[2], 255])
                    .collect()
            };
            pixmap_from_rgba(w, h, &rgba)
        }
        ImageKind::Gif => {
            let mut opts = gif::DecodeOptions::new();
            opts.set_color_output(gif::ColorOutput::RGBA);
            let err = |e: gif::DecodingError| RasterError::Image(e.to_string());
            let mut dec = opts.read_info(&d.bytes[..]).map_err(err)?;
            let (w, h) = (dec.width() as u32, dec.height() as u32);
            let frame = dec
                .read_next_frame()
                .map_err(err)?
                .ok_or_else(|| RasterError::Image("GIF without frames".into()))?;
            // first frame, placed on a transparent canvas of the logical screen size
            let mut canvas = vec![0u8; (w * h * 4) as usize];
            let (fl, ft, fw) = (frame.left as u32, frame.top as u32, frame.width as u32);
            for (row, chunk) in frame.buffer.chunks((fw * 4) as usize).enumerate() {
                let y = ft + row as u32;
                if y >= h {
                    break;
                }
                for (col, px) in chunk.chunks(4).enumerate() {
                    let x = fl + col as u32;
                    if x < w {
                        let o = ((y * w + x) * 4) as usize;
                        canvas[o..o + 4].copy_from_slice(px);
                    }
                }
            }
            pixmap_from_rgba(w, h, &canvas)
        }
        ImageKind::Jpeg => {
            let mut dec = jpeg_decoder::Decoder::new(&d.bytes[..]);
            let px = dec
                .decode()
                .map_err(|e| RasterError::Image(e.to_string()))?;
            let info = dec
                .info()
                .ok_or_else(|| RasterError::Image("no JPEG info".into()))?;
            let (w, h) = (info.width as u32, info.height as u32);
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            match info.pixel_format {
                jpeg_decoder::PixelFormat::RGB24 => px
                    .chunks(3)
                    .for_each(|c| rgba.extend_from_slice(&[c[0], c[1], c[2], 255])),
                jpeg_decoder::PixelFormat::L8 => px
                    .iter()
                    .for_each(|&v| rgba.extend_from_slice(&[v, v, v, 255])),
                jpeg_decoder::PixelFormat::L16 => px
                    .chunks(2)
                    .for_each(|c| rgba.extend_from_slice(&[c[0], c[0], c[0], 255])),
                jpeg_decoder::PixelFormat::CMYK32 => px.chunks(4).for_each(|c| {
                    let k = c[3] as u32;
                    rgba.extend_from_slice(&[
                        (c[0] as u32 * k / 255) as u8,
                        (c[1] as u32 * k / 255) as u8,
                        (c[2] as u32 * k / 255) as u8,
                        255,
                    ])
                }),
            }
            let mut pm = Pixmap::new(w, h).ok_or(RasterError::Size(w, h))?;
            for (dst, src) in pm.pixels_mut().iter_mut().zip(rgba.chunks(4)) {
                *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
            }
            Ok(pm)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_box(
    pm: &mut Pixmap,
    sc: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    v: &Visual,
    sides: [bool; 4],
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let ts = Transform::from_scale(sc, sc);
    if let Some(bg) = v.background
        && let Some(p) = rect_path(x, y, w, h, v.radius)
    {
        pm.fill_path(&p, &paint(bg), FillRule::Winding, ts, None);
    }
    let Some(b) = v.border else { return };
    let bw = b.width;
    if sides.iter().all(|&d| d) {
        if let Some(p) = rect_path(
            x + bw / 2.0,
            y + bw / 2.0,
            w - bw,
            h - bw,
            (v.radius - bw / 2.0).max(0.0),
        ) {
            let mut stroke = Stroke {
                width: bw,
                ..Default::default()
            };
            stroke.dash = match b.style {
                BorderStyle::Solid => None,
                BorderStyle::Dashed => StrokeDash::new(vec![bw * 3.0, bw * 2.0], 0.0),
                BorderStyle::Dotted => StrokeDash::new(vec![bw, bw * 1.5], 0.0),
            };
            pm.stroke_path(&p, &paint(b.color), &stroke, ts, None);
        }
        return;
    }
    // partial borders (a box cut by a page break): filled bars
    let bars = [
        (sides[0], x, y, w, bw),
        (sides[1], x + w - bw, y, bw, h),
        (sides[2], x, y + h - bw, w, bw),
        (sides[3], x, y, bw, h),
    ];
    for (on, bx, by, bwid, bh) in bars {
        if on && let Some(p) = rect_path(bx, by, bwid, bh, 0.0) {
            pm.fill_path(&p, &paint(b.color), FillRule::Winding, ts, None);
        }
    }
}

fn draw_run(
    pm: &mut Pixmap,
    db: &FontDb,
    cache: &mut HashMap<(FaceId, u16), Option<Path>>,
    base: Transform,
    x: f32,
    baseline: f32,
    run: &turbopdf_text::Run,
) {
    let face = db.face(run.face);
    let shaper = face.shaper();
    let k = run.size / face.units_per_em;
    let p = paint(run.color);
    let mut pen = x;
    for g in &run.glyphs {
        let path = cache.entry((run.face, g.id)).or_insert_with(|| {
            let mut b = Outline(PathBuilder::new());
            shaper.outline_glyph(GlyphId(g.id), &mut b)?;
            b.0.finish()
        });
        if let Some(path) = path {
            let gx = pen + g.x_offset_em * run.size;
            let gy = baseline - g.y_offset_em * run.size;
            pm.fill_path(
                path,
                &p,
                FillRule::Winding,
                base.pre_translate(gx, gy).pre_scale(k, -k),
                None,
            );
        }
        pen += g.advance_em * run.size;
    }
}

/// One PNG per page. `scale` multiplies the 72 dpi size (the SDK uses 2 by default).
pub fn render(layout: &Layout, db: &FontDb, scale: f32) -> Result<Vec<Vec<u8>>, RasterError> {
    if !(0.1..=8.0).contains(&scale) {
        return Err(RasterError::Scale(scale));
    }
    let mut glyph_cache: HashMap<(FaceId, u16), Option<Path>> = HashMap::new();
    let mut image_cache: HashMap<usize, Pixmap> = HashMap::new();
    let mut out = Vec::new();

    for page in &layout.pages {
        let (pw, ph) = (
            (page.width * scale).ceil() as u32,
            (page.height * scale).ceil() as u32,
        );
        let mut pm = Pixmap::new(pw, ph).ok_or(RasterError::Size(pw, ph))?;
        pm.fill(tiny_skia::Color::WHITE);

        for op in &page.ops {
            match op {
                DrawOp::Box {
                    x,
                    y,
                    w,
                    h,
                    visual,
                    sides,
                } => draw_box(&mut pm, scale, *x, *y, *w, *h, visual, *sides),
                DrawOp::Text { x, baseline, run } => draw_run(
                    &mut pm,
                    db,
                    &mut glyph_cache,
                    Transform::from_scale(scale, scale),
                    *x,
                    *baseline,
                    run,
                ),
                DrawOp::Rotated {
                    cx,
                    cy,
                    angle,
                    x,
                    baseline,
                    run,
                } => {
                    let base = Transform::from_scale(scale, scale)
                        .pre_translate(*cx, *cy)
                        .pre_concat(Transform::from_rotate(-angle));
                    draw_run(&mut pm, db, &mut glyph_cache, base, *x, *baseline, run);
                }
                DrawOp::Image {
                    x,
                    y,
                    w,
                    h,
                    image,
                    fit,
                } => {
                    let data = &layout.images[*image];
                    let (nw, nh) = data.natural;
                    if let Some(tree) = &data.svg {
                        let (dw, dh) = match fit {
                            ObjectFit::Fill => (*w, *h),
                            ObjectFit::None => (nw, nh),
                            ObjectFit::Contain => {
                                let k = (w / nw).min(h / nh);
                                (nw * k, nh * k)
                            }
                            ObjectFit::Cover => {
                                let k = (w / nw).max(h / nh);
                                (nw * k, nh * k)
                            }
                        };
                        let (ox, oy) = (x + (w - dw) / 2.0, y + (h - dh) / 2.0);
                        let ts = Transform::from_scale(scale, scale)
                            .pre_translate(ox, oy)
                            .pre_scale(dw / tree.size().width(), dh / tree.size().height());
                        resvg::render(tree, ts, &mut pm.as_mut());
                        continue;
                    }
                    if !image_cache.contains_key(image) {
                        image_cache.insert(*image, decode_image(layout, *image)?);
                    }
                    let src = &image_cache[image];
                    let (dw, dh) = match fit {
                        ObjectFit::Fill => (*w, *h),
                        ObjectFit::None => (nw, nh),
                        ObjectFit::Contain => {
                            let k = (w / nw).min(h / nh);
                            (nw * k, nh * k)
                        }
                        ObjectFit::Cover => {
                            let k = (w / nw).max(h / nh);
                            (nw * k, nh * k)
                        }
                    };
                    let (ox, oy) = (x + (w - dw) / 2.0, y + (h - dh) / 2.0);
                    let mut mask = None;
                    if matches!(fit, ObjectFit::Cover | ObjectFit::None)
                        && let (Some(m), Some(rp)) =
                            (Mask::new(pw, ph), rect_path(*x, *y, *w, *h, 0.0))
                    {
                        let mut m = m;
                        m.fill_path(
                            &rp,
                            FillRule::Winding,
                            true,
                            Transform::from_scale(scale, scale),
                        );
                        mask = Some(m);
                    }
                    let ts = Transform::from_scale(scale, scale)
                        .pre_translate(ox, oy)
                        .pre_scale(dw / src.width() as f32, dh / src.height() as f32);
                    let pp = PixmapPaint {
                        quality: FilterQuality::Bicubic,
                        ..Default::default()
                    };
                    pm.draw_pixmap(0, 0, src.as_ref(), &pp, ts, mask.as_ref());
                }
            }
        }
        out.push(
            pm.encode_png()
                .map_err(|e| RasterError::Encode(e.to_string()))?,
        );
    }
    Ok(out)
}

#[allow(dead_code)]
fn _assert_ttf_parser_in_scope(_: &ttf_parser::Face) {}
