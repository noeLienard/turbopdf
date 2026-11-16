//! Section by section layout, then the final pages with header, footer and page numbers.

use crate::build::{
    Cx, TocEntry, fill_page_tokens, layout_box, plain_text, substitute_page_tokens,
};
use crate::paginate::{Op, Paginator, Structure};
use crate::style::{Inherited, inherit, rgba};
use crate::{
    Anchor, ArtifactKind, DrawOp, Layout, LayoutError, LinkRect, OutlineEntry, PageLayout, Tag,
    Visual,
};
use std::collections::{BTreeMap, BTreeSet};
use turbopdf_fonts::{FontDb, ResourceLoader};
use turbopdf_text as text;
use turbopdf_tree as t;

/// Headings of a body, in order, with the anchor their node gets (its `id`, or its JSON Pointer).
/// Views are followed; tables, headers and footers are not.
fn collect_headings(nodes: &[t::Node], base: &str, out: &mut Vec<TocEntry>) {
    for (i, n) in nodes.iter().enumerate() {
        let pointer = format!("{base}/children/{i}");
        match n {
            t::Node::Heading(h) => {
                let title = plain_text(&h.content).trim().to_string();
                if !title.is_empty() {
                    out.push(TocEntry {
                        level: h.level,
                        title,
                        anchor: h.id.clone().unwrap_or(pointer),
                    });
                }
            }
            t::Node::View(v) => collect_headings(&v.children, &pointer, out),
            _ => {}
        }
    }
}

/// The watermark of a section, centered on the page.
fn watermark_ops(db: &FontDb, w: &t::Watermark, pw: f32, ph: f32) -> Vec<DrawOp> {
    let face = db.select(
        w.font_family.as_deref().unwrap_or("Inter"),
        w.font_weight.unwrap_or(700),
        false,
    );
    let color = w.color.map(rgba).unwrap_or([0, 0, 0, 31]);
    let shape = |size: f32| {
        let style = text::TextStyle {
            face,
            fallback: None,
            size,
            color,
            line_height: 1.0,
            link: None,
            effects: text::Effects::default(),
        };
        text::layout(
            db,
            &[text::Span {
                text: w.text.clone(),
                style,
            }],
            &text::ParagraphOptions::new(1.0e6),
        )
    };
    let size = w.font_size.unwrap_or_else(|| {
        let width = shape(100.0).width.max(1.0);
        (0.75 * (pw * pw + ph * ph).sqrt() / width * 100.0).min(200.0)
    });
    let p = shape(size);
    let Some(line) = p.lines.first() else {
        return Vec::new();
    };
    let angle = w.angle.unwrap_or(45.0);
    line.runs
        .iter()
        .map(|r| DrawOp::Rotated {
            cx: pw / 2.0,
            cy: ph / 2.0,
            angle,
            x: -line.width / 2.0 + r.x,
            baseline: line.baseline - line.height / 2.0,
            run: text::Run {
                x: 0.0,
                ..r.clone()
            },
        })
        .collect()
}

fn page_size(p: &t::Page) -> (f32, f32) {
    let (w, h) = match p.size {
        None | Some(t::PageSize::Format(t::PageFormat::A4)) => (595.28, 841.89),
        Some(t::PageSize::Format(t::PageFormat::A3)) => (841.89, 1190.55),
        Some(t::PageSize::Format(t::PageFormat::A5)) => (419.53, 595.28),
        Some(t::PageSize::Format(t::PageFormat::Letter)) => (612.0, 792.0),
        Some(t::PageSize::Format(t::PageFormat::Legal)) => (612.0, 1008.0),
        Some(t::PageSize::Custom { width, height }) => (width, height),
    };
    // custom sizes are taken as given; orientation only swaps named formats
    let named = !matches!(p.size, Some(t::PageSize::Custom { .. }));
    if named && p.orientation == Some(t::Orientation::Landscape) {
        (h, w)
    } else {
        (w, h)
    }
}

