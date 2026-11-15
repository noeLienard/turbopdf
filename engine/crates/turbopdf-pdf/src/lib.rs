//! PDF backend (krilla).
//!
//! Determinism: no wall clock (the creation date is written only if the tree provides one), content-derived
//! identifiers, fonts subsetted by krilla, objects emitted in drawing order. The same [`Layout`] always gives
//! the same bytes.

use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::configure::{Accessibility, ConfigurationBuilder};
use krilla::destination::{Destination, XyzDestination};
use krilla::geom::{Path, PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::{DateTime, Metadata};
use krilla::num::NormalizedF32;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, Stroke, StrokeDash};
use krilla::tagging as kt;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::{Data, Document};
use krilla_svg::{SurfaceExt, SvgSettings};
use std::collections::BTreeMap;
use turbopdf_fonts::{FaceId, FontDb};
use turbopdf_layout::{
    ArtifactKind, BorderSpec, DrawOp, ImageKind, Layout, OutlineEntry, StructRole, Tag, Visual,
};
use turbopdf_text::{Rgba, Run};
use turbopdf_tree::{BorderStyle, Meta, ObjectFit};

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("font error: {0}")]
    Font(String),
    #[error("image error: {0}")]
    Image(String),
    #[error("PDF serialization failed: {0}")]
    Serialize(String),
    /// PDF/UA requirements the document does not meet.
    #[error("not PDF/UA conformant: {}", .0.join("; "))]
    Accessibility(Vec<String>),
}

/// How the PDF is written.
#[derive(Clone, Copy, Debug, Default)]
pub struct PdfOptions {
    /// Tagged PDF (structure tree for assistive technology).
    pub tagged: bool,
    /// Validate PDF/UA-1 (implies `tagged`).
    pub pdfua: bool,
}

/// Same marked-content sequence: artifacts of one kind, or content of one structure element.
fn same_tag(a: Tag, b: Tag) -> bool {
    match (a, b) {
        (Tag::Artifact(x), Tag::Artifact(y)) => x == y,
        (Tag::Content { elem: x, .. }, Tag::Content { elem: y, .. }) => x == y,
        _ => false,
    }
}

fn artifact_type(k: ArtifactKind) -> kt::ArtifactType {
    match k {
        ArtifactKind::Layout => kt::ArtifactType::Layout,
        ArtifactKind::Header => kt::ArtifactType::Header,
        ArtifactKind::Footer => kt::ArtifactType::Footer,
        ArtifactKind::Watermark => kt::ArtifactType::Watermark,
        ArtifactKind::Pagination => kt::ArtifactType::PaginationOther,
    }
}

/// Children of each structure element: (seq, order, node), sorted at the end into reading order.
type Kids = Vec<Vec<(u32, u8, kt::Node)>>;

/// Marks what `draw` paints with `tag`; content becomes a child of its structure element.
fn tagged(
    s: &mut krilla::surface::Surface,
    on: bool,
    tag: Tag,
    text: bool,
    kids: &mut Kids,
    draw: impl FnOnce(&mut krilla::surface::Surface),
) {
    if !on {
        draw(s);
        return;
    }
    match tag {
        Tag::Artifact(k) => {
            s.start_tagged(kt::ContentTag::Artifact(kt::Artifact::with_kind(
                artifact_type(k),
            )));
            draw(s);
            s.end_tagged();
        }
        Tag::Content { elem, seq } => {
            let id = s.start_tagged(if text {
                kt::ContentTag::Span(kt::SpanTag::empty())
            } else {
                kt::ContentTag::Other
            });
            draw(s);
            s.end_tagged();
            kids[elem as usize].push((seq, 1, id.into()));
        }
    }
}

