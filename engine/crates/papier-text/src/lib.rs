//! Text shaping (rustybuzz), line breaking and hyphenation (hypher).
//!
//! Deterministic by construction: only the fonts handed in are used, iteration order is the
//! input order, line breaking is greedy, no locale / clock / system font is ever consulted.

use papier_fonts::{FaceId, FontDb};
use rustybuzz::{Direction, Script, ShapePlan, UnicodeBuffer};

pub type Rgba = [u8; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub face: FaceId,
    /// Points.
    pub size: f32,
    pub color: Rgba,
    /// Multiplier of the font size (1.4 = 140 %).
    pub line_height: f32,
    /// Index of a link target in the caller's table: the glyphs of this span are clickable.
    pub link: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug)]
pub struct ParagraphOptions {
    pub max_width: f32,
    pub align: Align,
    /// BCP 47 language ("fr", "en-US"). Used for hyphenation only.
    pub lang: Option<String>,
    pub hyphenate: bool,
}

impl ParagraphOptions {
    pub fn new(max_width: f32) -> Self {
        Self {
            max_width,
            align: Align::Left,
            lang: None,
            hyphenate: false,
        }
    }
}

/// A shaped glyph, positioned relative to the start of its [`Run`]. All distances in points.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub id: u16,
    /// Horizontal advance in em units (multiply by size): what the PDF writer wants.
    pub advance_em: f32,
    pub x_offset_em: f32,
    pub y_offset_em: f32,
    /// Byte range of the source text of the run this glyph stands for.
    pub range: std::ops::Range<usize>,
}

/// Consecutive glyphs sharing face, size, color and link.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub face: FaceId,
    pub size: f32,
    pub color: Rgba,
    /// See [`TextStyle::link`].
    pub link: Option<u32>,
    /// X of the first glyph, relative to the start of the line box.
    pub x: f32,
    pub text: String,
    pub glyphs: Vec<Glyph>,
}

impl Run {
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance_em * self.size).sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub runs: Vec<Run>,
    /// Width of the content (spaces at the end excluded).
    pub width: f32,
    /// Distance from the top of the line box to the baseline.
    pub baseline: f32,
    pub height: f32,
}

/// A character that no font could draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingGlyph {
    pub character: char,
    pub face: FaceId,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    pub lines: Vec<Line>,
    /// Widest line.
    pub width: f32,
    pub height: f32,
    pub missing: Vec<MissingGlyph>,
}

// ------------------------------------------------------------------ internals

#[derive(Clone)]
struct Piece {
    run: Run, // x relative to the unit start
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Newline,
}

#[derive(Clone)]
struct Unit {
    kind: Kind,
    pieces: Vec<Piece>,
    width: f32,
    /// Style of the first char, used for empty lines and spaces.
    style: TextStyle,
    /// Source word, for hyphenation (only for words made of one style).
    text: String,
}