/// Deep copy of `nodes` with `{page}` / `{pages}` replaced in text. `targets` gives the page of a
/// `pageNumber` target (absent while measuring).
fn substitute(
    nodes: &[t::Node],
    page: u32,
    pages: u32,
    targets: Option<&BTreeMap<String, Anchor>>,
) -> Vec<t::Node> {
    fn content(c: &t::TextContent, page: u32, pages: u32) -> t::TextContent {
        match c {
            t::TextContent::Plain(s) => t::TextContent::Plain(fill_page_tokens(s, page, pages)),
            t::TextContent::Spans(v) => t::TextContent::Spans(
                v.iter()
                    .map(|s| t::Span {
                        text: fill_page_tokens(&s.text, page, pages),
                        style: s.style.clone(),
                        link: s.link.clone(),
                    })
                    .collect(),
            ),
        }
    }
    nodes
        .iter()
        .map(|n| match n {
            t::Node::Text(x) => t::Node::Text(t::Text {
                content: content(&x.content, page, pages),
                ..x.clone()
            }),
            t::Node::Heading(x) => t::Node::Heading(t::Heading {
                content: content(&x.content, page, pages),
                ..x.clone()
            }),
            t::Node::PageNumber(p) => {
                let shown = match (&p.target, targets) {
                    (None, _) => {
                        fill_page_tokens(p.format.as_deref().unwrap_or("{page}"), page, pages)
                    }
                    (Some(_), None) => {
                        fill_page_tokens(p.format.as_deref().unwrap_or("{page}"), 888, pages)
                    }
                    (Some(tg), Some(map)) => match map.get(tg) {
                        Some(a) => fill_page_tokens(
                            p.format.as_deref().unwrap_or("{page}"),
                            a.page as u32 + 1,
                            pages,
                        ),
                        None => substitute_page_tokens(
                            p.format.as_deref().unwrap_or("{page}"),
                            "?",
                            &pages.to_string(),
                        ),
                    },
                };
                t::Node::Text(t::Text {
                    id: p.id.clone(),
                    style: p.style.clone(),
                    keep_together: None,
                    role: None,
                    content: t::TextContent::Plain(shown),
                })
            }
            t::Node::View(v) => t::Node::View(t::View {
                children: substitute(&v.children, page, pages, targets),
                ..v.clone()
            }),
            t::Node::Table(tb) => {
                let rows = |rs: &[t::Row]| -> Vec<t::Row> {
                    rs.iter()
                        .map(|r| t::Row {
                            cells: r
                                .cells
                                .iter()
                                .map(|c| t::Cell {
                                    children: substitute(&c.children, page, pages, targets),
                                    ..c.clone()
                                })
                                .collect(),
                            ..r.clone()
                        })
                        .collect()
                };
                t::Node::Table(t::Table {
                    header: tb.header.as_deref().map(rows),
                    rows: rows(&tb.rows),
                    ..tb.clone()
                })
            }
            other => other.clone(),
        })
        .collect()
}

struct Section<'a> {
    page: &'a t::Page,
    index: usize,
    width: f32,
    height: f32,
    margin: [f32; 4], // top right bottom left
    header_h: f32,
    footer_h: f32,
    inh: Inherited,
    body: Vec<Vec<Op>>,
}