fn tag_kind(role: &StructRole) -> kt::TagKind {
    use kt::Tag as T;
    match role {
        StructRole::Section => T::Section.into(),
        StructRole::P => T::P.into(),
        StructRole::H(level, title) => T::Hn(
            std::num::NonZeroU16::new((*level).max(1) as u16).unwrap(),
            Some(title.clone()),
        )
        .into(),
        StructRole::Figure(alt) => T::Figure(alt.clone()).into(),
        StructRole::Table => T::Table.into(),
        StructRole::Tr => T::TR.into(),
        StructRole::Th => T::TH(kt::TableHeaderScope::Column).into(),
        StructRole::Td => T::TD.into(),
        StructRole::List { ordered } => T::L(if *ordered {
            kt::ListNumbering::Decimal
        } else {
            kt::ListNumbering::Disc
        })
        .into(),
        StructRole::Li => T::LI.into(),
        StructRole::Lbl => T::Lbl.into(),
        StructRole::LBody => T::LBody.into(),
        StructRole::BlockQuote => T::BlockQuote.into(),
        StructRole::Code => T::Code.into(),
        StructRole::Caption => T::Caption.into(),
        StructRole::Toc => T::TOC.into(),
        StructRole::Toci => T::TOCI.into(),
        StructRole::Link => T::Link.into(),
    }
}

/// Structure elements (parents always come before their children) → tag tree in reading order.
fn tag_tree(layout: &Layout, mut kids: Kids, lang: Option<String>) -> kt::TagTree {
    let mut top: Vec<(u32, kt::Node)> = Vec::new();
    for (i, e) in layout.structure.iter().enumerate().rev() {
        let mut children = std::mem::take(&mut kids[i]);
        children.sort_by_key(|(seq, order, _)| (*seq, *order));
        let group = kt::TagGroup::with_children(
            tag_kind(&e.role),
            children.into_iter().map(|(_, _, n)| n).collect(),
        );
        match e.parent {
            Some(p) => kids[p as usize].push((e.seq, 1, group.into())),
            None => top.push((e.seq, group.into())),
        }
    }
    top.sort_by_key(|(seq, _)| *seq);
    let mut tree = kt::TagTree::new().with_lang(lang);
    for (_, n) in top {
        tree.push(n);
    }
    tree
}

/// A krilla validation error, said so that the author knows what to fix.
fn explain(e: &krilla::configure::ValidationError) -> String {
    use krilla::configure::ValidationError as V;
    match e {
        V::NoDocumentLanguage => "the document has no language (meta.lang, <html lang>)".into(),
        V::NoDocumentTitle => "the document has no title (meta.title, <title>)".into(),
        V::MissingAltText(_) => "an image or link has no alt text".into(),
        V::MissingHeadingTitle => "a heading has no text".into(),
        V::MissingDocumentOutline => {
            "the document has no bookmarks: keep `bookmarks` on and add headings".into()
        }
        V::NoCodepointMapping(..) | V::InvalidCodepointMapping(..) | V::UnicodePrivateArea(..) => {
            "a glyph has no Unicode meaning (icon fonts, private-use characters)".into()
        }
        other => format!("{other:?}"),
    }
}

fn fill_of(c: Rgba) -> Fill {
    Fill {
        paint: rgb::Color::new(c[0], c[1], c[2]).into(),
        opacity: NormalizedF32::new(c[3] as f32 / 255.0).unwrap(),
        rule: FillRule::default(),
    }
}

fn rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.0 {
        pb.push_rect(Rect::from_xywh(x, y, w, h)?);
    } else {
        // cubic approximation of a quarter circle
        let k = r * 0.552_284_8;
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
    }
    pb.finish()
}

