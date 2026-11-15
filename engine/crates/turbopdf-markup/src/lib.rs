//! # turbopdf-markup
//!
//! HTML + CSS → the parts of a TurboPDF document tree: pages (with their header / footer), metadata, the
//! `@page` setup, `@font-face` fonts, and warnings. HTML is parsed by html5ever (the WHATWG algorithm);
//! stylesheets are `<style>` elements, local `<link rel="stylesheet">` files and the `css` option.
//!
//! Files (stylesheets, fonts) are read through the host's [`ResourceLoader`]: the same sandbox as the
//! engine's images. Nothing is fetched over the network and no script runs.

mod cascade;
mod convert;
mod dom;
mod images;
pub mod style;

use cascade::{FontFaceRule, StyleSource, Stylesheet, scheme};
use convert::Converter;
use dom::{Dom, Id};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashSet;
use turbopdf_fonts::ResourceLoader;
use turbopdf_tree::{BinarySource, Edges, FontFace, FontStyle, Node, PageFormat, PageSize};

pub use convert::keep_spaces;

/// Built-in rules: the page-break utility classes.
const UA_CSS: &str = "
.turbopdf-break-before { break-before: page }
.turbopdf-break-after { break-after: page }
.turbopdf-keep-together { break-inside: avoid }
";

#[derive(Debug, Default)]
pub struct Input<'a> {
    pub html: &'a str,
    /// Extra CSS, applied after the document's own stylesheets.
    pub css: Option<&'a str>,
    /// Header / footer HTML, converted with the document's stylesheets. `{page}` / `{pages}` are kept.
    pub header: Option<&'a str>,
    pub footer: Option<&'a str>,
}

/// One `<turbopdf-page>`, or the content outside of any.
#[derive(Debug, Serialize)]
pub struct PageParts {
    /// The page's own props (`size`, `margin`…), as the components wrote them.
    pub props: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<Node>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<Vec<Node>>,
    pub children: Vec<Node>,
}

/// The `@page` rule, resolved.
#[derive(Debug, Default, Serialize)]
pub struct PageSetup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<PageSize>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub landscape: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin: Option<Edges>,
}

