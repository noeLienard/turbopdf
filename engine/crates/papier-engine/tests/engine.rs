//! End-to-end tests of the engine: tree in, layout / PDF / warnings out.
//!
//! Layout assertions read the positioned drawing operations (`Engine::layout`), which is what the PDF and
//! PNG backends draw; PDF assertions check the bytes themselves.

use base64::Engine as _;
use papier_engine::papier_fonts::{FontDb, ResourceLoader};
use papier_engine::papier_layout::{DrawOp, Layout};
use papier_engine::papier_tree::{Document, MoveReason, Warning};
use papier_engine::{Engine, EngineError};
use serde_json::{Value, json};
use std::sync::Arc;

// ------------------------------------------------------------------ helpers

fn doc(v: Value) -> Document {
    serde_json::from_value(v).expect("valid tree JSON")
}

/// A one-section document (A4, default margins) with these body nodes.
fn body(children: Value) -> Document {
    doc(
        json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": children }] }),
    )
}

fn layout(d: &Document) -> (Layout, FontDb) {
    Engine::new().layout(d).expect("layout")
}

/// Text of each page, whitespace removed (runs may or may not carry the spaces between words).
fn page_texts(l: &Layout) -> Vec<String> {
    l.pages
        .iter()
        .map(|p| {
            p.ops
                .iter()
                .filter_map(|o| match o {
                    DrawOp::Text { run, .. } => Some(run.text.as_str()),
                    _ => None,
                })
                .collect::<String>()
                .split_whitespace()
                .collect()
        })
        .collect()
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect()
}

/// Pages (1-based) whose text contains `needle`.
fn pages_with(l: &Layout, needle: &str) -> Vec<usize> {
    let n = squash(needle);
    page_texts(l)
        .iter()
        .enumerate()
        .filter(|(_, t)| t.contains(&n))
        .map(|(i, _)| i + 1)
        .collect()
}

fn text(s: &str) -> Value {
    json!({ "type": "text", "content": s })
}

fn cell(s: &str) -> Value {
    json!({ "children": [text(s)] })
}

/// A table of `n` rows: "R{i}" | "T{i}", with a "Ref | Total" header.
fn table(n: usize, extra: Value) -> Value {
    let rows: Vec<Value> = (0..n)
        .map(|i| json!({ "cells": [cell(&format!("R{i:03}x")), cell(&format!("T{i:03}y"))] }))
        .collect();
    let mut t = json!({
        "type": "table",
        "columns": [{ "width": "auto" }, { "width": 100 }],
        "header": [{ "cells": [cell("Ref"), cell("Total")] }],
        "cellStyle": { "padding": 3 },
        "rows": rows,
    });
    t.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    t
}

