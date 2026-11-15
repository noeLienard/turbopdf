//! Light web fonts for the preview.
//!
//! The font is kept whole (cmap, GSUB, GPOS, metrics: the browser shapes text exactly like the engine),
//! except the outlines of the glyphs the document does not use, which are emptied. Then every table is
//! zlib-compressed into a WOFF 1 file. Inter goes from ~410 KB to a few dozen KB for a typical page.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::collections::BTreeSet;
use std::io::Write;

struct Table<'a> {
    tag: [u8; 4],
    data: std::borrow::Cow<'a, [u8]>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// `font` (TrueType or OpenType, not a collection) → WOFF bytes, with only `used` glyphs drawn.
/// `None` when the font cannot be read: the caller embeds it as it is.
pub fn light_woff(font: &[u8], used: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let flavor = u32_at(font, 0)?;
    if &font[..4] == b"ttcf" {
        return None;
    }
    let n = u16_at(font, 4)? as usize;
    let mut tables = Vec::with_capacity(n);
    for i in 0..n {
        let e = 12 + 16 * i;
        let tag: [u8; 4] = font.get(e..e + 4)?.try_into().ok()?;
        let off = u32_at(font, e + 8)? as usize;
        let len = u32_at(font, e + 12)? as usize;
        if &tag == b"DSIG" {
            continue; // the signature is broken by any change, and browsers ignore it
        }
        let data = font.get(off..off + len)?;
        if &tag == b"post" && data.len() > 32 {
            // glyph names (version 2) are useless to a browser: keep the header only, as version 3
            let mut post = data[..32].to_vec();
            post[..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
            tables.push(Table {
                tag,
                data: post.into(),
            });
            continue;
        }
        tables.push(Table {
            tag,
            data: std::borrow::Cow::Borrowed(data),
        });
    }
    let find = |t: &[u8; 4], tables: &[Table]| tables.iter().position(|x| &x.tag == t);
    // TrueType outlines, not variable: empty the glyphs nobody uses
    if let (Some(glyf), Some(loca), Some(head), Some(maxp), None) = (
        find(b"glyf", &tables),
        find(b"loca", &tables),
        find(b"head", &tables),
        find(b"maxp", &tables),
        find(b"gvar", &tables),
    ) && let Some((new_glyf, new_loca)) = strip_glyf(
        &tables[glyf].data,
        &tables[loca].data,
        i16::from_be_bytes([tables[head].data[50], tables[head].data[51]]),
        u16_at(&tables[maxp].data, 4)?,
        used,
    ) {
        let mut h = tables[head].data.to_vec();
        h[50..52].copy_from_slice(&1i16.to_be_bytes()); // long offsets in the new loca
        tables[head].data = h.into();
        tables[glyf].data = new_glyf.into();
        tables[loca].data = new_loca.into();
    }
    tables.sort_by_key(|t| t.tag);
    Some(woff(flavor, &tables))
}

/// New `glyf` and long-format `loca` keeping the outlines of `used` glyphs (and their components).
fn strip_glyf(
    glyf: &[u8],
    loca: &[u8],
    loca_format: i16,
    num_glyphs: u16,
    used: &BTreeSet<u16>,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let offset = |g: u16| -> Option<usize> {
        if loca_format == 0 {
            u16_at(loca, g as usize * 2).map(|v| v as usize * 2)
        } else {
            u32_at(loca, g as usize * 4).map(|v| v as usize)
        }
    };
    let range = |g: u16| -> Option<std::ops::Range<usize>> {
        let (a, b) = (offset(g)?, offset(g + 1)?);
        (a <= b && b <= glyf.len()).then_some(a..b)
    };
    // closure over composite glyphs
    let mut keep: BTreeSet<u16> = used.iter().copied().filter(|&g| g < num_glyphs).collect();
    keep.insert(0); // .notdef
    let mut todo: Vec<u16> = keep.iter().copied().collect();
    while let Some(g) = todo.pop() {
        let r = range(g)?;
        let d = &glyf[r];
        if d.len() < 10 || i16::from_be_bytes([d[0], d[1]]) >= 0 {
            continue;
        }
        let mut p = 10;
        loop {
            let flags = u16_at(d, p)?;
            let comp = u16_at(d, p + 2)?;
            if comp < num_glyphs && keep.insert(comp) {
                todo.push(comp);
            }
            p += 4 + if flags & 0x1 != 0 { 4 } else { 2 };
            p += if flags & 0x8 != 0 {
                2
            } else if flags & 0x40 != 0 {
                4
            } else if flags & 0x80 != 0 {
                8
            } else {
                0
            };
            if flags & 0x20 == 0 {
                break;
            }
        }
    }
    let mut out_glyf = Vec::new();
    let mut out_loca = Vec::with_capacity((num_glyphs as usize + 1) * 4);
    for g in 0..num_glyphs {
        out_loca.extend_from_slice(&(out_glyf.len() as u32).to_be_bytes());
        if keep.contains(&g) {
            out_glyf.extend_from_slice(&glyf[range(g)?]);
            while out_glyf.len() % 4 != 0 {
                out_glyf.push(0);
            }
        }
    }
    out_loca.extend_from_slice(&(out_glyf.len() as u32).to_be_bytes());
    Some((out_glyf, out_loca))
}

fn checksum(d: &[u8]) -> u32 {
    d.chunks(4).fold(0u32, |s, c| {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        s.wrapping_add(u32::from_be_bytes(b))
    })
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

/// WOFF 1: header, table directory, then each table zlib-compressed (or stored when that is not smaller).
fn woff(flavor: u32, tables: &[Table]) -> Vec<u8> {
    let n = tables.len();
    let blobs: Vec<Vec<u8>> = tables
        .iter()
        .map(|t| {
            let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
            let _ = z.write_all(&t.data);
            let c = z.finish().unwrap_or_default();
            if !c.is_empty() && c.len() < t.data.len() {
                c
            } else {
                t.data.to_vec()
            }
        })
        .collect();
    let sfnt_size = 12 + 16 * n + tables.iter().map(|t| pad4(t.data.len())).sum::<usize>();
    let mut offset = 44 + 20 * n;
    let mut dir = Vec::with_capacity(20 * n);
    let mut body = Vec::new();
    for (t, b) in tables.iter().zip(&blobs) {
        dir.extend_from_slice(&t.tag);
        dir.extend_from_slice(&(offset as u32).to_be_bytes());
        dir.extend_from_slice(&(b.len() as u32).to_be_bytes());
        dir.extend_from_slice(&(t.data.len() as u32).to_be_bytes());
        dir.extend_from_slice(&checksum(&t.data).to_be_bytes());
        body.extend_from_slice(b);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 44 + 20 * n + body.len();
    }
    let total = 44 + dir.len() + body.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"wOFF");
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(sfnt_size as u32).to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // font version (informative)
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&[0u8; 20]); // no metadata, no private data
    out.extend_from_slice(&dir);
    out.extend_from_slice(&body);
    out
}