/// A problem that did not stop the conversion, as the SDK reports it.
#[derive(Debug, Serialize)]
pub struct Warning {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub url: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Converted {
    /// `<turbopdf-document>` props, `<html lang>`, `<title>`.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub meta: Map<String, Value>,
    pub pages: Vec<PageParts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<Node>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<Vec<Node>>,
    pub page: PageSetup,
    /// `@font-face` fonts that could be read: `{ "buffer": n }` sources, `n` indexing [`Self::buffers`].
    pub fonts: Vec<FontFace>,
    pub warnings: Vec<Warning>,
    #[serde(skip)]
    pub buffers: Vec<Vec<u8>>,
}

#[derive(Debug)]
pub struct InvalidComponent(pub Vec<String>);

impl std::fmt::Display for InvalidComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl std::error::Error for InvalidComponent {}

/// Every `<style>` element's text and every `<link rel="stylesheet">` href, in document order.
fn style_sources(dom: &Dom, id: Id, styles: &mut Vec<String>, links: &mut Vec<String>) {
    for &c in dom.children(id) {
        let Some(e) = dom.element(c) else { continue };
        if e.tag == "style" {
            styles.push(dom.own_text(c));
        } else if e.tag == "link"
            && e.attr("rel").is_some_and(|r| {
                r.split_whitespace()
                    .any(|t| t.eq_ignore_ascii_case("stylesheet"))
            })
            && let Some(href) = e.attr("href").filter(|h| !h.is_empty())
        {
            links.push(href.to_string());
        } else if !e.is_svg() {
            style_sources(dom, c, styles, links);
        }
    }
}

fn is_remote(url: &str) -> bool {
    scheme(url).is_some_and(|s| !s.eq_ignore_ascii_case("file"))
}

fn folder(href: &str) -> String {
    let p = href.trim_start_matches("file://").replace('\\', "/");
    match p.rsplit_once('/') {
        Some((dir, _)) if dir != "." => dir.to_string(),
        _ => String::new(),
    }
}

/// Body content as nodes. Styled `<html>` / `<body>` elements stay boxes around it.
fn loose_content(cv: &mut Converter, ids: &[Id], wrappers: &[Id]) -> Vec<Node> {
    let flex = wrappers.last().is_some_and(|&b| {
        matches!(
            cv.computed.prop(b, &["display"]).as_deref(),
            Some("flex" | "grid" | "inline-flex" | "inline-grid")
        )
    });
    let mut nodes = cv.blocks(ids, flex);
    for &w in wrappers.iter().rev() {
        let style = style::parse_style(cv.computed.get(w), style::ROOT_FONT_SIZE);
        let has_id = cv.dom.element(w).is_some_and(|e| e.has("id"));
        if !style::is_empty(&style) || has_id {
            // convert the wrapper itself as a box, with these nodes inside
            let mut v = turbopdf_tree::View {
                style: Some(style),
                children: nodes,
                ..Default::default()
            };
            if let Some(i) = cv.dom.element(w).and_then(|e| e.attr("id"))
                && cv.used_ids.insert(i.to_string())
            {
                v.id = Some(i.to_string());
            }
            nodes = vec![Node::View(v)];
        }
    }
    nodes
}

fn page_setup(rule: &cascade::PageRule) -> PageSetup {
    let mut setup = PageSetup::default();
    if let Some(size) = &rule.size {
        let lower = size.to_ascii_lowercase();
        let tokens: Vec<&str> = lower.split_whitespace().collect();
        let paper = tokens.iter().find_map(|t| match *t {
            "a3" => Some(PageFormat::A3),
            "a4" => Some(PageFormat::A4),
            "a5" => Some(PageFormat::A5),
            "letter" => Some(PageFormat::Letter),
            "legal" => Some(PageFormat::Legal),
            _ => None,
        });
        let lengths: Vec<f32> = tokens
            .iter()
            .filter_map(|t| style::css_length(t, style::ROOT_FONT_SIZE))
            .map(|n| n.max(0.0))
            .collect();
        if let Some(p) = paper {
            setup.size = Some(PageSize::Format(p));
        }
        if let [w, h] = lengths.as_slice() {
            setup.size = Some(PageSize::Custom {
                width: *w,
                height: *h,
            });
        }
        setup.landscape = tokens.contains(&"landscape");
    }
    if let Some(m) = &rule.margin {
        let decls = vec![("padding".to_string(), m.clone())];
        setup.margin = Some(
            style::parse_style(&decls, style::ROOT_FONT_SIZE)
                .padding
                .unwrap_or(Edges::All(28.35)),
        );
    }
    setup
}

const FONT_FORMATS: &[&str] = &["truetype", "opentype", "ttf", "otf"];

fn is_font_file(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.ends_with(".ttf") || u.ends_with(".otf")
}

/// `@font-face` rules → fonts: the first local .ttf / .otf source (or `data:` URL) of each rule.
fn font_faces(rules: &[FontFaceRule], loader: &dyn ResourceLoader, out: &mut Converted) {
    for rule in rules {
        let mut fail = |url: &str, reason: &str| {
            out.warnings.push(Warning {
                kind: "font_failed",
                url: url.to_string(),
                message: format!(
                    "Font \"{}\" from {url} was not loaded: {reason} Text in \"{}\" uses Inter instead.",
                    rule.family, rule.family
                ),
            })
        };
        let data = rule.src.iter().find_map(|(url, _)| {
            images::data_url(url).filter(|(mime, _)| {
                mime.contains("ttf")
                    || mime.contains("otf")
                    || mime.contains("truetype")
                    || mime.contains("opentype")
            })
        });
        let local = rule.src.iter().find(|(url, format)| {
            !is_remote(url)
                && match format {
                    Some(f) => FONT_FORMATS.contains(&f.as_str()),
                    None => is_font_file(url),
                }
        });
        let bytes = if let Some((_, b64)) = data {
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(b) => b,
                Err(_) => {
                    fail("data: URL", "its base64 is invalid.");
                    continue;
                }
            }
        } else if let Some((url, _)) = local {
            match loader.load(url) {
                Ok(b) => b,
                Err(reason) => {
                    fail(url, &format!("{reason}."));
                    continue;
                }
            }
        } else {
            let (first, _) = &rule.src[0];
            fail(
                first,
                if is_remote(first) {
                    "web fonts are never downloaded. Put the .ttf / .otf file next to your HTML and use url(fonts/…)."
                } else {
                    "only .ttf and .otf files are supported (not WOFF / WOFF2): convert it, e.g. with fonttools."
                },
            );
            continue;
        };
        let w = rule
            .weight
            .as_deref()
            .unwrap_or("normal")
            .split_whitespace()
            .next()
            .unwrap_or("normal")
            .to_ascii_lowercase();
        let weight = match w.as_str() {
            "bold" => 700,
            "normal" => 400,
            n => n
                .parse::<f32>()
                .ok()
                .map(|n| ((n / 100.0).round() * 100.0).clamp(100.0, 900.0) as u16)
                .filter(|n| *n > 0)
                .unwrap_or(400),
        };
        let italic = rule
            .style
            .as_deref()
            .is_some_and(|s| s.contains("italic") || s.contains("oblique"));
        out.fonts.push(FontFace {
            family: rule.family.clone(),
            weight,
            style: Some(if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            }),
            source: BinarySource::Buffer {
                buffer: out.buffers.len() as u32,
            },
        });
        out.buffers.push(bytes);
    }
}

