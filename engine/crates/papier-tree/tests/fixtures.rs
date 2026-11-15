use papier_tree::{Document, Node, Warning, validate};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// JSON numbers 12 and 12.0 are the same value for us. The tree stores f32, and `to_value` widens it
/// to f64 (1.2 -> 1.2000000476...), so both sides are compared at f32 precision.
fn normalize(v: Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap() as f32 as f64),
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

fn load(name: &str) -> (Value, Document) {
    let text = fs::read_to_string(fixtures_dir().join(name)).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    let doc: Document =
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    (value, doc)
}

fn colors_are_canonical(v: &mut Value) {
    // fixtures may use short hex (#f00); the canonical form is #rrggbb
    match v {
        // `#prix` is a link to an id, not a colour
        Value::String(s) if s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()) => {
            *s = papier_tree::Color::parse(s).unwrap().to_string()
        }
        Value::Array(a) => a.iter_mut().for_each(colors_are_canonical),
        Value::Object(o) => o.values_mut().for_each(colors_are_canonical),
        _ => {}
    }
}

#[test]
fn every_fixture_parses_validates_and_roundtrips() {
    let mut count = 0;
    for entry in fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        let (mut original, doc) = load(name);
        validate(&doc).unwrap_or_else(|e| panic!("{name} should be valid: {e:?}"));
        colors_are_canonical(&mut original);
        let back = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            normalize(back),
            normalize(original),
            "{name}: roundtrip changed the tree"
        );
        count += 1;
    }
    assert!(count >= 3);
}

#[test]
fn all_nodes_fixture_contains_every_node_type() {
    let (_, doc) = load("all_nodes.json");
    let mut seen = std::collections::BTreeSet::new();
    fn walk(nodes: &[Node], seen: &mut std::collections::BTreeSet<&'static str>) {
        for n in nodes {
            match n {
                Node::View(v) => {
                    seen.insert("view");
                    walk(&v.children, seen)
                }
                Node::Text(_) => {
                    seen.insert("text");
                }
                Node::Heading(_) => {
                    seen.insert("heading");
                }
                Node::Image(_) => {
                    seen.insert("image");
                }
                Node::Table(t) => {
                    seen.insert("table");
                    for r in t.rows.iter().chain(t.header.iter().flatten()) {
                        for c in &r.cells {
                            walk(&c.children, seen)
                        }
                    }
                }
                Node::PageBreak(_) => {
                    seen.insert("page_break");
                }
                Node::PageNumber(_) => {
                    seen.insert("page_number");
                }
                Node::TableOfContents(_) => {
                    seen.insert("table_of_contents");
                }
            }
        }
    }
    for p in &doc.children {
        seen.insert("page");
        walk(&p.children, &mut seen)
    }
    seen.insert("document");
    for t in [
        "document",
        "page",
        "view",
        "text",
        "heading",
        "image",
        "table",
        "page_break",
        "page_number",
        "table_of_contents",
    ] {
        assert!(seen.contains(t), "fixture misses node type {t}");
    }
}

fn doc_with_body(body: Value) -> Document {
    serde_json::from_value(json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": body }] })).unwrap()
}

fn codes(doc: &Document) -> Vec<(String, &'static str)> {
    validate(doc)
        .unwrap_err()
        .into_iter()
        .map(|e| (e.path, e.code))
        .collect()
}

#[test]
fn validation_reports_precise_paths() {
    let doc = doc_with_body(json!([
        { "type": "heading", "level": 9, "content": "x" },
        { "type": "view", "children": [ { "type": "text", "id": "a", "content": "x" }, { "type": "text", "id": "a", "content": "y" } ] },
        { "type": "table", "columns": [{}, {}], "rows": [ { "cells": [ { "children": [] } ] } ] },
        { "type": "text", "style": { "fontSize": 0, "fontWeight": 50, "gap": -1 }, "content": "z" }
    ]));
    let c = codes(&doc);
    assert!(
        c.contains(&(
            "/children/0/children/0/level".into(),
            "invalid_heading_level"
        )),
        "{c:?}"
    );
    assert!(
        c.contains(&("/children/0/children/1/children/1".into(), "duplicate_id")),
        "{c:?}"
    );
    assert!(
        c.contains(&(
            "/children/0/children/2/rows/0/cells".into(),
            "column_mismatch"
        )),
        "{c:?}"
    );
    assert!(
        c.contains(&(
            "/children/0/children/3/style/fontSize".into(),
            "invalid_number"
        )),
        "{c:?}"
    );
    assert!(
        c.contains(&(
            "/children/0/children/3/style/fontWeight".into(),
            "invalid_font_weight"
        )),
        "{c:?}"
    );
    assert!(
        c.contains(&("/children/0/children/3/style/gap".into(), "invalid_number")),
        "{c:?}"
    );
}

#[test]
fn validation_checks_document_level() {
    let doc: Document =
        serde_json::from_value(json!({ "type": "document", "version": 2, "children": [] }))
            .unwrap();
    let c = codes(&doc);
    assert!(c.contains(&("/version".into(), "unsupported_version")));
    assert!(c.contains(&("/children".into(), "no_pages")));
}

#[test]
fn unknown_fields_and_bad_values_are_rejected() {
    let bad = |v: Value| serde_json::from_value::<Document>(v).is_err();
    // typo in a style key
    assert!(bad(
        json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": [{ "type": "text", "content": "x", "style": { "fontsize": 12 } }] }] })
    ));
    // unknown node type
    assert!(bad(
        json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": [{ "type": "paragraph" }] }] })
    ));
    // bad color / dimension
    assert!(bad(
        json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": [{ "type": "text", "content": "x", "style": { "color": "red" } }] }] })
    ));
    assert!(bad(
        json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": [{ "type": "text", "content": "x", "style": { "width": "wide" } }] }] })
    ));
}

#[test]
fn colors_parse_and_print_canonically() {
    use papier_tree::Color;
    assert_eq!(Color::parse("#f00").unwrap().to_string(), "#ff0000");
    assert_eq!(Color::parse("#2563EB").unwrap().to_string(), "#2563eb");
    assert_eq!(Color::parse("#00000080").unwrap().to_string(), "#00000080");
    assert_eq!(Color::parse("transparent").unwrap(), Color::TRANSPARENT);
    assert!(
        Color::parse("#12").is_err()
            && Color::parse("blue").is_err()
            && Color::parse("#gggggg").is_err()
    );
}

#[test]
fn dimensions_roundtrip() {
    use papier_tree::Dimension;
    for (json, dim) in [
        ("12.5", Dimension::Points(12.5)),
        ("\"auto\"", Dimension::Auto),
        ("\"50%\"", Dimension::Percent(50.0)),
    ] {
        let parsed: Dimension = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, dim);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
    }
}

#[test]
fn warnings_serialize_with_camel_case_fields() {
    let w = Warning::MovedToNextPage {
        path: "/children/0/children/4".into(),
        from_page: 1,
        to_page: 2,
        reason: papier_tree::MoveReason::KeepTogether,
    };
    assert_eq!(
        serde_json::to_value(&w).unwrap(),
        json!({ "type": "moved_to_next_page", "path": "/children/0/children/4", "fromPage": 1, "toPage": 2, "reason": "keep_together" })
    );
    let w = Warning::MissingGlyph {
        path: "/x".into(),
        page: 1,
        character: "𓀀".into(),
        codepoint: 0x13000,
        font_family: "Inter".into(),
    };
    assert_eq!(serde_json::to_value(&w).unwrap()["fontFamily"], "Inter");
}