fn fallback_face(db: &FontDb, face: FaceId) -> FaceId {
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
fn shape(
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
fn shape_with_fallback(
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
    let covers = |face: &papier_fonts::ShapeFace<'_>, c: char| {
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

fn tokenize(
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

fn hyphen_lang(lang: &str) -> Option<hypher::Lang> {
    let code = lang.split(['-', '_']).next()?.to_ascii_lowercase();
    let bytes: [u8; 2] = code.as_bytes().try_into().ok()?;
    hypher::Lang::from_iso(bytes)
}

/// Split a word unit at `byte` (in its text), re-shaping both halves; the left half gets a hyphen.
fn split_word(
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

struct Pending {
    units: Vec<Unit>, // words and spaces of the line being built
    width: f32,       // width including inner spaces, excluding trailing space
}

fn line_metrics(db: &FontDb, units: &[Unit], fallback: &TextStyle) -> (f32, f32) {
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

fn finish_line(
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

/// Shaped text, independent of the width: shape **once**, then lay out at any width.
pub struct Prepared {
    units: Vec<Unit>,
    base: TextStyle,
    missing: Vec<MissingGlyph>,
    min_content: f32,
    max_content: f32,
}

impl Prepared {
    /// Widest unbreakable word, and everything on one line.
    pub fn intrinsic_widths(&self) -> (f32, f32) {
        (self.min_content, self.max_content)
    }
}

/// Shape the spans. Empty input gives `None`.
pub fn prepare(db: &FontDb, cache: &mut ShapeCache, spans: &[Span]) -> Option<Prepared> {
    let base = spans.first()?.style;
    let mut missing = Vec::new();
    let units = tokenize(db, cache, spans, &mut missing);
    let (mut min, mut max, mut cur) = (0.0f32, 0.0f32, 0.0f32);
    for u in &units {
        if u.kind == Kind::Newline {
            max = max.max(cur);
            cur = 0.0;
        } else {
            cur += u.width;
            if u.kind == Kind::Word {
                min = min.max(u.width);
            }
        }
    }
    Some(Prepared {
        units,
        base,
        missing,
        min_content: min,
        max_content: max.max(cur),
    })
}

/// Lay a paragraph out: shape, break into lines, align.
pub fn layout(db: &FontDb, spans: &[Span], opts: &ParagraphOptions) -> Paragraph {
    match prepare(db, &mut ShapeCache::new(), spans) {
        Some(p) => layout_prepared(db, &p, opts),
        None => Paragraph::default(),
    }
}

/// Break already-shaped text into lines at `opts.max_width`.
pub fn layout_prepared(db: &FontDb, prepared: &Prepared, opts: &ParagraphOptions) -> Paragraph {
    let mut missing = prepared.missing.clone();
    let base = prepared.base;
    let units = prepared.units.clone();
    let lang = if opts.hyphenate {
        opts.lang.as_deref().and_then(hyphen_lang)
    } else {
        None
    };
    let eps = 0.01;

    let mut lines: Vec<Line> = Vec::new();
    let mut cur = Pending {
        units: vec![],
        width: 0.0,
    };
    let mut pending_space: Option<Unit> = None;
    let mut queue: std::collections::VecDeque<Unit> = units.into();

    let flush = |cur: &mut Pending, lines: &mut Vec<Line>, forced: bool| {
        let units = std::mem::take(&mut cur.units);
        cur.width = 0.0;
        lines.push(finish_line(
            db,
            units,
            opts.max_width,
            opts.align,
            !forced,
            &base,
        ));
    };

    while let Some(unit) = queue.pop_front() {
        match unit.kind {
            Kind::Newline => {
                pending_space = None;
                flush(&mut cur, &mut lines, true);
            }
            Kind::Space => {
                if !cur.units.is_empty() {
                    pending_space = Some(unit);
                }
            }
            Kind::Word => {
                let space_w = pending_space.as_ref().map_or(0.0, |s| s.width);
                let fits = cur.width + space_w + unit.width <= opts.max_width + eps;
                if fits {
                    if let Some(s) = pending_space.take() {
                        cur.width += s.width;
                        cur.units.push(s);
                    }
                    cur.width += unit.width;
                    cur.units.push(unit);
                    continue;
                }
                // does not fit: try hyphenation (also for the first word of a line)
                if let (Some(lang), false) = (lang, unit.text.is_empty()) {
                    let avail = opts.max_width
                        - cur.width
                        - if cur.units.is_empty() { 0.0 } else { space_w };
                    let mut best: Option<usize> = None;
                    let mut at = 0;
                    for syl in hypher::hyphenate(&unit.text, lang) {
                        at += syl.len();
                        if at >= unit.text.len() {
                            break;
                        }
                        let (left, _) = split_word(db, &unit, at, &mut Vec::new());
                        if left.width <= avail + eps {
                            best = Some(at);
                        } else {
                            break;
                        }
                    }
                    if let Some(at) = best {
                        let (left, right) = split_word(db, &unit, at, &mut missing);
                        if let Some(s) = pending_space.take() {
                            cur.width += s.width;
                            cur.units.push(s);
                        }
                        cur.width += left.width;
                        cur.units.push(left);
                        flush(&mut cur, &mut lines, false);
                        queue.push_front(right);
                        continue;
                    }
                }
                if cur.units.is_empty() {
                    // an unbreakable word alone on its line: let it overflow (the layout reports it)
                    cur.width += unit.width;
                    cur.units.push(unit);
                } else {
                    pending_space = None;
                    flush(&mut cur, &mut lines, false);
                    queue.push_front(unit);
                }
            }
        }
    }
    if !cur.units.is_empty() || lines.is_empty() {
        flush(&mut cur, &mut lines, true); // last line is never justified
    }

    let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let height = lines.iter().map(|l| l.height).sum();
    Paragraph {
        lines,
        width,
        height,
        missing,
    }
}

/// Min-content (widest unbreakable word) and max-content (everything on one line) widths.
pub fn intrinsic_widths(db: &FontDb, spans: &[Span]) -> (f32, f32) {
    prepare(db, &mut ShapeCache::new(), spans).map_or((0.0, 0.0), |p| p.intrinsic_widths())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(db: &FontDb, size: f32) -> TextStyle {
        TextStyle {
            face: db.select("Inter", 400, false),
            size,
            color: [0, 0, 0, 255],
            line_height: 1.4,
            link: None,
        }
    }
    fn span(db: &FontDb, text: &str, size: f32) -> Span {
        Span {
            text: text.to_string(),
            style: style(db, size),
        }
    }
    fn line_text(l: &Line) -> String {
        l.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn shapes_a_single_line() {
        let db = FontDb::default();
        let p = layout(
            &db,
            &[span(&db, "Hello world", 12.0)],
            &ParagraphOptions::new(500.0),
        );
        assert_eq!(p.lines.len(), 1);
        assert_eq!(line_text(&p.lines[0]), "Hello world");
        assert!(p.width > 40.0 && p.width < 100.0, "width {}", p.width);
        assert!((p.height - 12.0 * 1.4).abs() < 0.5, "height {}", p.height);
        assert!(p.missing.is_empty());
    }

    #[test]
    fn wraps_greedily_and_never_exceeds_width() {
        let db = FontDb::default();
        let text = "The quick brown fox jumps over the lazy dog and keeps running far away";
        let p = layout(&db, &[span(&db, text, 12.0)], &ParagraphOptions::new(120.0));
        assert!(p.lines.len() >= 4, "{} lines", p.lines.len());
        for l in &p.lines {
            assert!(
                l.width <= 120.01,
                "line too wide: {} {:?}",
                l.width,
                line_text(l)
            );
        }
        let joined = p.lines.iter().map(line_text).collect::<Vec<_>>().join(" ");
        assert_eq!(joined, text);
    }

    #[test]
    fn hard_newlines_break_lines() {
        let db = FontDb::default();
        let p = layout(
            &db,
            &[span(&db, "one\ntwo\n\nfour", 12.0)],
            &ParagraphOptions::new(500.0),
        );
        assert_eq!(p.lines.len(), 4);
        assert_eq!(line_text(&p.lines[2]), ""); // blank line keeps its height
        assert!(p.lines[2].height > 10.0);
    }

    #[test]
    fn alignment_moves_the_runs() {
        let db = FontDb::default();
        let spans = [span(&db, "right", 12.0)];
        let mut o = ParagraphOptions::new(200.0);
        o.align = Align::Right;
        let r = layout(&db, &spans, &o);
        assert!((r.lines[0].runs[0].x + r.lines[0].width - 200.0).abs() < 0.01);
        o.align = Align::Center;
        let c = layout(&db, &spans, &o);
        assert!((c.lines[0].runs[0].x - (200.0 - c.lines[0].width) / 2.0).abs() < 0.01);
    }

    #[test]
    fn justify_fills_every_line_but_the_last() {
        let db = FontDb::default();
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
        let mut o = ParagraphOptions::new(150.0);
        o.align = Align::Justify;
        let p = layout(&db, &[span(&db, text, 11.0)], &o);
        assert!(p.lines.len() > 2);
        for l in &p.lines[..p.lines.len() - 1] {
            assert!((l.width - 150.0).abs() < 0.05, "not justified: {}", l.width);
        }
        assert!(p.lines.last().unwrap().width < 150.0);
    }

    #[test]
    fn styled_spans_share_a_line() {
        let db = FontDb::default();
        let bold = TextStyle {
            face: db.select("Inter", 700, false),
            ..style(&db, 12.0)
        };
        let spans = [
            span(&db, "plain ", 12.0),
            Span {
                text: "bold".into(),
                style: bold,
            },
            span(&db, " plain", 12.0),
        ];
        let p = layout(&db, &spans, &ParagraphOptions::new(500.0));
        assert_eq!(p.lines.len(), 1);
        assert!(p.lines[0].runs.len() >= 3, "styles must stay separate runs");
        assert_eq!(line_text(&p.lines[0]), "plain bold plain");
    }

    #[test]
    fn an_unbreakable_word_overflows_instead_of_looping() {
        let db = FontDb::default();
        let p = layout(
            &db,
            &[span(&db, "Supercalifragilisticexpialidocious", 14.0)],
            &ParagraphOptions::new(40.0),
        );
        assert_eq!(p.lines.len(), 1);
        assert!(p.lines[0].width > 40.0);
    }

    #[test]
    fn hyphenation_splits_long_words_when_enabled() {
        let db = FontDb::default();
        let spans = [span(&db, "internationalization international", 12.0)];
        let mut o = ParagraphOptions::new(70.0);
        o.lang = Some("en".into());
        o.hyphenate = true;
        let on = layout(&db, &spans, &o);
        assert!(
            on.lines.iter().any(|l| line_text(l).ends_with('-')),
            "{:?}",
            on.lines.iter().map(line_text).collect::<Vec<_>>()
        );
        for l in &on.lines {
            assert!(l.width <= 70.01);
        }
        o.hyphenate = false;
        let off = layout(&db, &spans, &o);
        assert!(!off.lines.iter().any(|l| line_text(l).ends_with('-')));
    }

    #[test]
    fn reports_missing_glyphs_once() {
        let db = FontDb::default();
        let p = layout(
            &db,
            &[span(&db, "ok \u{10FFFD} \u{10FFFD}", 12.0)],
            &ParagraphOptions::new(500.0),
        );
        assert_eq!(p.missing.len(), 1);
        assert_eq!(p.missing[0].character, '\u{10FFFD}');
    }

    #[test]
    fn accents_and_symbols_are_covered_by_inter() {
        let db = FontDb::default();
        let p = layout(
            &db,
            &[span(&db, "Écrit à Lyon — 12 000 € ✓ œuvre", 12.0)],
            &ParagraphOptions::new(500.0),
        );
        assert!(
            p.missing.iter().all(|m| m.character == '✓'),
            "{:?}",
            p.missing
        ); // only the checkmark may be absent
    }

    #[test]
    fn intrinsic_widths_min_and_max() {
        let db = FontDb::default();
        let (min, max) = intrinsic_widths(&db, &[span(&db, "short extraordinarily", 12.0)]);
        let one = layout(
            &db,
            &[span(&db, "short extraordinarily", 12.0)],
            &ParagraphOptions::new(1000.0),
        );
        assert!((max - one.width).abs() < 0.01);
        assert!(min < max && min > 40.0);
    }

    #[test]
    fn layout_is_deterministic() {
        let db = FontDb::default();
        let spans = [span(
            &db,
            "Determinism matters: same text, same glyphs, same positions.",
            11.0,
        )];
        let a = layout(&db, &spans, &ParagraphOptions::new(180.0));
        let b = layout(&db, &spans, &ParagraphOptions::new(180.0));
        assert_eq!(a, b);
    }
}
