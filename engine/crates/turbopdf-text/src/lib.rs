//! Text shaping (rustybuzz), line breaking and hyphenation (hypher).
//!
//! Deterministic by construction: only the fonts handed in are used, iteration order is the
//! input order, line breaking is greedy, no locale / clock / system font is ever consulted.

use rustybuzz::{Direction, Script, ShapePlan, UnicodeBuffer};
use turbopdf_fonts::{FaceId, FontDb};

mod breaking;
mod shape;

use breaking::*;
pub use shape::ShapeCache;
use shape::*;

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
mod tests;
