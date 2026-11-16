//! Words, spaces and newlines → lines: greedy breaking, hyphenation, alignment and justification.

use super::*;

pub(crate) fn tokenize(
    db: &FontDb,
    cache: &mut ShapeCache,
    spans: &[Span],
    missing: &mut Vec<MissingGlyph>,
) -> Vec<Unit> {
    // flatten to (char, span index)
    let mut chars: Vec<(char, usize)> = Vec::new();
    for (i, s) in spans.iter().enumerate() {
        chars.extend(s.text.chars().map(|c| (c, i)));
    }
    let mut units = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (c, si) = chars[i];
        let style = spans[si].style;
        if c == '\n' {
            units.push(Unit {
                kind: Kind::Newline,
                pieces: vec![],
                width: 0.0,
                style,
                text: String::new(),
            });
            i += 1;
        } else if c.is_whitespace() && c != '\u{a0}' && c != '\u{202f}' {
            // one collapsed space per whitespace run
            let mut j = i;
            while j < chars.len()
                && chars[j].0.is_whitespace()
                && chars[j].0 != '\n'
                && chars[j].0 != '\u{a0}'
                && chars[j].0 != '\u{202f}'
            {
                j += 1;
            }
            let run = shape_with_fallback(db, cache, style, " ", missing);
            let width = run.iter().map(Run::width).sum();
            units.push(Unit {
                kind: Kind::Space,
                pieces: run.into_iter().map(|run| Piece { run }).collect(),
                width,
                style,
                text: " ".into(),
            });
            i = j;
        } else {
            let mut j = i;
            while j < chars.len()
                && (!chars[j].0.is_whitespace()
                    || chars[j].0 == '\u{a0}'
                    || chars[j].0 == '\u{202f}')
            {
                j += 1;
            }
            // group by span (style)
            let mut pieces = Vec::new();
            let mut k = i;
            let mut word = String::new();
            let mut single_style = true;
            while k < j {
                let sk = chars[k].1;
                let mut e = k;
                let mut seg = String::new();
                while e < j && chars[e].1 == sk {
                    seg.push(chars[e].0);
                    e += 1;
                }
                if sk != si {
                    single_style = false;
                }
                word.push_str(&seg);
                for run in shape_with_fallback(db, cache, spans[sk].style, &seg, missing) {
                    pieces.push(Piece { run });
                }
                k = e;
            }
            // lay pieces out within the unit
            let mut x = 0.0;
            for p in &mut pieces {
                p.run.x = x;
                x += p.run.width();
            }
            units.push(Unit {
                kind: Kind::Word,
                pieces,
                width: x,
                style,
                text: if single_style { word } else { String::new() },
            });
            i = j;
        }
    }
    units
}

pub(crate) fn hyphen_lang(lang: &str) -> Option<hypher::Lang> {
    let code = lang.split(['-', '_']).next()?.to_ascii_lowercase();
    let bytes: [u8; 2] = code.as_bytes().try_into().ok()?;
    hypher::Lang::from_iso(bytes)
}

/// Split a word unit at `byte` (in its text), re-shaping both halves; the left half gets a hyphen.
pub(crate) fn split_word(
    db: &FontDb,
    unit: &Unit,
    byte: usize,
    missing: &mut Vec<MissingGlyph>,
) -> (Unit, Unit) {
    let (a, b) = unit.text.split_at(byte);
    let style = unit.style;
    let mk = |t: &str, missing: &mut Vec<MissingGlyph>| {
        let mut pieces: Vec<Piece> =
            shape_with_fallback(db, &mut ShapeCache::new(), style, t, missing)
                .into_iter()
                .map(|run| Piece { run })
                .collect();
        let mut x = 0.0;
        for p in &mut pieces {
            p.run.x = x;
            x += p.run.width();
        }
        Unit {
            kind: Kind::Word,
            pieces,
            width: x,
            style,
            text: t.to_string(),
        }
    };
    (mk(&format!("{a}-"), missing), mk(b, missing))
}

pub(crate) struct Pending {
    /// Words and spaces of the line being built.
    pub(crate) units: Vec<Unit>,
    /// Width including inner spaces, excluding the trailing space.
    pub(crate) width: f32,
}

pub(crate) fn line_metrics(db: &FontDb, units: &[Unit], fallback: &TextStyle) -> (f32, f32) {
    // (ascent above baseline, descent below baseline) of the line box
    let measure = |style: &TextStyle| {
        let f = db.face(style.face);
        let size = style.size;
        let a = f.ascender / f.units_per_em * size;
        let d = -f.descender / f.units_per_em * size;
        let half = (style.line_height * size - (a + d)) / 2.0;
        (a + half, d + half)
    };
    let mut boxes = units
        .iter()
        .filter(|u| u.kind != Kind::Space)
        .map(|u| measure(&u.style))
        .peekable();
    if boxes.peek().is_none() {
        return measure(fallback);
    }
    boxes.fold((f32::MIN, f32::MIN), |(a, d), (ua, ud)| {
        (a.max(ua), d.max(ud))
    })
}

pub(crate) fn finish_line(
    db: &FontDb,
    mut units: Vec<Unit>,
    max_width: f32,
    align: Align,
    justify_this: bool,
    base: &TextStyle,
) -> Line {
    // drop trailing spaces
    while units.last().is_some_and(|u| u.kind == Kind::Space) {
        units.pop();
    }
    let content: f32 = units.iter().map(|u| u.width).sum();
    let spaces = units.iter().filter(|u| u.kind == Kind::Space).count();
    let (asc, desc) = line_metrics(db, &units, base);

    let mut extra = 0.0;
    let mut offset = 0.0;
    match align {
        Align::Left => {}
        Align::Center => offset = ((max_width - content) / 2.0).max(0.0),
        Align::Right => offset = (max_width - content).max(0.0),
        Align::Justify => {
            if justify_this && spaces > 0 && content < max_width {
                extra = (max_width - content) / spaces as f32;
            }
        }
    }

    let mut runs: Vec<Run> = Vec::new();
    let mut x = offset;
    for u in &units {
        for p in &u.pieces {
            let mut run = p.run.clone();
            run.x = x + p.run.x;
            if u.kind == Kind::Space
                && extra > 0.0
                && let Some(g) = run.glyphs.first_mut()
            {
                g.advance_em += extra / run.size;
            }
            runs.push(run);
        }
        x += u.width + if u.kind == Kind::Space { extra } else { 0.0 };
    }
    // merge neighbours that share face / size / color
    let mut merged: Vec<Run> = Vec::new();
    for r in runs {
        if let Some(last) = merged.last_mut()
            && last.face == r.face
            && last.size == r.size
            && last.color == r.color
            && last.link == r.link
            && last.effects == r.effects
            && (last.x + last.width() - r.x).abs() < 0.001
        {
            let shift = last.text.len();
            last.glyphs.extend(r.glyphs.into_iter().map(|mut g| {
                g.range = g.range.start + shift..g.range.end + shift;
                g
            }));
            last.text.push_str(&r.text);
            continue;
        }
        merged.push(r);
    }
    Line {
        runs: merged,
        width: content + extra * spaces as f32,
        baseline: asc,
        height: asc + desc,
    }
}