const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn inter(file: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../papier-fonts/assets/{file}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

// ------------------------------------------------------------------ one test per node type

#[test]
fn document_and_page_produce_a_pdf() {
    let out = Engine::new()
        .render(&body(json!([text("Bonjour")])))
        .unwrap();
    assert!(out.pdf.starts_with(b"%PDF-"));
    assert_eq!(out.pages, 1);
    assert!(out.warnings.is_empty());
}

#[test]
fn page_size_orientation_and_margin() {
    let d = doc(json!({ "type": "document", "version": 1, "children": [
        { "type": "page", "size": "A5", "orientation": "landscape", "margin": 10, "children": [text("x")] }
    ]}));
    let (l, _) = layout(&d);
    let p = &l.pages[0];
    assert!(p.width > p.height, "landscape");
    assert!(
        (p.height - 419.53).abs() < 0.5,
        "A5 short side, got {}",
        p.height
    );
    let DrawOp::Text { x, .. } = p
        .ops
        .iter()
        .find(|o| matches!(o, DrawOp::Text { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert!((x - 10.0).abs() < 0.01, "margin 10 → text at x=10, got {x}");
}

#[test]
fn text_is_laid_out_and_wraps() {
    let long = "mot ".repeat(200);
    let (l, _) = layout(&body(json!([text(&long)])));
    let baselines: Vec<f32> = l.pages[0]
        .ops
        .iter()
        .filter_map(|o| {
            if let DrawOp::Text { baseline, .. } = o {
                Some(*baseline)
            } else {
                None
            }
        })
        .collect();
    let mut lines = baselines.clone();
    lines.dedup();
    assert!(lines.len() > 3, "200 words must wrap on several lines");
    assert_eq!(page_texts(&l)[0], squash(&long));
}

#[test]
fn text_spans_keep_their_style() {
    let d = body(
        json!([{ "type": "text", "content": [{ "text": "normal " }, { "text": "gras", "style": { "fontWeight": 700 } }] }]),
    );
    let (l, db) = layout(&d);
    let weights: Vec<(String, u16)> = l.pages[0]
        .ops
        .iter()
        .filter_map(|o| {
            if let DrawOp::Text { run, .. } = o {
                Some((run.text.trim().to_string(), db.face(run.face).weight))
            } else {
                None
            }
        })
        .collect();
    assert!(
        weights.iter().any(|(t, w)| t.contains("gras") && *w == 700),
        "{weights:?}"
    );
    assert!(
        weights
            .iter()
            .any(|(t, w)| t.contains("normal") && *w == 400),
        "{weights:?}"
    );
}

#[test]
fn view_draws_its_background_and_lays_out_children_in_a_row() {
    let d = body(
        json!([{ "type": "view", "style": { "flexDirection": "row", "backgroundColor": "#ff0000", "padding": 5 }, "children": [text("gauche"), text("droite")] }]),
    );
    let (l, _) = layout(&d);
    let ops = &l.pages[0].ops;
    assert!(ops.iter().any(
        |o| matches!(o, DrawOp::Box { visual, .. } if visual.background == Some([255, 0, 0, 255]))
    ));
    let xs: Vec<f32> = ops
        .iter()
        .filter_map(|o| {
            if let DrawOp::Text { x, .. } = o {
                Some(*x)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(xs.len(), 2);
    assert!(
        xs[1] > xs[0] + 10.0,
        "row: second child to the right of the first ({xs:?})"
    );
}

#[test]
fn heading_uses_its_level_size() {
    let (l, _) = layout(&body(
        json!([{ "type": "heading", "level": 1, "content": "Titre" }, { "type": "heading", "level": 3, "content": "Sous" }]),
    ));
    let sizes: Vec<f32> = l.pages[0]
        .ops
        .iter()
        .filter_map(|o| {
            if let DrawOp::Text { run, .. } = o {
                Some(run.size)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(sizes, vec![28.0, 18.0]);
}

#[test]
fn image_is_drawn() {
    let d = body(
        json!([{ "type": "image", "src": { "data": PNG_1X1 }, "format": "png", "style": { "width": 50, "height": 20 } }]),
    );
    let (l, _) = layout(&d);
    assert!(l.pages[0].ops.iter().any(|o| matches!(o, DrawOp::Image { w, h, .. } if (*w - 50.0).abs() < 0.01 && (*h - 20.0).abs() < 0.01)));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert!(
        Engine::new()
            .render(&d)
            .unwrap()
            .pdf
            .windows(8)
            .any(|w| w == b"/XObject")
    );
}

#[test]
fn table_cells_are_laid_out_in_columns() {
    let (l, _) = layout(&body(json!([table(3, json!({}))])));
    let t = &page_texts(&l)[0];
    assert!(t.starts_with("RefTotalR000xT000yR001x"), "{t}");
}

#[test]
fn page_break_starts_a_new_page() {
    let (l, _) = layout(&body(
        json!([text("avant"), { "type": "page_break" }, text("après")]),
    ));
    assert_eq!(l.pages.len(), 2);
    assert_eq!(pages_with(&l, "avant"), vec![1]);
    assert_eq!(pages_with(&l, "après"), vec![2]);
}

#[test]
fn page_number_in_footer_counts_every_page() {
    let d = doc(json!({ "type": "document", "version": 1, "children": [{
        "type": "page",
        "footer": [{ "type": "page_number", "format": "Page {page} / {pages}" }],
        "children": [text("un"), { "type": "page_break" }, text("deux"), { "type": "page_break" }, text("trois")],
    }]}));
    let (l, _) = layout(&d);
    assert_eq!(l.pages.len(), 3);
    for p in 1..=3 {
        assert_eq!(pages_with(&l, &format!("Page {p} / 3")), vec![p]);
    }
}

#[test]
fn header_tokens_and_page_number_in_body() {
    let d = doc(json!({ "type": "document", "version": 1, "children": [{
        "type": "page",
        "header": [text("Rapport — {page}/{pages}")],
        "children": [{ "type": "page_number", "format": "corps {page}" }, { "type": "page_break" }, text("fin")],
    }]}));
    let (l, _) = layout(&d);
    assert_eq!(pages_with(&l, "Rapport — 1/2"), vec![1]);
    assert_eq!(pages_with(&l, "Rapport — 2/2"), vec![2]);
    assert_eq!(pages_with(&l, "corps 1"), vec![1]);
}

// ------------------------------------------------------------------ table pagination

#[test]
fn long_table_never_splits_a_row_and_repeats_its_header() {
    let (l, _) = layout(&body(json!([table(200, json!({}))])));
    assert!(
        l.pages.len() >= 3,
        "200 rows need several pages, got {}",
        l.pages.len()
    );
    let texts = page_texts(&l);
    let mut last_page = 1;
    for i in 0..200 {
        let r = pages_with(&l, &format!("R{i:03}x"));
        let t = pages_with(&l, &format!("T{i:03}y"));
        assert_eq!(r.len(), 1, "row {i} drawn once");
        assert_eq!(r, t, "row {i}: both cells on the same page");
        assert!(r[0] >= last_page, "rows stay in order");
        last_page = r[0];
    }
    for (p, t) in texts.iter().enumerate() {
        assert!(
            t.starts_with("RefTotal"),
            "header repeated at the top of page {}",
            p + 1
        );
    }
    // nothing drawn below the bottom margin
    for p in &l.pages {
        for o in &p.ops {
            if let DrawOp::Box { y, h, .. } = o {
                assert!(
                    y + h <= p.height - 36.0 + 0.01,
                    "box ends at {} on a {}pt page",
                    y + h,
                    p.height
                );
            }
        }
    }
}

#[test]
fn header_is_not_repeated_when_disabled() {
    let (l, _) = layout(&body(json!([table(200, json!({ "repeatHeader": false }))])));
    assert!(l.pages.len() >= 2);
    assert_eq!(pages_with(&l, "RefTotal"), vec![1]);
}

#[test]
fn table_with_keep_together_moves_as_a_block() {
    let filler = json!({ "type": "view", "style": { "height": 700 }, "children": [] });
    let (l, _) = layout(&body(json!([
        filler,
        table(5, json!({ "keepTogether": true }))
    ])));
    assert_eq!(pages_with(&l, "Ref"), vec![2]);
    assert_eq!(pages_with(&l, "R004x"), vec![2]);
    assert!(
        l.warnings.iter().any(|w| matches!(
            w,
            Warning::MovedToNextPage {
                reason: MoveReason::KeepTogether,
                ..
            }
        )),
        "{:?}",
        l.warnings
    );
}

// ------------------------------------------------------------------ keepTogether

fn block(keep: bool) -> Value {
    let lines: Vec<Value> = (0..8).map(|i| text(&format!("ligne{i}"))).collect();
    json!({ "type": "view", "id": "bloc", "keepTogether": keep, "children": lines })
}

#[test]
fn keep_together_moves_the_whole_block() {
    let filler = json!({ "type": "view", "style": { "height": 700 }, "children": [] });
    let (l, _) = layout(&body(json!([filler, block(true)])));
    for i in 0..8 {
        assert_eq!(pages_with(&l, &format!("ligne{i}")), vec![2], "line {i}");
    }
    assert!(l.warnings.iter().any(|w| matches!(w, Warning::MovedToNextPage { path, from_page: 1, to_page: 2, reason: MoveReason::KeepTogether } if path == "bloc")), "{:?}", l.warnings);
}

#[test]
fn without_keep_together_the_block_is_split() {
    let filler = json!({ "type": "view", "style": { "height": 700 }, "children": [] });
    let (l, _) = layout(&body(json!([filler, block(false)])));
    assert_eq!(pages_with(&l, "ligne0"), vec![1]);
    assert_eq!(pages_with(&l, "ligne7"), vec![2]);
}

#[test]
fn heading_is_kept_with_the_next_block() {
    let filler = json!({ "type": "view", "style": { "height": 735 }, "children": [] });
    let (l, _) = layout(&body(
        json!([filler, { "type": "heading", "level": 2, "content": "Chapitre" }, text(&"suite ".repeat(80))]),
    ));
    assert_eq!(pages_with(&l, "Chapitre"), vec![2]);
    // ordinary typography: no warning unless keepWithNext was asked for
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    let asked =
        json!({ "type": "heading", "level": 2, "keepWithNext": true, "content": "Chapitre" });
    let (l, _) = layout(&body(json!([filler, asked, text(&"suite ".repeat(80))])));
    assert_eq!(pages_with(&l, "Chapitre"), vec![2]);
    assert!(
        l.warnings.iter().any(|w| matches!(
            w,
            Warning::MovedToNextPage {
                reason: MoveReason::KeepWithNext,
                ..
            }
        )),
        "{:?}",
        l.warnings
    );
}

#[test]
fn ordinary_text_flow_gives_no_warning() {
    // 120 one-line paragraphs, a long paragraph and a long table over several pages: nothing to report
    let paras: Vec<_> = (0..120).map(|i| text(&format!("Paragraphe {i}"))).collect();
    let rows: Vec<_> = (0..150)
        .map(|i| json!({ "cells": [{ "children": [text(&format!("r{i}"))] }] }))
        .collect();
    let mut kids = paras;
    kids.push(text(&"mot ".repeat(3000)));
    kids.push(json!({ "type": "table", "columns": [{ "width": "auto" }], "rows": rows }));
    let (l, _) = layout(&body(json!(kids)));
    assert!(l.pages.len() > 3);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

// ------------------------------------------------------------------ fonts

struct MapLoader(Vec<(&'static str, Vec<u8>)>);
impl ResourceLoader for MapLoader {
    fn load(&self, path: &str) -> Result<Vec<u8>, String> {
        self.0
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| b.clone())
            .ok_or_else(|| format!("not found: {path}"))
    }
}

fn families(l: &Layout, db: &FontDb) -> Vec<String> {
    l.pages[0]
        .ops
        .iter()
        .filter_map(|o| {
            if let DrawOp::Text { run, .. } = o {
                Some(db.face(run.face).family.clone())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn inter_is_embedded_and_used_by_default() {
    let (l, db) = layout(&body(
        json!([text("défaut"), { "type": "text", "style": { "fontWeight": 700, "fontStyle": "italic" }, "content": "gras italique" }]),
    ));
    assert!(families(&l, &db).iter().all(|f| f == "Inter"));
    let faces: Vec<(u16, bool)> = l.pages[0]
        .ops
        .iter()
        .filter_map(|o| {
            if let DrawOp::Text { run, .. } = o {
                let f = db.face(run.face);
                Some((f.weight, f.italic))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(faces.first(), Some(&(400, false)));
    assert_eq!(faces.last(), Some(&(700, true)));
}

#[test]
fn font_loaded_from_a_file() {
    let d = doc(json!({ "type": "document", "version": 1,
        "fonts": [{ "family": "Marque", "weight": 400, "source": { "file": "fonts/marque.ttf" } }],
        "children": [{ "type": "page", "children": [{ "type": "text", "style": { "fontFamily": "Marque" }, "content": "Logo" }] }] }));
    let engine = Engine::with_loader(Arc::new(MapLoader(vec![(
        "fonts/marque.ttf",
        inter("Inter-Bold.ttf"),
    )])));
    let (l, db) = engine.layout(&d).unwrap();
    assert_eq!(families(&l, &db), vec!["Marque"]);
    assert!(engine.render(&d).unwrap().pdf.starts_with(b"%PDF-"));
}

#[test]
fn font_loaded_from_base64_data() {
    let data = base64::engine::general_purpose::STANDARD.encode(inter("Inter-Bold.ttf"));
    let d = doc(json!({ "type": "document", "version": 1,
        "fonts": [{ "family": "Marque", "weight": 400, "source": { "data": data } }],
        "children": [{ "type": "page", "children": [{ "type": "text", "style": { "fontFamily": "Marque" }, "content": "Logo" }] }] }));
    let (l, db) = layout(&d);
    assert_eq!(families(&l, &db), vec!["Marque"]);
}

#[test]
fn font_file_without_loader_is_a_clear_error() {
    let d = doc(json!({ "type": "document", "version": 1,
        "fonts": [{ "family": "Marque", "weight": 400, "source": { "file": "fonts/marque.ttf" } }],
        "children": [{ "type": "page", "children": [text("x")] }] }));
    match Engine::new().render(&d) {
        Err(EngineError::Font { family, .. }) => assert_eq!(family, "Marque"),
        other => panic!("expected a font error, got {:?}", other.map(|o| o.pages)),
    }
}

#[test]
fn invalid_font_bytes_are_a_clear_error() {
    let d = doc(json!({ "type": "document", "version": 1,
        "fonts": [{ "family": "Cassée", "weight": 400, "source": { "data": "AAAA" } }],
        "children": [{ "type": "page", "children": [text("x")] }] }));
    assert!(matches!(
        Engine::new().render(&d),
        Err(EngineError::Font { .. })
    ));
}

#[test]
fn unknown_family_falls_back_to_inter() {
    let (l, db) = layout(&body(
        json!([{ "type": "text", "style": { "fontFamily": "Inexistante" }, "content": "repli" }]),
    ));
    assert_eq!(families(&l, &db), vec!["Inter"]);
}

#[test]
fn fonts_are_subsetted() {
    let pdf = Engine::new()
        .render(&body(json!([text("Bonjour")])))
        .unwrap()
        .pdf;
    let full = inter("Inter-Regular.ttf").len();
    assert!(
        pdf.len() < full / 4,
        "PDF is {} bytes, Inter alone is {full}: the font must be subsetted",
        pdf.len()
    );
}

// ------------------------------------------------------------------ warnings

#[test]
fn warning_overflow_for_a_word_wider_than_its_box() {
    let d = body(
        json!([{ "type": "view", "id": "etroit", "style": { "width": 30 }, "children": [text("Anticonstitutionnellement")] }]),
    );
    let out = Engine::new().render(&d).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| matches!(w, Warning::Overflow { dx, page: 1, .. } if *dx > 0.0)),
        "{:?}",
        out.warnings
    );
}

#[test]
fn warning_overflow_for_a_row_taller_than_a_page() {
    let tall =
        json!({ "children": [{ "type": "view", "style": { "height": 2000 }, "children": [] }] });
    let d = body(
        json!([{ "type": "table", "columns": [{ "width": "auto" }], "rows": [{ "cells": [tall] }] }]),
    );
    let out = Engine::new().render(&d).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| matches!(w, Warning::Overflow { dy, .. } if *dy > 1000.0)),
        "{:?}",
        out.warnings
    );
}

#[test]
fn warning_moved_to_next_page_for_an_atomic_image() {
    let filler = json!({ "type": "view", "style": { "height": 700 }, "children": [] });
    let img = json!({ "type": "image", "id": "photo", "src": { "data": PNG_1X1 }, "format": "png", "style": { "width": 100, "height": 100 } });
    let out = Engine::new().render(&body(json!([filler, img]))).unwrap();
    assert_eq!(out.pages, 2);
    assert!(out.warnings.iter().any(|w| matches!(w, Warning::MovedToNextPage { path, reason: MoveReason::Atomic, .. } if path == "photo")), "{:?}", out.warnings);
}

#[test]
fn warning_missing_glyph() {
    let out = Engine::new()
        .render(&body(json!([text("prix 漢")])))
        .unwrap();
    assert!(
        out.warnings.iter().any(|w| matches!(w, Warning::MissingGlyph { character, codepoint: 0x6F22, page: 1, .. } if character == "漢")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn warning_image_failed_does_not_stop_the_render() {
    let d = body(json!([
        { "type": "image", "id": "casse", "src": { "data": "AAAA" }, "format": "png" },
        { "type": "image", "id": "absente", "src": { "file": "images/x.png" }, "format": "png" },
        text("la suite est rendue"),
    ]));
    let out = Engine::new().render(&d).unwrap();
    let failed: Vec<&str> = out
        .warnings
        .iter()
        .filter_map(|w| {
            if let Warning::ImageFailed { path, .. } = w {
                Some(path.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(failed, vec!["casse", "absente"]);
    assert!(out.pdf.starts_with(b"%PDF-"));
}

#[test]
fn clean_document_has_no_warnings() {
    let tree = std::fs::read_to_string(format!(
        "{}/../../fixtures/invoice.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let out = Engine::new()
        .render(&serde_json::from_str(&tree).unwrap())
        .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

// ------------------------------------------------------------------ validation & determinism

#[test]
fn invalid_tree_is_refused_with_its_path() {
    let d = body(json!([{ "type": "heading", "level": 9, "content": "x" }]));
    match Engine::new().render(&d) {
        Err(EngineError::Invalid(errors)) => {
            assert_eq!(errors[0].path, "/children/0/children/0/level")
        }
        other => panic!(
            "expected a validation error, got {:?}",
            other.map(|o| o.pages)
        ),
    }
}

#[test]
fn same_tree_gives_the_same_bytes() {
    let tree = std::fs::read_to_string(format!(
        "{}/../../fixtures/invoice_200.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let d: Document = serde_json::from_str(&tree).unwrap();
    let a = Engine::new().render(&d).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100)); // a clock would now read differently
    let b = Engine::new().render(&d).unwrap();
    assert_eq!(a.pages, 3, "the 200-row invoice fits on 3 pages");
    assert!(a.pdf == b.pdf, "two renders of the same tree differ");
    assert!(
        !a.pdf.windows(13).any(|w| w == b"/CreationDate"),
        "no creation date unless the tree gives one"
    );
}

// ------------------------------------------------------------------ links, bookmarks, table of contents

fn pdf_text(pdf: &[u8]) -> String {
    String::from_utf8_lossy(pdf).into_owned()
}

#[test]
fn links_on_spans_and_boxes_become_clickable_areas() {
    let d = body(json!([
        { "type": "text", "content": [{ "text": "Voir " }, { "text": "le site", "link": "https://example.com/doc" }] },
        { "type": "view", "link": "#annexe", "style": { "height": 30, "backgroundColor": "#eeeeee" }, "children": [text("Aller à l'annexe")] },
        { "type": "page_break" },
        { "type": "heading", "id": "annexe", "level": 2, "content": "Annexe" },
    ]));
    let (l, _) = layout(&d);
    assert_eq!(l.pages[0].links.len(), 2, "{:?}", l.pages[0].links);
    let span = l.pages[0]
        .links
        .iter()
        .find(|r| l.links[r.link as usize] == "https://example.com/doc")
        .expect("span link");
    assert!(span.w > 10.0 && span.h > 5.0);
    let boxed = l.pages[0]
        .links
        .iter()
        .find(|r| l.links[r.link as usize] == "#annexe")
        .expect("box link");
    assert!((boxed.h - 30.0).abs() < 0.01);
    assert_eq!(l.anchors["annexe"].page, 1);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);

    let pdf = pdf_text(&Engine::new().render(&d).unwrap().pdf);
    assert!(pdf.contains("/URI(https://example.com/doc)"), "URI action");
    assert_eq!(pdf.matches("/Subtype/Link").count(), 2);
}

#[test]
fn a_link_to_a_missing_id_is_reported_and_unsafe_schemes_are_not_clickable() {
    let d = body(json!([{ "type": "text", "content": [
        { "text": "nulle part", "link": "#nope" },
        { "text": " script", "link": "javascript:alert(1)" },
    ] }]));
    let out = Engine::new().render(&d).unwrap();
    assert_eq!(
        out.warnings,
        vec![Warning::BrokenLink {
            path: "#nope".into(),
            page: 1,
            target: "nope".into()
        }]
    );
    let pdf = pdf_text(&out.pdf);
    assert!(!pdf.contains("javascript"));
    assert!(!pdf.contains("/Subtype/Link"));
}

#[test]
fn headings_become_nested_bookmarks() {
    let d = body(json!([
        { "type": "heading", "level": 1, "content": "Un" },
        { "type": "heading", "level": 2, "content": "Un point un" },
        { "type": "page_break" },
        { "type": "heading", "level": 1, "content": "Deux" },
        { "type": "heading", "level": 4, "content": "trop profond" },
    ]));
    let (l, _) = layout(&d);
    let got: Vec<_> = l
        .outline
        .iter()
        .map(|e| (e.level, e.title.as_str(), e.page))
        .collect();
    assert_eq!(
        got,
        vec![(1, "Un", 0), (2, "Un point un", 0), (1, "Deux", 1)]
    );
    let pdf = pdf_text(&Engine::new().render(&d).unwrap().pdf);
    assert!(pdf.contains("/Outlines"));

    let mut off = d.clone();
    off.bookmarks = Some(false);
    assert!(layout(&off).0.outline.is_empty());
    assert!(!pdf_text(&Engine::new().render(&off).unwrap().pdf).contains("/Outlines"));
}

#[test]
fn table_of_contents_lists_headings_with_their_pages_and_links_to_them() {
    let d = body(json!([
        { "type": "table_of_contents", "id": "toc", "levels": 2 },
        { "type": "page_break" },
        { "type": "heading", "level": 1, "content": "Introduction" },
        { "type": "heading", "level": 2, "id": "contexte", "content": "Contexte" },
        { "type": "heading", "level": 3, "content": "Détail" },
        { "type": "page_break" },
        { "type": "heading", "level": 1, "content": "Conclusion" },
    ]));
    let (l, _) = layout(&d);
    let first = &page_texts(&l)[0];
    assert!(first.contains("Introduction2"), "{first}");
    assert!(first.contains("Contexte2"), "{first}");
    assert!(first.contains("Conclusion3"), "{first}");
    assert!(!first.contains("Détail"), "level 3 is deeper than `levels`");
    let targets: Vec<_> = l.pages[0]
        .links
        .iter()
        .map(|r| l.links[r.link as usize].as_str())
        .collect();
    assert!(targets.contains(&"#contexte"), "{targets:?}");
    assert_eq!(targets.len(), 3);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn page_number_with_a_target_gives_the_page_of_that_node() {
    let d = body(json!([
        { "type": "page_number", "format": "voir page {page}", "target": "tarifs" },
        { "type": "page_break" },
        text("deux"),
        { "type": "page_break" },
        { "type": "view", "id": "tarifs", "children": [text("Tarifs")] },
        { "type": "page_number", "format": "cible {page}", "target": "absent" },
    ]));
    let (l, _) = layout(&d);
    assert!(
        page_texts(&l)[0].contains("voirpage3"),
        "{:?}",
        page_texts(&l)
    );
    assert!(page_texts(&l)[2].contains("cible?"));
    assert!(
        matches!(&l.warnings[..], [Warning::BrokenLink { target, .. }] if target == "absent"),
        "{:?}",
        l.warnings
    );
}

// ------------------------------------------------------------------ SVG, WebP, GIF

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40" viewBox="0 0 120 40"><rect width="120" height="40" rx="6" fill="#2563eb"/><circle cx="20" cy="20" r="10" fill="#fff"/><text x="40" y="26" font-size="16" fill="#fff">Logo</text><image href="file:///etc/passwd" width="1" height="1"/></svg>"##;
const WEBP_1X1: &str = "UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==";
const GIF_1X1: &str = "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

#[test]
fn svg_images_keep_their_size_and_are_drawn_as_vectors() {
    let d = body(json!([{ "type": "image", "src": { "data": b64(SVG) } }]));
    let (l, _) = layout(&d);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(l.images[0].natural, (90.0, 30.0)); // 120 × 40 px
    let img = l.pages[0].ops.iter().find_map(|o| match o {
        DrawOp::Image { w, h, .. } => Some((*w, *h)),
        _ => None,
    });
    assert_eq!(img, Some((90.0, 30.0)));
    let out = Engine::new().render(&d).unwrap();
    let pdf = pdf_text(&out.pdf);
    assert!(!pdf.contains("/Subtype/Image"), "vector, not a bitmap");
    assert!(!pdf.contains("passwd"));
    let png = Engine::new().preview(&d, 1.0).unwrap();
    assert_eq!(png.len(), 1);
}

#[test]
fn webp_and_gif_images_are_supported() {
    let d = body(json!([
        { "type": "image", "src": { "data": WEBP_1X1 }, "style": { "width": 20, "height": 20 } },
        { "type": "image", "src": { "data": GIF_1X1 }, "style": { "width": 20, "height": 20 } },
    ]));
    let out = Engine::new().render(&d).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    // each image, plus the soft mask of the one with transparency
    assert!(pdf_text(&out.pdf).matches("/Subtype/Image").count() >= 2);
    assert_eq!(Engine::new().preview(&d, 1.0).unwrap().len(), 1);
}

#[test]
fn invalid_svg_is_an_image_failed_warning() {
    let d = body(json!([{ "type": "image", "id": "logo", "src": { "data": b64("<svg><rect") } }]));
    let out = Engine::new().render(&d).unwrap();
    assert!(
        matches!(&out.warnings[..], [Warning::ImageFailed { path, reason }] if path == "logo" && reason.contains("SVG")),
        "{:?}",
        out.warnings
    );
}

// ------------------------------------------------------------------ position: absolute, watermark

#[test]
fn absolute_children_are_placed_in_their_parent_and_leave_the_flow() {
    let d = body(json!([
        { "type": "view", "style": { "height": 100, "backgroundColor": "#eeeeee" }, "children": [
            text("Facture"),
            { "type": "view", "style": { "position": "absolute", "top": 10, "right": 10, "width": 60, "height": 20, "backgroundColor": "#dc2626" }, "children": [text("PAYÉ")] },
            text("Ligne suivante"),
        ] },
    ]));
    let (l, _) = layout(&d);
    let boxes: Vec<_> = l.pages[0]
        .ops
        .iter()
        .filter_map(|o| match o {
            DrawOp::Box {
                x, y, w, h, visual, ..
            } if visual.background == Some([0xdc, 0x26, 0x26, 255]) => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect();
    // A4 595.28 wide, margin 36: parent spans 36..559.28
    assert_eq!(boxes.len(), 1);
    let (x, y, w, h) = boxes[0];
    assert!(
        (x - (559.28 - 10.0 - 60.0)).abs() < 0.05
            && (y - 46.0).abs() < 0.05
            && w == 60.0
            && h == 20.0,
        "{boxes:?}"
    );
    // the stamp is drawn after (above) the text of the parent, and does not push "Ligne suivante" down
    let ys: Vec<_> = l.pages[0]
        .ops
        .iter()
        .filter_map(|o| match o {
            DrawOp::Text { baseline, run, .. } => Some((run.text.clone(), *baseline)),
            _ => None,
        })
        .collect();
    let facture = ys.iter().find(|(t, _)| t.contains("Facture")).unwrap().1;
    let suivante = ys.iter().find(|(t, _)| t.contains("Ligne")).unwrap().1;
    assert!(suivante - facture < 20.0, "{ys:?}");
    assert!(
        ys.iter().position(|(t, _)| t.contains("PAY")).unwrap()
            > ys.iter().position(|(t, _)| t.contains("Ligne")).unwrap()
    );
}

#[test]
fn watermark_is_stamped_on_every_page() {
    let d = doc(json!({ "type": "document", "version": 1, "children": [{
        "type": "page",
        "watermark": { "text": "BROUILLON" },
        "children": [text("un"), { "type": "page_break" }, text("deux")],
    }] }));
    let (l, _) = layout(&d);
    for p in &l.pages {
        let w: Vec<_> = p
            .ops
            .iter()
            .filter_map(|o| match o {
                DrawOp::Rotated {
                    angle, run, cx, cy, ..
                } => Some((*angle, run.text.clone(), *cx, *cy, run.color)),
                _ => None,
            })
            .collect();
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].0, 45.0);
        assert_eq!(w[0].1, "BROUILLON");
        assert!((w[0].2 - 297.64).abs() < 0.01 && (w[0].3 - 420.945).abs() < 0.01);
        assert_eq!(w[0].4[3], 31);
        assert!(
            matches!(p.ops.last(), Some(DrawOp::Rotated { .. })),
            "drawn above the content"
        );
    }
    Engine::new().render(&d).unwrap();
    assert_eq!(Engine::new().preview(&d, 0.5).unwrap().len(), 2);
}

#[test]
fn new_features_stay_deterministic() {
    let d = body(json!([
        { "type": "table_of_contents" },
        { "type": "heading", "level": 1, "content": [{ "text": "Titre ", "link": "https://example.com" }] },
        { "type": "image", "src": { "data": b64(SVG) } },
        { "type": "image", "src": { "data": WEBP_1X1 }, "style": { "width": 10 } },
    ]));
    let a = Engine::new().render(&d).unwrap().pdf;
    let b = Engine::new().render(&d).unwrap().pdf;
    assert_eq!(a, b);
}

// ------------------------------------------------------------------ tagged PDF, PDF/UA

fn count(pdf: &[u8], needle: &str) -> usize {
    pdf_text(pdf).matches(needle).count()
}

fn accessible_doc(extra: Value) -> Document {
    let mut v = json!({
        "type": "document", "version": 1, "meta": { "title": "Rapport", "lang": "fr" },
        "children": [{ "type": "page", "footer": [{ "type": "page_number" }], "children": [
            { "type": "heading", "level": 1, "content": "Rapport" },
            { "type": "text", "content": [{ "text": "Voir " }, { "text": "le site", "link": "https://example.com" }] },
            { "type": "view", "role": "list", "children": [
                { "type": "view", "role": "list_item", "style": { "flexDirection": "row" }, "children": [
                    { "type": "text", "role": "list_label", "content": "•" },
                    { "type": "view", "role": "list_body", "children": [text("Premier point")] },
                ]},
            ]},
            { "type": "table", "columns": [{ "width": "auto" }, { "width": 80 }],
              "header": [{ "cells": [{ "children": [text("Mois")] }, { "children": [text("CA")] }] }],
              "rows": [{ "cells": [{ "children": [text("Octobre")] }, { "children": [text("12")] }] }] },
            { "type": "image", "alt": "Logo Acme", "src": { "data": GIF_1X1 }, "format": "gif", "style": { "width": 20 } },
            { "type": "image", "alt": "", "src": { "data": GIF_1X1 }, "format": "gif", "style": { "width": 20 } },
        ]}],
    });
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    doc(v)
}

#[test]
fn documents_are_tagged_with_their_logical_structure() {
    let out = Engine::new().render(&accessible_doc(json!({}))).unwrap();
    let pdf = &out.pdf;
    assert_eq!(count(pdf, "/StructTreeRoot"), 2, "root + catalog entry");
    assert!(pdf_text(pdf).contains("/Marked true"));
    for (role, n) in [
        ("/S/H1", 1),
        ("/S/L/", 1),
        ("/S/LI", 1),
        ("/S/Lbl", 1),
        ("/S/LBody", 1),
        ("/S/Table", 1),
        ("/S/TR", 2),
        ("/S/TH", 2),
        ("/S/TD", 2),
        ("/S/Link", 1),
    ] {
        assert_eq!(count(pdf, role), n, "{role}");
    }
    // the logo is a figure with its alt; the decorative image is not in the structure
    assert_eq!(count(pdf, "/S/Figure"), 1);
    assert!(pdf_text(pdf).contains("/Alt(Logo Acme)"));
    // the heading carries its title (needed by PDF/UA) and the document its language
    assert!(pdf_text(pdf).contains("/T(Rapport)"));
    assert!(pdf_text(pdf).contains("/Lang(fr)"));
    // the link annotation is part of the structure
    assert!(pdf_text(pdf).contains("/StructParent"));
}

#[test]
fn tagged_false_writes_no_structure() {
    let out = Engine::new()
        .render(&accessible_doc(json!({ "tagged": false })))
        .unwrap();
    assert_eq!(count(&out.pdf, "/StructTreeRoot"), 0);
    assert_eq!(count(&out.pdf, "/S/P"), 0);
}

#[test]
fn a_header_repeated_on_every_page_is_read_once() {
    let rows: Vec<Value> = (0..120)
        .map(|i| json!({ "cells": [{ "children": [text(&format!("ligne {i}"))] }] }))
        .collect();
    let d = body(
        json!([{ "type": "table", "columns": [{ "width": "auto" }], "header": [{ "cells": [{ "children": [text("Titre")] }] }], "rows": rows }]),
    );
    let out = Engine::new().render(&d).unwrap();
    assert!(out.pages >= 2);
    assert_eq!(count(&out.pdf, "/S/TH"), 1);
    assert_eq!(count(&out.pdf, "/S/TD"), 120);
}

#[test]
fn pdfua_documents_validate() {
    let out = Engine::new()
        .render(&accessible_doc(json!({ "pdfua": true })))
        .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(pdf_text(&out.pdf).contains("pdfuaid:part"));
    assert!(pdf_text(&out.pdf).contains("/DisplayDocTitle true"));
}

#[test]
fn pdfua_needs_a_title_and_a_language() {
    let mut d = accessible_doc(json!({ "pdfua": true }));
    d.meta = None;
    let Err(EngineError::Invalid(errors)) = Engine::new().render(&d) else {
        panic!("expected invalid")
    };
    let codes: Vec<&str> = errors.iter().map(|e| e.code).collect();
    assert_eq!(codes, ["pdfua_title", "pdfua_lang"]);
}

#[test]
fn pdfua_images_without_alt_become_decorative_with_a_warning() {
    let mut v = serde_json::to_value(accessible_doc(json!({ "pdfua": true }))).unwrap();
    v["children"][0]["children"].as_array_mut().unwrap().push(
        json!({ "type": "image", "id": "photo", "src": { "data": GIF_1X1 }, "format": "gif" }),
    );
    let out = Engine::new().render(&doc(v)).unwrap();
    assert!(
        matches!(&out.warnings[..], [Warning::MissingAlt { path, page: 1 }] if path == "photo"),
        "{:?}",
        out.warnings
    );
    assert_eq!(count(&out.pdf, "/S/Figure"), 1);
}

#[test]
fn pdfua_refuses_characters_no_font_has() {
    let mut v = serde_json::to_value(accessible_doc(json!({ "pdfua": true }))).unwrap();
    v["children"][0]["children"]
        .as_array_mut()
        .unwrap()
        .push(text("漢字"));
    match Engine::new().render(&doc(v)) {
        Err(EngineError::Pdf(e)) => assert!(
            e.to_string().contains("characters in no font: 漢 字"),
            "{e}"
        ),
        other => panic!("expected a PDF/UA error, got {other:?}"),
    }
}

#[test]
fn an_absolute_child_may_cover_its_parents_padding() {
    let d = body(
        json!([{ "type": "view", "style": { "height": 80, "padding": { "right": 100 } }, "children": [
            text("contenu"),
            { "type": "view", "style": { "position": "absolute", "top": 0, "right": 0, "width": 90, "height": 80, "backgroundColor": "#fde68a" }, "children": [] },
        ]}]),
    );
    let out = Engine::new().render(&d).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn html_preview_is_real_text_with_working_links() {
    let d = body(json!([
        { "type": "text", "content": [{ "text": "Voir " }, { "text": "la suite", "link": "#suite" }, { "text": " & <plus>" }] },
        { "type": "text", "content": [{ "text": "site", "link": "https://exemple.fr" }] },
        { "type": "text", "content": [{ "text": "unsafe", "link": "javascript:alert(1)" }] },
        { "type": "page_break" },
        { "type": "text", "id": "suite", "style": { "fontWeight": 700 }, "content": "Suite" },
    ]));
    let p = Engine::new().preview_html(&d).unwrap();
    assert_eq!(p.pages.len(), 2);
    let (p1, p2) = (&p.pages[0].html, &p.pages[1].html);
    assert!(
        p1.starts_with("<section class=\"papier-page\" style=\"width:595.28pt;height:841.89pt\""),
        "{p1}"
    );
    // words are text, escaped
    assert!(
        p1.contains(">Voir </span>")
            && p1.contains(">&amp; </span>")
            && p1.contains(">&lt;plus&gt;</span>"),
        "{p1}"
    );
    // links: internal jumps to the target on page 2, external opens a new tab
    assert!(p1.contains("href=\"#papier-suite\""));
    assert!(p1.contains("href=\"https://exemple.fr\" target=\"_blank\""));
    assert!(!p1.contains("javascript:"), "{p1}");
    assert!(p2.contains("id=\"papier-suite\""));
    // two faces used (regular and bold), each embedded once
    assert_eq!(p.style.matches("@font-face").count(), 2);
    assert!(p.style.contains("ascent-override"));
}

/// Decode every QR code found in a PNG page.
fn read_qr(png: &[u8]) -> Vec<String> {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let px = pm.data();
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        let i = (y * w + x) * 4;
        ((px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32) / 3) as u8
    });
    img.detect_grids()
        .into_iter()
        .filter_map(|g| g.decode().ok().map(|(_, s)| s))
        .collect()
}

#[test]
fn qr_codes_are_drawn_and_scan() {
    let url = "https://atelier-lumiere.fr/billet/000418?r=F&s=12";
    let d = body(json!([
        { "type": "image", "src": { "qr": url }, "alt": "Billet en ligne", "style": { "width": 120 } },
        { "type": "image", "src": { "qr": "PAIEMENT", "ecc": "H", "color": "#1e3a8a" }, "style": { "width": 90 } },
    ]));
    let out = Engine::new().render(&d).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let png = &Engine::new().preview(&d, 2.0).unwrap()[0];
    let mut found = read_qr(png);
    found.sort();
    assert_eq!(found, vec!["PAIEMENT".to_string(), url.to_string()]);
    // vector in the HTML preview too
    let html = Engine::new().preview_html(&d).unwrap();
    assert_eq!(html.style.matches("image/svg+xml").count(), 2);
}

#[test]
fn an_empty_qr_code_is_an_error() {
    let d = body(json!([{ "type": "image", "src": { "qr": "" } }]));
    assert!(matches!(
        Engine::new().render(&d),
        Err(EngineError::Invalid(_))
    ));
}

/// WOFF 1 → the sfnt it holds (inverse of what the HTML preview does), for checking the font.
fn unwoff(w: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let be16 = |o: usize| u16::from_be_bytes([w[o], w[o + 1]]) as usize;
    let be32 = |o: usize| u32::from_be_bytes([w[o], w[o + 1], w[o + 2], w[o + 3]]) as usize;
    assert_eq!(&w[..4], b"wOFF");
    assert_eq!(be32(8), w.len(), "WOFF length field");
    let n = be16(12);
    let mut tables = Vec::new();
    for i in 0..n {
        let e = 44 + 20 * i;
        let (off, comp, orig) = (be32(e + 4), be32(e + 8), be32(e + 12));
        let raw = &w[off..off + comp];
        let data = if comp < orig {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(raw)
                .read_to_end(&mut out)
                .unwrap();
            out
        } else {
            raw.to_vec()
        };
        assert_eq!(data.len(), orig);
        tables.push((w[e..e + 4].to_vec(), data));
    }
    let mut sfnt = w[4..8].to_vec();
    sfnt.extend_from_slice(&(n as u16).to_be_bytes());
    sfnt.extend_from_slice(&[0; 6]);
    let mut off = 12 + 16 * n;
    let mut body = Vec::new();
    for (tag, data) in &tables {
        sfnt.extend_from_slice(tag);
        sfnt.extend_from_slice(&[0; 4]);
        sfnt.extend_from_slice(&(off as u32).to_be_bytes());
        sfnt.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        off = 12 + 16 * n + body.len();
    }
    sfnt.extend_from_slice(&body);
    sfnt
}

#[test]
fn html_preview_fonts_are_light_and_keep_only_the_glyphs_used() {
    let d = body(json!([text("Bonjour")]));
    let p = Engine::new().preview_html(&d).unwrap();
    let b64 = p
        .style
        .split("data:font/woff;base64,")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    let woff = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    assert!(woff.len() < 80_000, "{} bytes", woff.len()); // Inter is ~410 KB
    let sfnt = unwoff(&woff);
    let face = ttf_parser::Face::parse(&sfnt, 0).unwrap();
    let outline = |c: char| {
        let g = face.glyph_index(c).unwrap(); // cmap kept: the browser finds every character
        face.outline_glyph(g, &mut NoOutline).is_some()
    };
    assert!(
        outline('B') && outline('j'),
        "used glyphs keep their outline"
    );
    assert!(!outline('Z') && !outline('x'), "unused glyphs are emptied");
    assert!(
        face.tables().gpos.is_some() && face.tables().gsub.is_some(),
        "kerning and ligatures kept"
    );
}

struct NoOutline;
impl ttf_parser::OutlineBuilder for NoOutline {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}