/// HTML (a page, a fragment, or the markup React renders) → the parts of a document tree.
pub fn convert(input: &Input, loader: &dyn ResourceLoader) -> Result<Converted, InvalidComponent> {
    let mut out = Converted {
        meta: Map::new(),
        pages: Vec::new(),
        header: None,
        footer: None,
        page: PageSetup::default(),
        fonts: Vec::new(),
        warnings: Vec::new(),
        buffers: Vec::new(),
    };
    let mut dom = Dom::parse(input.html);

    // stylesheets: built-in rules, linked files, <style> elements, then the `css` option
    let (mut styles, mut links) = (Vec::new(), Vec::new());
    style_sources(&dom, 0, &mut styles, &mut links);
    let mut linked: Vec<(String, String)> = Vec::new();
    for href in links {
        let fail = |out: &mut Converted, reason: String| {
            out.warnings.push(Warning {
                kind: "stylesheet_failed",
                url: href.clone(),
                message: format!("Stylesheet {href} was not applied: {reason}"),
            })
        };
        if is_remote(&href) {
            fail(
                &mut out,
                "remote stylesheets are never fetched. Inline it in a <style> element or pass it with the `css` option.".into(),
            );
            continue;
        }
        match loader.load(&href) {
            Ok(bytes) => linked.push((String::from_utf8_lossy(&bytes).into_owned(), folder(&href))),
            Err(reason) => fail(&mut out, format!("{reason}.")),
        }
    }
    let mut sources = vec![StyleSource {
        css: UA_CSS,
        base: "",
    }];
    sources.extend(linked.iter().map(|(css, base)| StyleSource { css, base }));
    sources.extend(styles.iter().map(|css| StyleSource { css, base: "" }));
    if let Some(css) = input.css {
        sources.push(StyleSource { css, base: "" });
    }
    let sheet = Stylesheet::parse(&sources);
    out.page = page_setup(&sheet.page);
    font_faces(&sheet.font_faces, loader, &mut out);

    let skeleton = dom.skeleton();
    let computed = cascade::cascade(&mut dom, &sheet, &[skeleton.html]);
    let mut used_ids = HashSet::new();
    let mut errors = Vec::new();

    // metadata: <html lang>, <title>
    if let Some(lang) = dom
        .element(skeleton.html)
        .and_then(|e| e.attr("lang"))
        .filter(|l| !l.is_empty())
    {
        out.meta.insert("lang".into(), Value::String(lang.into()));
    }
    if let Some(t) = skeleton
        .head
        .and_then(|h| dom.find(h, "title"))
        .or_else(|| dom.find(0, "title"))
    {
        let title = dom.own_text(t).trim().to_string();
        if !title.is_empty() {
            out.meta.insert("title".into(), Value::String(title));
        }
    }

    {
        let mut cv = Converter {
            dom: &dom,
            computed: &computed,
            used_ids: &mut used_ids,
            errors: &mut errors,
        };
        let body = skeleton.body.unwrap_or(skeleton.html);
        let mut top: Vec<Id> = cv.dom.children(body).to_vec();
        let mut wrappers = vec![skeleton.html, body];
        if let Some(doc) = top
            .iter()
            .copied()
            .find(|&c| cv.dom.tag(c) == Some("turbopdf-document"))
        {
            // component props win over <html lang> / <title>
            for (k, v) in cv_props(&cv, doc) {
                out.meta.insert(k, v);
            }
            top = cv.dom.children(doc).to_vec();
            wrappers.clear();
        }
        let mut loose: Vec<Id> = Vec::new();
        let flush = |cv: &mut Converter,
                     loose: &mut Vec<Id>,
                     pages: &mut Vec<PageParts>,
                     wrappers: &[Id]| {
            let nodes = loose_content(cv, loose, wrappers);
            if !nodes.is_empty() {
                pages.push(PageParts {
                    props: Map::new(),
                    header: None,
                    footer: None,
                    children: nodes,
                });
            }
            loose.clear();
        };
        for c in top {
            if cv.dom.tag(c) == Some("turbopdf-page") {
                flush(&mut cv, &mut loose, &mut out.pages, &wrappers);
                let part = |cv: &mut Converter, tag: &str| {
                    let el = cv
                        .dom
                        .element_children(c)
                        .find(|&x| cv.dom.tag(x) == Some(tag))?;
                    let kids = cv.dom.children(el).to_vec();
                    Some(cv.blocks(&kids, false))
                };
                let header = part(&mut cv, "turbopdf-header").filter(|h| !h.is_empty());
                let footer = part(&mut cv, "turbopdf-footer").filter(|f| !f.is_empty());
                let children = part(&mut cv, "turbopdf-body").unwrap_or_default();
                out.pages.push(PageParts {
                    props: cv_props(&cv, c),
                    header,
                    footer,
                    children,
                });
            } else {
                loose.push(c);
            }
        }
        flush(&mut cv, &mut loose, &mut out.pages, &wrappers);
    }

    // header / footer: converted with the document's stylesheets
    let mut band = |html: Option<&str>| -> Option<Vec<Node>> {
        let mut d = Dom::parse(html?);
        let sk = d.skeleton();
        let computed = cascade::cascade(&mut d, &sheet, &[sk.html]);
        let body = sk.body.unwrap_or(sk.html);
        let kids = d.children(body).to_vec();
        let nodes = Converter {
            dom: &d,
            computed: &computed,
            used_ids: &mut used_ids,
            errors: &mut errors,
        }
        .blocks(&kids, false);
        (!nodes.is_empty()).then_some(nodes)
    };
    out.header = band(input.header);
    out.footer = band(input.footer);

    if errors.is_empty() {
        Ok(out)
    } else {
        Err(InvalidComponent(errors))
    }
}

fn cv_props(cv: &Converter, id: Id) -> Map<String, Value> {
    cv.dom
        .element(id)
        .and_then(|e| e.attr("data-p"))
        .and_then(|p| serde_json::from_str::<Value>(p).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// An HTML fragment (a raw HTML block in Markdown) → nodes. Inline styles only, no file access.
pub fn fragment(html: &str) -> Result<Vec<Node>, InvalidComponent> {
    let mut d = Dom::parse(html);
    let sk = d.skeleton();
    let sheet = Stylesheet::parse(&[]);
    let computed = cascade::cascade(&mut d, &sheet, &[sk.html]);
    let body = sk.body.unwrap_or(sk.html);
    let kids = d.children(body).to_vec();
    let mut used = HashSet::new();
    let mut errors = Vec::new();
    let nodes = Converter {
        dom: &d,
        computed: &computed,
        used_ids: &mut used,
        errors: &mut errors,
    }
    .blocks(&kids, false);
    if errors.is_empty() {
        Ok(nodes)
    } else {
        Err(InvalidComponent(errors))
    }
}