fn draw_box(
    s: &mut krilla::surface::Surface,
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
    if let Some(bg) = v.background
        && let Some(p) = rect_path(x, y, w, h, v.radius)
    {
        s.set_fill(Some(fill_of(bg)));
        s.set_stroke(None);
        s.draw_path(&p);
    }
    let Some(b) = v.border else { return };
    let bw = b.width;
    if sides.iter().all(|&d| d) {
        // all four sides: ONE stroked outline (4x fewer operations than four bars), inset by half the width
        if let Some(p) = rect_path(
            x + bw / 2.0,
            y + bw / 2.0,
            w - bw,
            h - bw,
            (v.radius - bw / 2.0).max(0.0),
        ) {
            let dash = match b.style {
                BorderStyle::Solid => None,
                BorderStyle::Dashed => Some(StrokeDash {
                    array: vec![bw * 3.0, bw * 2.0],
                    offset: 0.0,
                }),
                BorderStyle::Dotted => Some(StrokeDash {
                    array: vec![bw, bw * 1.5],
                    offset: 0.0,
                }),
            };
            s.set_fill(None);
            s.set_stroke(Some(Stroke {
                paint: rgb::Color::new(b.color[0], b.color[1], b.color[2]).into(),
                width: bw,
                opacity: NormalizedF32::new(b.color[3] as f32 / 255.0).unwrap(),
                dash,
                ..Default::default()
            }));
            s.draw_path(&p);
        }
        return;
    }
    // partial borders: filled bars, so a box cut by a page break can drop its top/bottom edge
    s.set_stroke(None);
    s.set_fill(Some(fill_of(b.color)));
    let bars = [
        (sides[0], x, y, w, bw),
        (sides[1], x + w - bw, y, bw, h),
        (sides[2], x, y + h - bw, w, bw),
        (sides[3], x, y, bw, h),
    ];
    for (on, bx, by, bwid, bh) in bars {
        if on && let Some(p) = rect_path(bx, by, bwid, bh, 0.0) {
            s.draw_path(&p);
        }
    }
}

// ------------------------------------------------------------------ batching
//
// krilla wraps every draw call in its own graphics block (`q … cm … Q` + colour + width), so a table of
// 1 000 cells cost ~150 KB of content stream per page, and compressing it dominated the PDF time.
// Consecutive boxes that do not overlap are therefore drawn as one path per colour, and consecutive text
// runs on the same baseline as one glyph run. Painting order is kept wherever it could be seen.

const MAX_BATCH: usize = 128;

struct BoxItem {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    visual: Visual,
    sides: [bool; 4],
}

fn batchable(v: &Visual, w: f32, h: f32) -> bool {
    w > 0.0 && h > 0.0 && v.radius <= 0.0 && v.border.is_none_or(|b| b.style == BorderStyle::Solid)
}

fn overlaps(a: &BoxItem, b: &BoxItem) -> bool {
    const E: f32 = 0.01;
    a.x + E < b.x + b.w && b.x + E < a.x + a.w && a.y + E < b.y + b.h && b.y + E < a.y + a.h
}

fn group<K: PartialEq>(groups: &mut Vec<(K, PathBuilder)>, key: K) -> &mut PathBuilder {
    match groups.iter().position(|(k, _)| *k == key) {
        Some(i) => &mut groups[i].1,
        None => {
            groups.push((key, PathBuilder::new()));
            &mut groups.last_mut().unwrap().1
        }
    }
}

fn push_rect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pb.push_rect(r);
    }
}