pub fn run(
    doc: &t::Document,
    db: &FontDb,
    loader: &dyn ResourceLoader,
) -> Result<Layout, LayoutError> {
    let lang = doc.meta.as_ref().and_then(|m| m.lang.clone());
    let mut toc = Vec::new();
    for (si, page) in doc.children.iter().enumerate() {
        collect_headings(&page.children, &format!("/children/{si}"), &mut toc);
    }
    let mut cx = Cx {
        db,
        loader,
        images: Vec::new(),
        warnings: Vec::new(),
        lang: lang.clone(),
        hyphenate: lang.is_some(),
        shapes: text::ShapeCache::new(),
        links: Vec::new(),
        toc,
        svg_fonts: None,
        pdfua: doc.pdfua == Some(true),
    };
    let bookmarks = doc.bookmarks.unwrap_or(true);
    let doc_inh = inherit(&Inherited::default(), doc.default_style.as_ref());
    let column = t::Style::default();

    let mut sections: Vec<Section> = Vec::new();
    let mut page_count = 0u32;
    let mut structure = Structure::default();
    for (si, page) in doc.children.iter().enumerate() {
        let (pw, ph) = page_size(page);
        let m = page.margin.map_or([36.0; 4], |e| e.resolve());
        let inh = inherit(&doc_inh, page.style.as_ref());
        let band_w = (pw - m[1] - m[3]).max(1.0);

        // header / footer are reserved once, measured with the widest plausible page numbers
        let mut measure = |nodes: &Option<Vec<t::Node>>, key: &str| -> f32 {
            match nodes.as_deref() {
                Some(n) if !n.is_empty() => {
                    layout_box(
                        &mut cx,
                        &column,
                        &substitute(n, 88, 88, None),
                        &format!("/children/{si}/{key}"),
                        band_w,
                        &inh,
                    )
                    .h
                }
                _ => 0.0,
            }
        };
        let header_h = measure(&page.header, "header");
        let footer_h = measure(&page.footer, "footer");

        let body_w = band_w;
        let body_h = ph - m[0] - m[2] - header_h - footer_h;
        if body_h < 20.0 {
            return Err(LayoutError::Invalid(format!(
                "page {si}: header, footer and margins leave no room for the body ({body_h:.1}pt)"
            )));
        }
        let root = layout_box(
            &mut cx,
            &column,
            &page.children,
            &format!("/children/{si}"),
            body_w,
            &inh,
        );
        let mut pag = Paginator::new(body_h, page_count + 1);
        pag.structure = std::mem::take(&mut structure);
        pag.place_children(root.kind_children(), 0.0, root.inset[0]);
        structure = std::mem::take(&mut pag.structure);
        page_count += pag.pages.len() as u32;
        cx.warnings.append(&mut pag.warnings);
        sections.push(Section {
            page,
            index: si,
            width: pw,
            height: ph,
            margin: m,
            header_h,
            footer_h,
            inh,
            body: pag.pages,
        });
    }

    // ---- where every node with an id starts (first occurrence wins)
    let mut anchors: BTreeMap<String, Anchor> = BTreeMap::new();
    let mut index = 0usize;
    for s in &sections {
        let (ox, oy) = (s.margin[3], s.margin[0] + s.header_h);
        for ops in &s.body {
            for op in ops {
                if let Op::Anchor { name, x, y } = op {
                    anchors.entry(name.clone()).or_insert(Anchor {
                        page: index,
                        x: x + ox,
                        y: y + oy,
                    });
                }
            }
            index += 1;
        }
    }

    // ---- final pages
    let mut pages = Vec::new();
    let mut warnings = Vec::new();
    let mut outline = Vec::new();
    let mut broken: BTreeSet<String> = BTreeSet::new();
    let mut number = 0u32;
    for mut s in sections {
        let band_w = (s.width - s.margin[1] - s.margin[3]).max(1.0);
        let body = std::mem::take(&mut s.body);
        let stamp = s
            .page
            .watermark
            .as_ref()
            .map(|w| watermark_ops(db, w, s.width, s.height))
            .unwrap_or_default();
        for body_ops in body {
            number += 1;
            let mut ops: Vec<DrawOp> = Vec::new();
            let mut tags: Vec<Tag> = Vec::new();
            let mut links: Vec<LinkRect> = Vec::new();
            if let Some(bg) = s
                .page
                .style
                .as_ref()
                .and_then(|st| st.background_color)
                .filter(|c| c.a > 0)
            {
                tags.push(Tag::Artifact(ArtifactKind::Layout));
                ops.push(DrawOp::Box {
                    x: 0.0,
                    y: 0.0,
                    w: s.width,
                    h: s.height,
                    visual: Visual {
                        background: Some([bg.r, bg.g, bg.b, bg.a]),
                        ..Default::default()
                    },
                    sides: [false; 4],
                });
            }
            for (nodes, key, y) in [
                (&s.page.header, "header", s.margin[0]),
                (
                    &s.page.footer,
                    "footer",
                    s.height - s.margin[2] - s.footer_h,
                ),
            ] {
                let Some(list) = nodes.as_deref().filter(|n| !n.is_empty()) else {
                    continue;
                };
                let band = layout_box(
                    &mut cx,
                    &t::Style::default(),
                    &substitute(list, number, page_count, Some(&anchors)),
                    &format!("/children/{}/{key}", s.index),
                    band_w,
                    &s.inh,
                );
                let reserved = if key == "header" {
                    s.header_h
                } else {
                    s.footer_h
                };
                let mut pag = Paginator::new(reserved.max(band.h), number);
                // headers and footers repeat on every page: not part of the reading order
                pag.artifact = Some(if key == "header" {
                    ArtifactKind::Header
                } else {
                    ArtifactKind::Footer
                });
                pag.emit_whole(&band, 0.0, 0.0);
                let mut w = std::mem::take(&mut pag.warnings);
                if band.h > reserved + 0.5 {
                    w.push(t::Warning::Overflow {
                        path: band.path.clone(),
                        page: number,
                        dx: 0.0,
                        dy: band.h - reserved,
                    });
                }
                warnings.append(&mut w);
                for op in pag.pages.remove(0) {
                    match op {
                        Op::Draw(d, tag) => {
                            ops.push(translate(d, s.margin[3], y));
                            tags.push(tag);
                        }
                        Op::Link(r) => links.push(LinkRect {
                            x: r.x + s.margin[3],
                            y: r.y + y,
                            ..r
                        }),
                        _ => {} // anchors and bookmarks of headers / footers would repeat on every page
                    }
                }
            }
            let (ox, oy) = (s.margin[3], s.margin[0] + s.header_h);
            for op in body_ops {
                match op {
                    Op::Draw(d, tag) => {
                        ops.push(translate(d, ox, oy));
                        tags.push(tag);
                    }
                    Op::Link(r) => links.push(LinkRect {
                        x: r.x + ox,
                        y: r.y + oy,
                        ..r
                    }),
                    Op::Anchor { .. } => {}
                    Op::Heading {
                        level, title, y, ..
                    } => {
                        if bookmarks {
                            outline.push(OutlineEntry {
                                level,
                                title,
                                page: number as usize - 1,
                                y: y + oy,
                            });
                        }
                    }
                    Op::Dynamic {
                        x,
                        y,
                        w,
                        text: d,
                        tag,
                    } => {
                        let (x, y, w) = (&x, &y, &w);
                        let shown = match &d.target {
                            None => fill_page_tokens(&d.format, number, page_count),
                            Some(tg) => match anchors.get(tg) {
                                Some(a) => {
                                    fill_page_tokens(&d.format, a.page as u32 + 1, page_count)
                                }
                                None => {
                                    if broken.insert(format!("#{tg}")) {
                                        warnings.push(t::Warning::BrokenLink {
                                            path: format!("#{tg}"),
                                            page: number,
                                            target: tg.clone(),
                                        });
                                    }
                                    substitute_page_tokens(&d.format, "?", &page_count.to_string())
                                }
                            },
                        };
                        let spans = crate::style::spans(
                            db,
                            &t::TextContent::Plain(shown),
                            &d.inh,
                            &mut Vec::new(),
                        );
                        let mut o = text::ParagraphOptions::new(*w);
                        o.align = d.inh.align;
                        let p = text::layout(db, &spans, &o);
                        let mut ly = *y;
                        for l in &p.lines {
                            for run in &l.runs {
                                let mut r = run.clone();
                                let rx = *x + r.x;
                                r.x = 0.0;
                                let baseline = ly + l.baseline + oy - r.effects.rise;
                                if let Some(line) = crate::decoration_box(rx + ox, baseline, &r) {
                                    ops.push(line);
                                    tags.push(Tag::Artifact(ArtifactKind::Layout));
                                }
                                ops.push(DrawOp::Text {
                                    x: rx + ox,
                                    baseline,
                                    run: r,
                                });
                                tags.push(tag);
                            }
                            ly += l.height;
                        }
                    }
                }
            }
            ops.extend(stamp.iter().cloned());
            tags.extend(stamp.iter().map(|_| Tag::Artifact(ArtifactKind::Watermark)));
            debug_assert_eq!(ops.len(), tags.len());
            for r in &links {
                let target = &cx.links[r.link as usize];
                if let Some(name) = target.strip_prefix('#')
                    && !anchors.contains_key(name)
                    && broken.insert(target.clone())
                {
                    warnings.push(t::Warning::BrokenLink {
                        path: target.clone(),
                        page: number,
                        target: name.to_string(),
                    });
                }
            }
            pages.push(PageLayout {
                width: s.width,
                height: s.height,
                ops,
                tags,
                links,
            });
        }
    }

    cx.warnings.append(&mut warnings);
    Ok(Layout {
        pages,
        images: cx.images,
        warnings: cx.warnings,
        links: cx.links,
        anchors,
        outline,
        structure: structure.elems,
    })
}

fn translate(op: DrawOp, dx: f32, dy: f32) -> DrawOp {
    match op {
        DrawOp::Box {
            x,
            y,
            w,
            h,
            visual,
            sides,
        } => DrawOp::Box {
            x: x + dx,
            y: y + dy,
            w,
            h,
            visual,
            sides,
        },
        DrawOp::Text { x, baseline, run } => DrawOp::Text {
            x: x + dx,
            baseline: baseline + dy,
            run,
        },
        DrawOp::Image {
            x,
            y,
            w,
            h,
            image,
            fit,
        } => DrawOp::Image {
            x: x + dx,
            y: y + dy,
            w,
            h,
            image,
            fit,
        },
        rotated @ DrawOp::Rotated { .. } => rotated,
    }
}
