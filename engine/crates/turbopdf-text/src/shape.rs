//! Shaping with rustybuzz: one face, then fallback to Inter for missing glyphs; words and plans cached.

use super::*;

pub(crate) fn fallback_face(db: &FontDb, face: FaceId) -> FaceId {
    let f = db.face(face);
    db.select("Inter", f.weight, f.italic)
}

/// Memory of already-shaped words. Tables repeat the same words thousands of times; shaping is the
/// expensive part, so identical (face, size, text) are shaped once. Lookup only: never iterated.
#[derive(Default)]
pub struct ShapeCache {
    runs: std::collections::HashMap<(FaceId, u32, String), (Run, Vec<MissingGlyph>)>,
    /// Compiled OpenType plans. Building one walks the GSUB/GPOS feature lists, which costs more than
    /// shaping a short word, so each (face, script) is compiled once.
    plans: std::collections::HashMap<(FaceId, Script), ShapePlan>,
}

impl ShapeCache {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Shape `text` with one face. `missing` collects chars mapped to glyph 0.
pub(crate) fn shape(
    db: &FontDb,
    plans: &mut std::collections::HashMap<(FaceId, Script), ShapePlan>,
    face: FaceId,
    size: f32,
    color: Rgba,
    text: &str,
    missing: &mut Vec<MissingGlyph>,
) -> Run {
    let f = db.face(face);
    let shaper = f.shaper();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(Direction::LeftToRight);
    buffer.guess_segment_properties();
    let script = buffer.script();
    let plan = plans
        .entry((face, script))
        .or_insert_with(|| ShapePlan::new(shaper, Direction::LeftToRight, Some(script), None, &[]));
    let out = rustybuzz::shape_with_plan(shaper, plan, buffer);
    let upem = f.units_per_em;
    let infos = out.glyph_infos();
    let positions = out.glyph_positions();
    let mut glyphs = Vec::with_capacity(infos.len());
    for (i, (info, pos)) in infos.iter().zip(positions).enumerate() {
        let start = info.cluster as usize;
        // cluster end = next glyph's cluster (or end of text)
        let end = infos
            .iter()
            .skip(i + 1)
            .map(|g| g.cluster as usize)
            .find(|&c| c != start)
            .unwrap_or(text.len());
        let end = end.max(start);
        if info.glyph_id == 0
            && let Some(ch) = text[start..].chars().next()
            && !ch.is_whitespace()
            && !missing.iter().any(|m| m.character == ch)
        {
            missing.push(MissingGlyph {
                character: ch,
                face,
            });
        }
        glyphs.push(Glyph {
            id: info.glyph_id as u16,
            advance_em: pos.x_advance as f32 / upem,
            x_offset_em: pos.x_offset as f32 / upem,
            y_offset_em: pos.y_offset as f32 / upem,
            range: start..end,
        });
    }
    Run {
        face,
        size,
        color,
        link: None,
        x: 0.0,
        text: text.to_string(),
        glyphs,
    }
}

/// Split `text` in chunks that each use the first face able to draw every char; Inter is the fallback.
pub(crate) fn shape_with_fallback(
    db: &FontDb,
    cache: &mut ShapeCache,
    style: TextStyle,
    text: &str,
    missing: &mut Vec<MissingGlyph>,
) -> Vec<Run> {
    let primary = db.face(style.face).shaper();
    let fb_id = fallback_face(db, style.face);
    let fb = if fb_id != style.face {
        Some(db.face(fb_id).shaper())
    } else {
        None
    };
    let covers = |face: &turbopdf_fonts::ShapeFace<'_>, c: char| {
        c.is_control() || face.glyph_index(c).is_some()
    };

    let mut chunks: Vec<(FaceId, String)> = Vec::new();
    for c in text.chars() {
        let id = if covers(primary, c) {
            style.face
        } else if fb.is_some_and(|f| covers(f, c)) {
            fb_id
        } else {
            style.face // nobody has it: primary draws .notdef and we report it
        };
        match chunks.last_mut() {
            Some((last, s)) if *last == id => s.push(c),
            _ => chunks.push((id, c.to_string())),
        }
    }
    chunks
        .into_iter()
        .map(|(id, s)| {
            let key = (id, style.size.to_bits(), s);
            if let Some((run, miss)) = cache.runs.get(&key) {
                for m in miss {
                    if !missing.contains(m) {
                        missing.push(m.clone());
                    }
                }
                return Run {
                    color: style.color,
                    link: style.link,
                    ..run.clone()
                };
            }
            let mut local = Vec::new();
            let run = shape(
                db,
                &mut cache.plans,
                id,
                style.size,
                style.color,
                &key.2,
                &mut local,
            );
            for m in &local {
                if !missing.contains(m) {
                    missing.push(m.clone());
                }
            }
            cache.runs.insert(key, (run.clone(), local));
            Run {
                link: style.link,
                ..run
            }
        })
        .collect()
}