/// Draw non-overlapping boxes: every background first, then every border (bars and outlines).
fn draw_boxes(s: &mut krilla::surface::Surface, items: &[BoxItem]) {
    let mut backgrounds: Vec<(Rgba, PathBuilder)> = Vec::new();
    let mut bars: Vec<(Rgba, PathBuilder)> = Vec::new();
    let mut outlines: Vec<((u32, Rgba), PathBuilder)> = Vec::new();
    for it in items {
        if let Some(bg) = it.visual.background {
            push_rect(group(&mut backgrounds, bg), it.x, it.y, it.w, it.h);
        }
        let Some(BorderSpec {
            width: bw, color, ..
        }) = it.visual.border
        else {
            continue;
        };
        if it.sides.iter().all(|&d| d) {
            push_rect(
                group(&mut outlines, (bw.to_bits(), color)),
                it.x + bw / 2.0,
                it.y + bw / 2.0,
                it.w - bw,
                it.h - bw,
            );
        } else {
            let pb = group(&mut bars, color);
            let (x, y, w, h) = (it.x, it.y, it.w, it.h);
            for (on, bx, by, bwid, bh) in [
                (it.sides[0], x, y, w, bw),
                (it.sides[1], x + w - bw, y, bw, h),
                (it.sides[2], x, y + h - bw, w, bw),
                (it.sides[3], x, y, bw, h),
            ] {
                if on {
                    push_rect(pb, bx, by, bwid, bh);
                }
            }
        }
    }
    s.set_stroke(None);
    for (c, pb) in backgrounds.into_iter().chain(bars) {
        if let Some(p) = pb.finish() {
            s.set_fill(Some(fill_of(c)));
            s.draw_path(&p);
        }
    }
    s.set_fill(None);
    for ((bw, c), pb) in outlines {
        if let Some(p) = pb.finish() {
            s.set_stroke(Some(Stroke {
                paint: rgb::Color::new(c[0], c[1], c[2]).into(),
                width: f32::from_bits(bw),
                opacity: NormalizedF32::new(c[3] as f32 / 255.0).unwrap(),
                ..Default::default()
            }));
            s.draw_path(&p);
        }
    }
}

/// Text runs merged into one draw call.
struct TextBatch {
    x: f32,
    baseline: f32,
    end: f32,
    face: FaceId,
    size: f32,
    color: Rgba,
    text: String,
    glyphs: Vec<KrillaGlyph>,
}

impl TextBatch {
    fn new(x: f32, baseline: f32, run: &Run) -> Self {
        let mut b = TextBatch {
            x,
            baseline,
            end: x,
            face: run.face,
            size: run.size,
            color: run.color,
            text: String::new(),
            glyphs: Vec::new(),
        };
        b.append(x, run);
        b
    }

    fn accepts(&self, x: f32, baseline: f32, run: &Run) -> bool {
        run.face == self.face
            && run.size == self.size
            && run.color == self.color
            && baseline == self.baseline
            && x >= self.end - 0.01
    }

    fn append(&mut self, x: f32, run: &Run) {
        if let Some(last) = self.glyphs.last_mut() {
            // jump to the next run by widening the last glyph's advance
            last.x_advance += (x - self.end) / self.size;
        }
        let base = self.text.len();
        self.text.push_str(&run.text);
        let mut w = 0.0;
        self.glyphs.extend(run.glyphs.iter().map(|g| {
            w += g.advance_em;
            KrillaGlyph {
                glyph_id: GlyphId::new(g.id as u32),
                text_range: base + g.range.start..base + g.range.end,
                x_advance: g.advance_em,
                x_offset: g.x_offset_em,
                y_offset: g.y_offset_em,
                y_advance: 0.0,
                location: None,
            }
        }));
        self.end = x + w * self.size;
    }
}

fn parse_date(iso: &str) -> Option<DateTime> {
    let (date, time) = iso.split_once('T').unwrap_or((iso, "00:00:00"));
    let mut d = date.split('-');
    let year: u16 = d.next()?.parse().ok()?;
    let month: u8 = d.next().and_then(|m| m.parse().ok()).unwrap_or(1);
    let day: u8 = d.next().and_then(|m| m.parse().ok()).unwrap_or(1);
    let time = time.trim_end_matches('Z');
    let mut t = time.split(':');
    let hour: u8 = t.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let minute: u8 = t.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let second: u8 = t
        .next()
        .and_then(|v| v.split(['.', '+', '-']).next()?.parse().ok())
        .unwrap_or(0);
    Some(
        DateTime::new(year)
            .month(month)
            .day(day)
            .hour(hour)
            .minute(minute)
            .second(second),
    )
}

