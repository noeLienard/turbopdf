//! HTML → tree parts, on the kind of markup found in the wild: unclosed tags, tables without `<tbody>`,
//! CMS output, inline SVG, framework-size stylesheets.

use serde_json::{Value, json};
use turbopdf_fonts::NoLoader;
use turbopdf_markup::{Input, convert};

fn parts(html: &str) -> Value {
    serde_json::to_value(
        convert(
            &Input {
                html,
                ..Default::default()
            },
            &NoLoader,
        )
        .expect("converts"),
    )
    .unwrap()
}

fn body(html: &str) -> Value {
    parts(html)["pages"][0]["children"].clone()
}

#[test]
fn unclosed_paragraphs_and_list_items_close_like_in_a_browser() {
    let b = body("<p>One<p>Two<ul><li>a<li>b</ul>");
    assert_eq!(b[0]["content"], "One");
    assert_eq!(b[1]["content"], "Two");
    let items = b[2]["children"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1]["children"][1]["children"][0]["content"], "b");
}

#[test]
fn a_table_without_tbody_keeps_its_header_row() {
    let b = body("<table><tr><th>Item<th>Price<tr><td>Logo<td>450<tr><td>Site<td>900</table>");
    let t = &b[0];
    assert_eq!(t["type"], "table");
    assert_eq!(t["header"].as_array().unwrap().len(), 1);
    assert_eq!(t["rows"].as_array().unwrap().len(), 2);
    assert_eq!(t["rows"][1]["cells"][1]["children"][0]["content"], "900");
}

#[test]
fn entities_and_whitespace() {
    let b = body("<p>Tom &amp; Jerry&nbsp;&copy; 2026\n   <b>bold</b></p>");
    assert_eq!(b[0]["content"][0]["text"], "Tom & Jerry\u{a0}© 2026 ");
    assert_eq!(b[0]["content"][1]["style"]["fontWeight"], 700);
}

#[test]
fn inline_svg_keeps_attribute_case_and_gets_a_namespace() {
    let b = body(
        r#"<div><svg viewBox="0 0 10 10" width="20"><linearGradient id="g"/><rect width="10" height="10" fill="url(#g)"/></svg></div>"#,
    );
    let img = &b[0]["children"][0];
    assert_eq!(img["format"], "svg");
    use base64::Engine as _;
    let xml = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(img["src"]["data"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(
        xml.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10""#),
        "{xml}"
    );
    assert!(xml.contains("<linearGradient"), "{xml}");
    assert_eq!(img["style"]["width"], 15.0);
}

#[test]
fn the_cascade_applies_variables_generated_content_and_specificity() {
    let html = r#"<style>
        :root { --brand: #1d4ed8 }
        p { color: red }
        .note { color: var(--brand) }
        #x.note { font-weight: bold }
        .note::before { content: "→ " attr(data-k); color: green }
        @media screen { p { color: blue } }
        @page { size: A5 landscape; margin: 12mm }
    </style><p class="note" id="x" data-k="K">Hi</p>"#;
    let p = parts(html);
    let n = &p["pages"][0]["children"][0];
    assert_eq!(n["style"]["color"], "#1d4ed8");
    assert_eq!(n["style"]["fontWeight"], 700);
    assert_eq!(n["content"][0]["text"], "→ K");
    assert_eq!(n["content"][0]["style"]["color"], "#008000");
    assert_eq!(n["id"], "x");
    assert_eq!(
        p["page"],
        json!({ "size": "A5", "landscape": true, "margin": (12.0f32 * 72.0 / 25.4) as f64 })
    );
}

#[test]
fn styled_body_stays_a_box_and_plain_body_is_unwrapped() {
    let plain = body("<html><body><p>a</p></body></html>");
    assert_eq!(plain[0]["type"], "text");
    let styled = body(
        r#"<html lang="fr"><head><title> Facture </title></head><body style="padding:10pt"><p>a</p></body></html>"#,
    );
    assert_eq!(styled[0]["type"], "view");
    assert_eq!(styled[0]["style"]["padding"], 10.0);
    let p = parts(r#"<html lang="fr"><head><title> Facture </title></head><body></body></html>"#);
    assert_eq!(p["meta"], json!({ "lang": "fr", "title": "Facture" }));
}

#[test]
fn components_map_to_tree_nodes() {
    let html = r#"<turbopdf-document data-p='{"title":"T"}'><turbopdf-page data-p='{"margin":20}'><turbopdf-header><turbopdf-text data-p='{}'>Head</turbopdf-text></turbopdf-header><turbopdf-body><turbopdf-view data-p='{"style":{"padding":4}}'><turbopdf-text data-p='{}'>Hello <turbopdf-span data-p='{"style":{"fontWeight":700}}'>you</turbopdf-span></turbopdf-text></turbopdf-view><turbopdf-pagebreak data-p='{}'></turbopdf-pagebreak></turbopdf-body></turbopdf-page></turbopdf-document>"#;
    let p = parts(html);
    assert_eq!(p["meta"]["title"], "T");
    let page = &p["pages"][0];
    assert_eq!(page["props"]["margin"], 20);
    assert_eq!(page["header"][0]["content"], "Head");
    assert_eq!(page["children"][0]["style"]["padding"], 4.0);
    assert_eq!(
        page["children"][0]["children"][0]["content"][1],
        json!({ "text": "you", "style": { "fontWeight": 700 } })
    );
    assert_eq!(page["children"][1]["type"], "page_break");
}

#[test]
fn a_broken_component_is_an_error_not_a_panic() {
    let r = convert(
        &Input {
            html: r#"<turbopdf-heading data-p='{"level":"x"}'>T</turbopdf-heading>"#,
            ..Default::default()
        },
        &NoLoader,
    );
    assert!(r.is_err());
}

#[test]
fn files_are_read_through_the_loader_and_failures_are_warnings() {
    let p = parts(
        r#"<link rel="stylesheet" href="https://cdn.example/x.css"><link rel="stylesheet" href="missing.css"><style>@font-face { font-family: B; src: url(b.woff2) }</style><p>x</p>"#,
    );
    let w = p["warnings"].as_array().unwrap();
    assert_eq!(
        w.iter()
            .map(|w| w["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["stylesheet_failed", "stylesheet_failed", "font_failed"]
    );
    assert!(w[0]["message"].as_str().unwrap().contains("never fetched"));
}

#[test]
fn a_framework_size_stylesheet_stays_fast() {
    let mut css = String::new();
    for i in 0..9000 {
        css.push_str(&format!(
            ".c{i} {{ color: #123456 }} .row .c{i} > span {{ padding: 1px }}\n"
        ));
    }
    let rows: String = (0..2000)
        .map(|i| format!("<div class=\"row c{}\"><span>{i}</span></div>", i % 9000))
        .collect();
    let html = format!("<style>{css}</style>{rows}");
    let t = std::time::Instant::now();
    let p = parts(&html);
    assert_eq!(p["pages"][0]["children"].as_array().unwrap().len(), 2000);
    // release: ~0.15 s (a quadratic cascade or stylesheet parser takes seconds); debug builds are ~10× slower
    let budget = if cfg!(debug_assertions) { 6000 } else { 600 };
    assert!(t.elapsed().as_millis() < budget, "took {:?}", t.elapsed());
}
