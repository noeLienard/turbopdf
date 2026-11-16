//! Shaping with rustybuzz: one face, then the next family of the list and Inter for missing glyphs;
//! words and plans cached.

use super::*;

pub(crate) fn fallback_face(db: &FontDb, face: FaceId) -> FaceId {
    let f = db.face(face);
    db.select("Inter", f.weight, f.italic)
}

/// A shaped word: face, size (bits), text, tabular digits.
type RunKey = (FaceId, u32, String, bool);

/// Memory of already-shaped words. Tables repeat the same words thousands of times; shaping is the
/// expensive part, so identical (face, size, text) are shaped once. Lookup only: never iterated.
#[derive(Default)]
pub struct ShapeCache {
    runs: std::collections::HashMap<RunKey, (Run, Vec<MissingGlyph>)>,
    /// Compiled OpenType plans. Building one walks the GSUB/GPOS feature lists, which costs more than
    /// shaping a short word, so each (face, script, tabular digits) is compiled once.
    plans: std::collections::HashMap<(FaceId, Script, bool), ShapePlan>,
}

impl ShapeCache {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Shape `text` with one face. `missing` collects chars mapped to glyph 0.
pub(crate) fn shape(
    db: &FontDb,
    plans: &mut std::collections::HashMap<(FaceId, Script, bool), ShapePlan>,
    face: FaceId,
    size: f32,
    tabular: bool,
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
    let plan = plans.entry((face, script, tabular)).or_insert_with(|| {
        let features: Vec<rustybuzz::Feature> = if tabular {
            vec![rustybuzz::Feature::new(
                rustybuzz::ttf_parser::Tag::from_bytes(b"tnum"),
                1,
                ..,
            )]
        } else {
            Vec::new()
        };
        ShapePlan::new(
            shaper,
            Direction::LeftToRight,
            Some(script),
            None,
            &features,
        )
    });
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
        color: [0, 0, 0, 255], // set by `with_effects`
        link: None,
        effects: Effects {
            tabular,
            ..Effects::default()
        },
        decoration_line: None,
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
    // the face, the next family of the list, then Inter
    let mut chain = vec![style.face];
    for id in [style.fallback, Some(fallback_face(db, style.face))]
        .into_iter()
        .flatten()
    {
        if !chain.contains(&id) {
            chain.push(id);
        }
    }
    let shapers: Vec<_> = chain.iter().map(|&id| db.face(id).shaper()).collect();
    let covers = |face: &turbopdf_fonts::ShapeFace<'_>, c: char| {
        c.is_control() || face.glyph_index(c).is_some()
    };

    let mut chunks: Vec<(FaceId, String)> = Vec::new();
    for c in text.chars() {
        // nobody has it: the first face draws .notdef and we report it
        let id = chain[shapers.iter().position(|f| covers(f, c)).unwrap_or(0)];
        match chunks.last_mut() {
            Some((last, s)) if *last == id => s.push(c),
            _ => chunks.push((id, c.to_string())),
        }
    }
    let finish = |run: Run| with_effects(db, run, &style);
    chunks
        .into_iter()
        .map(|(id, s)| {
            let key = (id, style.size.to_bits(), s, style.effects.tabular);
            if let Some((run, miss)) = cache.runs.get(&key) {
                for m in miss {
                    if !missing.contains(m) {
                        missing.push(m.clone());
                    }
                }
                return finish(run.clone());
            }
            let mut local = Vec::new();
            let run = shape(
                db,
                &mut cache.plans,
                id,
                style.size,
                style.effects.tabular,
                &key.2,
                &mut local,
            );
            for m in &local {
                if !missing.contains(m) {
                    missing.push(m.clone());
                }
            }
            cache.runs.insert(key, (run.clone(), local));
            finish(run)
        })
        .collect()
}

/// A shaped (possibly cached) run with the span's color, link and effects; letter spacing goes after the
/// last glyph of every character cluster.
fn with_effects(db: &FontDb, mut run: Run, style: &TextStyle) -> Run {
    run.color = style.color;
    run.link = style.link;
    run.effects = style.effects;
    let f = db.face(run.face);
    let k = run.size / f.units_per_em;
    // OpenType gives the top of the line (`post` / `OS/2`), above the baseline when positive
    let line = |(top, thick): (f32, f32)| Some((top * k - thick * k / 2.0, thick * k));
    run.decoration_line = match style.effects.decoration {
        Decoration::None => None,
        Decoration::Underline => line(f.underline),
        Decoration::LineThrough => line(f.strikeout),
        Decoration::Overline => line((f.ascender, f.underline.1)),
    };
    let ls = style.effects.letter_spacing;
    if ls != 0.0 && run.size > 0.0 {
        let n = run.glyphs.len();
        for i in 0..n {
            let last_of_cluster =
                i + 1 == n || run.glyphs[i + 1].range.start != run.glyphs[i].range.start;
            if last_of_cluster {
                run.glyphs[i].advance_em += ls / run.size;
            }
        }
    }
    run
}