fn metadata(meta: Option<&Meta>) -> Metadata {
    let mut m = Metadata::new()
        .creator("TurboPDF".to_string())
        .producer("TurboPDF".to_string());
    if let Some(meta) = meta {
        if let Some(v) = &meta.title {
            m = m.title(v.clone())
        }
        if let Some(v) = &meta.author {
            m = m.authors(vec![v.clone()])
        }
        if let Some(v) = &meta.subject {
            m = m.description(v.clone())
        }
        if let Some(v) = &meta.keywords {
            m = m.keywords(v.clone())
        }
        if let Some(v) = &meta.lang {
            m = m.language(v.clone())
        }
        if let Some(d) = meta.creation_date.as_deref().and_then(parse_date) {
            m = m.creation_date(d)
        }
    }
    m
}

fn link_target(layout: &Layout, target: &str) -> Option<Target> {
    if let Some(name) = target.strip_prefix('#') {
        let a = layout.anchors.get(name)?;
        return Some(Target::Destination(Destination::Xyz(XyzDestination::new(
            a.page,
            Point::from_xy(a.x, a.y),
        ))));
    }
    turbopdf_layout::is_safe_external_link(target)
        .then(|| Target::Action(Action::Link(LinkAction::new(target.trim().to_string()))))
}

/// Flat bookmarks (with levels) → nested outline nodes.
fn outline_nodes(entries: &[OutlineEntry], i: &mut usize, level: u8) -> Vec<OutlineNode> {
    let mut out = Vec::new();
    while *i < entries.len() && entries[*i].level >= level {
        let e = &entries[*i];
        *i += 1;
        let mut node = OutlineNode::new(
            e.title.clone(),
            XyzDestination::new(e.page, Point::from_xy(0.0, e.y)),
        );
        for child in outline_nodes(entries, i, e.level + 1) {
            node.push_child(child);
        }
        out.push(node);
    }
    out
}

/// Serialize a layout to PDF bytes.
pub fn write(
    layout: &Layout,
    db: &FontDb,
    meta: Option<&Meta>,
    opts: PdfOptions,
) -> Result<Vec<u8>, PdfError> {
    let tag_on = opts.tagged || opts.pdfua;
    let mut settings = krilla::SerializeSettings {
        enable_tagging: tag_on,
        ..Default::default()
    };
    if opts.pdfua {
        settings.configuration = ConfigurationBuilder::new()
            .with_accessibility_validator(Accessibility::UA1)
            .finish()
            .map_err(|e| PdfError::Serialize(format!("{e:?}")))?;
    }
    let mut kids: Kids = vec![Vec::new(); layout.structure.len()];
    let mut doc = Document::new_with(settings);
    doc.set_metadata(metadata(meta));
    if !layout.outline.is_empty() {
        let mut outline = Outline::new();
        let top = layout.outline.iter().map(|e| e.level).min().unwrap_or(1);
        let mut i = 0;
        while i < layout.outline.len() {
            for node in outline_nodes(&layout.outline, &mut i, top) {
                outline.push_child(node);
            }
            if i < layout.outline.len() && layout.outline[i].level < top {
                i += 1;
            }
        }
        doc.set_outline(outline);
    } else if opts.pdfua {
        // PDF/UA wants bookmarks: a document without headings gets one, its title, to the first page
        let title = meta
            .and_then(|m| m.title.clone())
            .unwrap_or_else(|| "Document".into());
        let mut outline = Outline::new();
        outline.push_child(OutlineNode::new(
            title,
            XyzDestination::new(0, Point::from_xy(0.0, 0.0)),
        ));
        doc.set_outline(outline);
    }

    let mut fonts: BTreeMap<FaceId, Font> = BTreeMap::new();
    let mut images: BTreeMap<usize, Image> = BTreeMap::new();

    for page in &layout.pages {
        let mut p = doc.start_page_with(
            PageSettings::from_wh(page.width, page.height)
                .ok_or_else(|| PdfError::Serialize("invalid page size".into()))?,
        );
        let mut s = p.surface();
        let ops = &page.ops;
        let tag_of = |i: usize| {
            page.tags
                .get(i)
                .copied()
                .unwrap_or(Tag::Artifact(ArtifactKind::Layout))
        };
        let mut i = 0;
        while i < ops.len() {
            let tag = tag_of(i);
            match &ops[i] {
                DrawOp::Box { .. } => {
                    let mut batch: Vec<BoxItem> = Vec::new();
                    let mut single = None;
                    while let Some(DrawOp::Box {
                        x,
                        y,
                        w,
                        h,
                        visual,
                        sides,
                    }) = ops.get(i)
                    {
                        if !same_tag(tag_of(i), tag) {
                            break;
                        }
                        let item = BoxItem {
                            x: *x,
                            y: *y,
                            w: *w,
                            h: *h,
                            visual: *visual,
                            sides: *sides,
                        };
                        if !batchable(visual, *w, *h) {
                            if batch.is_empty() {
                                single = Some(item);
                                i += 1;
                            }
                            break;
                        }
                        if batch.len() >= MAX_BATCH || batch.iter().any(|b| overlaps(b, &item)) {
                            break;
                        }
                        batch.push(item);
                        i += 1;
                    }
                    tagged(&mut s, tag_on, tag, false, &mut kids, |s| match single {
                        Some(b) => draw_box(s, b.x, b.y, b.w, b.h, &b.visual, b.sides),
                        None => draw_boxes(s, &batch),
                    });
                }
                DrawOp::Text { x, baseline, run } => {
                    let mut batch = TextBatch::new(*x, *baseline, run);
                    i += 1;
                    while let Some(DrawOp::Text { x, baseline, run }) = ops.get(i) {
                        if !same_tag(tag_of(i), tag) || !batch.accepts(*x, *baseline, run) {
                            break;
                        }
                        batch.append(*x, run);
                        i += 1;
                    }
                    if batch.glyphs.is_empty() {
                        continue;
                    }
                    let font = match fonts.get(&batch.face) {
                        Some(f) => f.clone(),
                        None => {
                            let f = Font::new(Data::from(db.face(batch.face).data.clone()), 0)
                                .ok_or_else(|| {
                                    PdfError::Font(db.face(batch.face).family.clone())
                                })?;
                            fonts.insert(batch.face, f.clone());
                            f
                        }
                    };
                    tagged(&mut s, tag_on, tag, true, &mut kids, |s| {
                        s.set_stroke(None);
                        s.set_fill(Some(fill_of(batch.color)));
                        s.draw_glyphs(
                            Point::from_xy(batch.x, batch.baseline),
                            &batch.glyphs,
                            font,
                            &batch.text,
                            batch.size,
                            false,
                        );
                    });
                }
                DrawOp::Rotated {
                    cx,
                    cy,
                    angle,
                    x,
                    baseline,
                    run,
                } => {
                    i += 1;
                    let batch = TextBatch::new(*x, *baseline, run);
                    if batch.glyphs.is_empty() {
                        continue;
                    }
                    let font = match fonts.get(&batch.face) {
                        Some(f) => f.clone(),
                        None => {
                            let f = Font::new(Data::from(db.face(batch.face).data.clone()), 0)
                                .ok_or_else(|| {
                                    PdfError::Font(db.face(batch.face).family.clone())
                                })?;
                            fonts.insert(batch.face, f.clone());
                            f
                        }
                    };
                    // y points down: a counter-clockwise turn on the page is a negative angle
                    tagged(&mut s, tag_on, tag, true, &mut kids, |s| {
                        s.push_transform(&Transform::from_translate(*cx, *cy));
                        s.push_transform(&Transform::from_rotate(-angle));
                        s.set_stroke(None);
                        s.set_fill(Some(fill_of(batch.color)));
                        s.draw_glyphs(
                            Point::from_xy(batch.x, batch.baseline),
                            &batch.glyphs,
                            font,
                            &batch.text,
                            batch.size,
                            false,
                        );
                        s.pop();
                        s.pop();
                    });
                }
                DrawOp::Image {
                    x,
                    y,
                    w,
                    h,
                    image,
                    fit,
                } => {
                    i += 1;
                    let data = &layout.images[*image];
                    let img = match (&data.svg, images.get(image)) {
                        (Some(_), _) => None,
                        (None, Some(i)) => Some(i.clone()),
                        (None, None) => {
                            let d = Data::from(data.bytes.clone());
                            let i = match data.kind {
                                ImageKind::Png => Image::from_png(d, true),
                                ImageKind::Jpeg => Image::from_jpeg(d, true),
                                ImageKind::Webp => Image::from_webp(d, true),
                                ImageKind::Gif => Image::from_gif(d, true),
                                ImageKind::Svg => unreachable!("SVGs are drawn as vectors"),
                            }
                            .map_err(PdfError::Image)?;
                            images.insert(*image, i.clone());
                            Some(i)
                        }
                    };
                    let (nw, nh) = data.natural;
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
                    let clip = matches!(fit, ObjectFit::Cover | ObjectFit::None);
                    tagged(&mut s, tag_on, tag, false, &mut kids, |s| {
                        let clipped = clip
                            && rect_path(*x, *y, *w, *h, 0.0)
                                .map(|c| s.push_clip_path(&c, &FillRule::default()))
                                .is_some();
                        s.push_transform(&Transform::from_translate(ox, oy));
                        if let Some(size) = Size::from_wh(dw, dh) {
                            match (img, &data.svg) {
                                (Some(img), _) => s.draw_image(img, size),
                                (None, Some(tree)) => {
                                    s.draw_svg(tree, size, SvgSettings::default());
                                }
                                (None, None) => {}
                            }
                        }
                        s.pop();
                        if clipped {
                            s.pop();
                        }
                    });
                }
            }
        }
        s.finish();
        for r in &page.links {
            let url = &layout.links[r.link as usize];
            let (Some(target), Some(rect)) = (
                link_target(layout, url),
                Rect::from_xywh(r.x, r.y, r.w, r.h),
            ) else {
                continue;
            };
            // alt of the annotation: where it goes
            let alt = match url.strip_prefix('#') {
                Some(_) => layout
                    .anchors
                    .get(&url[1..])
                    .map(|a| format!("Go to page {}", a.page + 1)),
                None => Some(url.trim().to_string()),
            };
            let annotation = Annotation::new_link(LinkAnnotation::new(rect, target), alt);
            match r.elem.filter(|_| tag_on) {
                Some(e) => {
                    let id = p.add_tagged_annotation(annotation);
                    // the annotation comes first in its Link element
                    kids[e as usize].push((layout.structure[e as usize].seq, 0, id.into()));
                }
                None => p.add_annotation(annotation),
            }
        }
        p.finish();
    }

    if tag_on {
        doc.set_tag_tree(tag_tree(layout, kids, meta.and_then(|m| m.lang.clone())));
    }
    doc.finish().map_err(|e| match e {
        krilla::error::KrillaError::Validation(list) => {
            use krilla::configure::ValidationError as V;
            let mut missing = String::new();
            let mut msgs: Vec<String> = Vec::new();
            for (v, _) in &list {
                match v {
                    V::ContainsNotDefGlyph(_, _, text) => {
                        for c in text.chars() {
                            if !missing.contains(c) {
                                missing.push(c);
                            }
                        }
                    }
                    other => {
                        let m = explain(other);
                        if !msgs.contains(&m) {
                            msgs.push(m);
                        }
                    }
                }
            }
            if !missing.is_empty() {
                let list: Vec<String> = missing.chars().map(String::from).collect();
                msgs.insert(
                    0,
                    format!(
                        "characters in no font: {} (add a font that has them)",
                        list.join(" ")
                    ),
                );
            }
            PdfError::Accessibility(msgs)
        }
        other => PdfError::Serialize(format!("{other:?}")),
    })
}
