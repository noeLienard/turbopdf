//! A cascaded DOM → document tree nodes.
//!
//! Two vocabularies are understood:
//! * TurboPDF components (`turbopdf/components`) render `<turbopdf-*>` elements whose `data-p` attribute
//!   holds their props as JSON: they map 1:1 to tree nodes.
//! * HTML (div, p, h1–h6, span/b/i, a, img, svg, table, ul/ol, hr, br, input/textarea/select), with its
//!   cascaded CSS: mapped to the closest tree nodes.

use crate::cascade::Computed;
use crate::dom::{Data, Dom, Element, Id};
use crate::images;
use crate::style::{
    ROOT_FONT_SIZE, css_length, is_empty, merge, merge_text, parse_style, text_style_is_empty,
    text_style_of,
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use turbopdf_tree::{
    Cell, Color, Column, Dimension, EdgeSides, Edges, FlexDirection, FontStyle, Heading, Image,
    Node, PageBreak, QrEcc, Role, Row, Span, Style, Table, TextAlign, TextContent, TextDecoration,
    TextStyle, VerticalAlign, View,
};

mod float;
mod form;
mod table;

const INLINE: &[&str] = &[
    "span",
    "b",
    "strong",
    "i",
    "em",
    "a",
    "code",
    "small",
    "u",
    "s",
    "sub",
    "sup",
    "ins",
    "del",
    "strike",
    "label",
    "br",
    "mark",
    "abbr",
    "time",
    "turbopdf-span",
];
const LINK_BLUE: Color = Color {
    r: 0x25,
    g: 0x63,
    b: 0xeb,
    a: 255,
};

fn hex(s: &str) -> Color {
    Color::parse(s).expect("valid built-in colour")
}

fn bottom(v: f32) -> Edges {
    Edges::Sides(EdgeSides {
        bottom: Some(v),
        ..Default::default()
    })
}

/// `white-space` → the text as the engine should see it: it collapses runs of ordinary spaces and breaks
/// lines at `\n`, so preserved spaces become non-breaking ones and newlines are dropped when collapsing.
fn white_space(s: &str, mode: Option<&str>) -> String {
    let nbsp = |t: String| t.replace(' ', "\u{a0}");
    match mode {
        Some("pre") => nbsp(keep_spaces(&s.replace('\t', "    "))),
        Some("pre-wrap" | "break-spaces") => keep_spaces(&s.replace('\t', "    ")),
        Some("pre-line") => s.split('\n').map(collapse).collect::<Vec<_>>().join("\n"),
        Some("nowrap") => nbsp(collapse(s)),
        _ => collapse(s),
    }
}

/// `text-transform`
fn transform(s: String, mode: Option<&str>) -> String {
    match mode {
        Some("uppercase") => s.to_uppercase(),
        Some("lowercase") => s.to_lowercase(),
        Some("capitalize") => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace() || c == '-' || c == '\u{a0}';
            }
            out
        }
        _ => s,
    }
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\r' | '\n' | '\x0c') {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn some_style(s: Style) -> Option<Style> {
    (!is_empty(&s)).then_some(s)
}

fn text_node(content: TextContent, style: Option<Style>, role: Option<Role>) -> Node {
    Node::Text(turbopdf_tree::Text {
        id: None,
        style,
        keep_together: None,
        role,
        content,
    })
}

fn view(children: Vec<Node>, style: Option<Style>, role: Option<Role>) -> Node {
    Node::View(View {
        children,
        style,
        role,
        ..Default::default()
    })
}

fn style_mut(n: &mut Node) -> Option<&mut Option<Style>> {
    match n {
        Node::View(v) => Some(&mut v.style),
        Node::Text(t) => Some(&mut t.style),
        Node::Heading(h) => Some(&mut h.style),
        Node::Image(i) => Some(&mut i.style),
        Node::Table(t) => Some(&mut t.style),
        Node::PageNumber(p) => Some(&mut p.style),
        Node::TableOfContents(t) => Some(&mut t.style),
        Node::PageBreak(_) => None,
    }
}

fn id_mut(n: &mut Node) -> &mut Option<String> {
    match n {
        Node::View(v) => &mut v.id,
        Node::Text(t) => &mut t.id,
        Node::Heading(h) => &mut h.id,
        Node::Image(i) => &mut i.id,
        Node::Table(t) => &mut t.id,
        Node::PageBreak(p) => &mut p.id,
        Node::PageNumber(p) => &mut p.id,
        Node::TableOfContents(t) => &mut t.id,
    }
}

/// `style` under the node's own style (the node's values win).
fn with_style(mut node: Node, style: Style) -> Node {
    if is_empty(&style) {
        return node;
    }
    if let Some(s) = style_mut(&mut node) {
        *s = Some(match s.take() {
            Some(own) => merge(&style, &own),
            None => style,
        });
    }
    node
}

pub struct Converter<'a> {
    pub dom: &'a Dom,
    pub computed: &'a Computed,
    /// HTML ids already given to a node: tree ids must be unique, later duplicates are dropped.
    pub used_ids: &'a mut HashSet<String>,
    /// Component props that are not valid tree JSON.
    pub errors: &'a mut Vec<String>,
}

impl<'a> Converter<'a> {
    fn el(&self, id: Id) -> Option<&'a Element> {
        self.dom.element(id)
    }

    fn tag(&self, id: Id) -> &'a str {
        self.dom.tag(id).unwrap_or("")
    }

    fn style(&self, id: Id) -> Style {
        parse_style(self.computed.get(id), ROOT_FONT_SIZE)
    }

    fn props(&self, id: Id) -> Map<String, Value> {
        self.el(id)
            .and_then(|e| e.attr("data-p"))
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn component(&mut self, mut props: Map<String, Value>, kind: &str, extra: Value) -> Vec<Node> {
        props.insert("type".into(), Value::String(kind.into()));
        if let Value::Object(m) = extra {
            props.extend(m);
        }
        match serde_json::from_value::<Node>(Value::Object(props)) {
            Ok(n) => vec![n],
            Err(e) => {
                self.errors.push(format!("<{kind}> component: {e}"));
                vec![]
            }
        }
    }

    fn is_text(&self, id: Id) -> bool {
        matches!(self.dom.nodes[id].data, Data::Text(_))
    }

    /// An `<a>` around blocks or images (a clickable card, a linked logo) is a block itself.
    fn is_inline(&self, id: Id) -> bool {
        if self.is_text(id) {
            return true;
        }
        let tag = self.tag(id);
        INLINE.contains(&tag)
            && !((tag == "a" || tag == "label")
                && self
                    .dom
                    .children(id)
                    .iter()
                    .any(|&c| !self.is_inline(c) && self.is_content(c)))
    }

    /// Elements and text: comments and doctypes are not content.
    fn is_content(&self, id: Id) -> bool {
        matches!(self.dom.nodes[id].data, Data::Element(_) | Data::Text(_))
    }

    fn hidden(&self, id: Id) -> bool {
        self.computed.prop(id, &["display"]).as_deref() == Some("none")
            || self.computed.prop(id, &["visibility"]).as_deref() == Some("hidden")
            || self.el(id).is_some_and(|e| e.has("hidden"))
    }

    fn item_layout(&self, id: Id) -> bool {
        matches!(
            self.computed.prop(id, &["display"]).as_deref(),
            Some("flex" | "grid" | "inline-flex" | "inline-grid")
        )
    }

    /// An inherited property (`white-space`, `text-transform`) of a node: from its nearest element ancestor
    /// (or itself) that sets it.
    fn inherited(&self, node: Id, prop: &str) -> Option<String> {
        let mut cur = Some(node);
        while let Some(id) = cur {
            if let Some(v) = self.computed.prop(id, &[prop]) {
                return match v.as_str() {
                    "inherit" => {
                        cur = self.dom.nodes[id].parent;
                        continue;
                    }
                    "initial" | "unset" | "revert" | "normal" | "none" => None,
                    _ => Some(v),
                };
            }
            cur = self.dom.nodes[id].parent;
        }
        None
    }

    /// The text of a text node, with the `white-space` and `text-transform` of its element.
    fn text_of(&self, node: Id, raw: &str) -> String {
        let ws = self.inherited(node, "white-space");
        let tt = self.inherited(node, "text-transform");
        transform(white_space(raw, ws.as_deref()), tt.as_deref())
    }

    fn href(&self, id: Id) -> Option<String> {
        let h = self.el(id)?.attr("href")?.trim();
        (!h.is_empty() && h != "#").then(|| h.to_string())
    }

    // -------------------------------------------------------------- text

    fn spans(&self, children: &[Id], style: &TextStyle, link: Option<&str>, out: &mut Vec<Span>) {
        let span = |text: String, style: &TextStyle, link: Option<&str>| Span {
            text,
            style: (!text_style_is_empty(style)).then(|| style.clone()),
            link: link.map(String::from),
        };
        for &c in children {
            match &self.dom.nodes[c].data {
                Data::Text(t) => {
                    out.push(span(self.text_of(c, t), style, link));
                    continue;
                }
                Data::Element(_) => {}
                _ => continue,
            }
            if self.hidden(c) {
                continue;
            }
            let mut s = style.clone();
            let mut l = link.map(String::from);
            let tag = self.tag(c);
            match tag {
                "br" => {
                    out.push(span("\n".into(), style, link));
                    continue;
                }
                "b" | "strong" => s.font_weight = Some(700),
                "i" | "em" => s.font_style = Some(FontStyle::Italic),
                // browser defaults; the document's CSS (applied below) can override them
                "u" | "ins" => s.text_decoration = Some(TextDecoration::Underline),
                "s" | "del" | "strike" => s.text_decoration = Some(TextDecoration::LineThrough),
                "sup" => s.vertical_align = Some(VerticalAlign::Super),
                "sub" => s.vertical_align = Some(VerticalAlign::Sub),
                "a" => {
                    if let Some(h) = self.href(c) {
                        l = Some(h);
                        s.color = Some(LINK_BLUE);
                        s.text_decoration = Some(TextDecoration::Underline);
                    }
                }
                "turbopdf-span" => {
                    let p = self.props(c);
                    if let Some(own) = p
                        .get("style")
                        .and_then(|v| serde_json::from_value::<TextStyle>(v.clone()).ok())
                    {
                        s = merge_text(&s, &own);
                    }
                    if let Some(Value::String(link)) = p.get("link") {
                        l = Some(link.clone());
                    }
                }
                _ => {}
            }
            let css = text_style_of(&self.style(c));
            if !text_style_is_empty(&css) {
                s = merge_text(&s, &css);
            }
            // block elements inside a text (e.g. <div> in a <p>) start a new line
            let block = !INLINE.contains(&tag) && !tag.starts_with("turbopdf-");
            if block {
                out.push(span("\n".into(), style, None));
            }
            self.spans(self.dom.children(c), &s, l.as_deref(), out);
            if block {
                out.push(span("\n".into(), style, None));
            }
        }
    }

    /// Spans → content: merged, trimmed, a plain string when there is no styling.
    fn content(&self, children: &[Id]) -> TextContent {
        let mut raw = Vec::new();
        self.spans(children, &TextStyle::default(), None, &mut raw);
        let mut merged: Vec<Span> = Vec::new();
        for sp in raw {
            match merged.last_mut() {
                Some(last) if last.style == sp.style && last.link == sp.link => {
                    last.text.push_str(&sp.text)
                }
                _ => merged.push(sp),
            }
        }
        // trim the spaces around hard breaks, then at both ends
        for sp in &mut merged {
            if sp.text.contains('\n') {
                let lines: Vec<&str> = sp.text.split('\n').collect();
                let last = lines.len() - 1;
                sp.text = lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let l = if i > 0 { l.trim_start_matches(' ') } else { l };
                        if i < last { l.trim_end_matches(' ') } else { l }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        if let Some(first) = merged.first_mut() {
            first.text = first.text.trim_start_matches([' ', '\n']).to_string();
        }
        if let Some(last) = merged.last_mut() {
            last.text = last.text.trim_end_matches([' ', '\n']).to_string();
        }
        merged.retain(|s| !s.text.is_empty());
        match merged.as_slice() {
            [] => TextContent::Plain(String::new()),
            [only] if only.style.is_none() && only.link.is_none() => {
                TextContent::Plain(only.text.clone())
            }
            _ => TextContent::Spans(merged),
        }
    }

    fn take_id(&mut self, id: Id, node: &mut Node) {
        if let Some(i) = self.el(id).and_then(|e| e.attr("id"))
            && !self.used_ids.contains(i)
            && id_mut(node).is_none()
        {
            self.used_ids.insert(i.to_string());
            *id_mut(node) = Some(i.to_string());
        }
    }

    // -------------------------------------------------------------- blocks

    /// Children of a container → block nodes. Runs of inline content become one text node, except in a
    /// flex / grid container, where (as in CSS) every child element is its own item.
    pub fn blocks(&mut self, children: &[Id], flex: bool) -> Vec<Node> {
        let mut out = Vec::new();
        if flex {
            for &c in children {
                if let Data::Text(t) = &self.dom.nodes[c].data {
                    let t = t.trim();
                    if !t.is_empty() {
                        out.push(text_node(
                            TextContent::Plain(self.text_of(c, t)),
                            None,
                            None,
                        ));
                    }
                } else if !self.is_content(c) || self.tag(c) == "br" {
                    continue;
                } else if self.is_inline(c) {
                    if self.hidden(c) {
                        continue;
                    }
                    let text = self.content(&[c]);
                    if !is_blank(&text) {
                        // the element is a flex item: its box (position, background, padding…) applies to the text
                        let mut node = text_node(text, some_style(self.style(c)), None);
                        self.take_id(c, &mut node);
                        out.push(node);
                    }
                } else {
                    out.extend(self.block(c));
                }
            }
            return out;
        }
        let mut inline: Vec<Id> = Vec::new();
        let flush = |me: &mut Self, inline: &mut Vec<Id>, out: &mut Vec<Node>| {
            if !inline.is_empty() {
                let c = me.content(inline);
                if !is_blank(&c) {
                    out.push(text_node(c, None, None));
                }
                inline.clear();
            }
        };
        let mut i = 0;
        while i < children.len() {
            let c = children[i];
            if !self.is_content(c) {
                i += 1;
                continue;
            }
            // a float (even an inline one, like `<img style="float:right">`) is a block
            if !self.hidden(c) && self.float_of(c).is_some() {
                flush(self, &mut inline, &mut out);
                let (row, next) = self.float_row(children, i);
                out.push(row);
                i = next;
                continue;
            }
            i += 1;
            if self.is_inline(c) {
                inline.push(c);
                continue;
            }
            flush(self, &mut inline, &mut out);
            out.extend(self.block(c));
        }
        flush(self, &mut inline, &mut out);
        out
    }

    /// One element → nodes, with CSS pagination: `break-before/after: page`, `break-inside: avoid`.
    pub fn block(&mut self, id: Id) -> Vec<Node> {
        if self.hidden(id) {
            return vec![];
        }
        let mut nodes = self.block_of(id);
        let e = self.el(id).expect("an element");
        // aria-hidden / role="presentation": decorative, skipped by screen readers
        if e.attr("aria-hidden") == Some("true")
            || matches!(e.attr("role"), Some("presentation" | "none"))
        {
            for n in &mut nodes {
                match n {
                    Node::View(v) => v.role = Some(Role::Artifact),
                    Node::Text(t) => t.role = Some(Role::Artifact),
                    Node::Image(i) => i.alt = Some(String::new()),
                    _ => {}
                }
            }
        }
        if let [only] = nodes.as_mut_slice()
            && !matches!(only, Node::PageBreak(_))
        {
            self.take_id(id, only);
        }
        if self
            .computed
            .prop(id, &["break-inside", "page-break-inside"])
            .as_deref()
            == Some("avoid")
        {
            for n in &mut nodes {
                match n {
                    Node::View(v) => v.keep_together = Some(true),
                    Node::Text(t) => t.keep_together = Some(true),
                    Node::Heading(h) => h.keep_together = Some(true),
                    Node::Table(t) => t.keep_together = Some(true),
                    _ => {}
                }
            }
        }
        let breaks = |names: &[&str]| {
            matches!(
                self.computed.prop(id, names).as_deref(),
                Some("page" | "always" | "left" | "right" | "recto" | "verso")
            )
        };
        let before = breaks(&["break-before", "page-break-before"]);
        let after = breaks(&["break-after", "page-break-after"]);
        let pb = || Node::PageBreak(PageBreak::default());
        let mut out = Vec::with_capacity(nodes.len() + 2);
        if before {
            out.push(pb());
        }
        out.extend(nodes);
        if after {
            out.push(pb());
        }
        out
    }

    fn block_of(&mut self, id: Id) -> Vec<Node> {
        let tag = self.tag(id).to_string();
        let children: Vec<Id> = self.dom.children(id).to_vec();
        match tag.as_str() {
            // ---- TurboPDF components
            "turbopdf-view" => {
                let kids = self.blocks(&children, false);
                let p = self.props(id);
                self.component(p, "view", json!({ "children": kids }))
            }
            "turbopdf-text" | "turbopdf-heading" => {
                let content = self.content(&children);
                let p = self.props(id);
                self.component(p, &tag[9..], json!({ "content": content }))
            }
            "turbopdf-image" => {
                let p = self.props(id);
                self.component(p, "image", json!({}))
            }
            "turbopdf-table" => self.component_table(id),
            "turbopdf-pagebreak" => {
                let p = self.props(id);
                self.component(p, "page_break", json!({}))
            }
            "turbopdf-pagenumber" => {
                let p = self.props(id);
                self.component(p, "page_number", json!({}))
            }
            "turbopdf-toc" => {
                let p = self.props(id);
                self.component(p, "table_of_contents", json!({}))
            }
            "turbopdf-document" | "turbopdf-page" | "turbopdf-body" => {
                self.blocks(&children, false)
            }
            "turbopdf-header" | "turbopdf-footer" => vec![],

            // ---- HTML
            "p" | "pre" | "blockquote" | "figcaption" => {
                let style = self.style(id);
                let text = if tag == "pre" {
                    self.pre_content(id)
                } else {
                    self.content(&children)
                };
                let mut s = merge(
                    &Style {
                        margin: Some(bottom(8.0)),
                        ..Default::default()
                    },
                    &style,
                );
                let role = match tag.as_str() {
                    "pre" => {
                        s = merge(
                            &s,
                            &merge(
                                &Style {
                                    background_color: Some(hex("#f3f4f6")),
                                    padding: Some(Edges::All(8.0)),
                                    font_size: Some(9.0),
                                    ..Default::default()
                                },
                                &style,
                            ),
                        );
                        Some(Role::Code)
                    }
                    "blockquote" => {
                        s = merge(
                            &s,
                            &merge(
                                &Style {
                                    padding: Some(Edges::Sides(EdgeSides {
                                        left: Some(10.0),
                                        ..Default::default()
                                    })),
                                    color: Some(hex("#4b5563")),
                                    ..Default::default()
                                },
                                &style,
                            ),
                        );
                        Some(Role::Blockquote)
                    }
                    "figcaption" => Some(Role::Caption),
                    _ => None,
                };
                vec![text_node(text, some_style(s), role)]
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let content = self.content(&children);
                let style = self.style(id);
                vec![with_style(
                    Node::Heading(Heading {
                        id: None,
                        level: tag.as_bytes()[1] - b'0',
                        style: None,
                        keep_with_next: None,
                        keep_together: None,
                        content,
                    }),
                    style,
                )]
            }
            "img" => self.image(id),
            "svg" => self.inline_svg(id),
            "a" => {
                let style = self.style(id);
                let href = self.href(id);
                let mut inner = self.blocks(&children, false);
                if href.is_some()
                    && inner.len() == 1
                    && is_empty(&style)
                    && let Node::Image(i) = &mut inner[0]
                {
                    i.link = href;
                    return inner;
                }
                vec![with_style(
                    Node::View(View {
                        link: href,
                        children: inner,
                        ..Default::default()
                    }),
                    style,
                )]
            }
            "table" => vec![self.html_table(id)],
            "hr" => {
                let style = self.style(id);
                let s = merge(
                    &Style {
                        height: Some(Dimension::Points(0.75)),
                        background_color: Some(hex("#e5e7eb")),
                        margin: Some(Edges::Sides(EdgeSides {
                            top: Some(8.0),
                            bottom: Some(8.0),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    &style,
                );
                vec![view(vec![], Some(s), Some(Role::Artifact))]
            }
            "ul" | "ol" => vec![self.list(id)],
            "input" | "textarea" | "select" => self.form_control(id),
            // a label around a control: side by side, as a browser lays them out inline
            "label" => {
                let kids = self.blocks(&children, true);
                vec![self.label_row(id, kids)]
            }
            "script" | "style" | "head" | "title" | "meta" | "link" | "template" | "noscript" => {
                vec![]
            }
            "section" | "article" => {
                let flex = self.item_layout(id);
                let kids = self.blocks(&children, flex);
                vec![with_style(
                    view(kids, None, Some(Role::Section)),
                    self.style(id),
                )]
            }
            // div, header, footer, main, nav, button, li…: a box
            _ => {
                let flex = self.item_layout(id);
                let kids = self.blocks(&children, flex);
                vec![with_style(view(kids, None, None), self.style(id))]
            }
        }
    }

    /// Preformatted text: indentation and runs of spaces become non-breaking spaces.
    fn pre_content(&self, id: Id) -> TextContent {
        fn text(dom: &Dom, ids: &[Id], out: &mut String) {
            for &c in ids {
                match &dom.nodes[c].data {
                    Data::Text(t) => out.push_str(t),
                    Data::Element(e) if e.tag == "br" => out.push('\n'),
                    Data::Element(_) => text(dom, dom.children(c), out),
                    _ => {}
                }
            }
        }
        let mut raw = String::new();
        text(self.dom, self.dom.children(id), &mut raw);
        let raw = raw.strip_suffix('\n').unwrap_or(&raw);
        TextContent::Plain(keep_spaces(raw))
    }

    /// `width` / `height` attributes (numbers are CSS pixels), under the CSS of the element.
    fn size_attrs(&self, e: &Element, style: Style) -> Style {
        let len = |v: Option<&str>| {
            let v = v?.trim();
            if v.ends_with('%') {
                return None;
            }
            css_length(v, ROOT_FONT_SIZE).map(Dimension::Points)
        };
        merge(
            &Style {
                width: len(e.attr("width")),
                height: len(e.attr("height")),
                ..Default::default()
            },
            &style,
        )
    }

    fn image(&mut self, id: Id) -> Vec<Node> {
        let e = self.el(id).expect("an element");
        let mut style = self.style(id);
        let (mut src, format) = images::source(e.attr("src").unwrap_or(""));
        if let turbopdf_tree::ImageSource::Qr(q) = &mut src {
            // a QR code takes its colours from CSS (`color`, `background-color`) and `data-ecc`
            q.color = style.color.take();
            q.background = style.background_color.take();
            q.ecc = match e.attr("data-ecc").map(str::to_ascii_uppercase).as_deref() {
                Some("L") => Some(QrEcc::L),
                Some("M") => Some(QrEcc::M),
                Some("Q") => Some(QrEcc::Q),
                Some("H") => Some(QrEcc::H),
                _ => None,
            };
        }
        let style = self.size_attrs(e, style);
        vec![with_style(
            Node::Image(Image {
                id: None,
                src,
                format,
                alt: e.attr("alt").map(String::from),
                fit: None,
                style: None,
                link: None,
            }),
            style,
        )]
    }

    fn inline_svg(&mut self, id: Id) -> Vec<Node> {
        let e = self.el(id).expect("an element");
        let style = self.size_attrs(e, self.style(id));
        vec![with_style(
            Node::Image(Image {
                id: None,
                src: images::svg(&self.dom.svg_source(id)),
                format: Some(turbopdf_tree::ImageFormat::Svg),
                alt: e.attr("aria-label").map(String::from),
                fit: None,
                style: None,
                link: None,
            }),
            style,
        )]
    }

    fn list(&mut self, id: Id) -> Node {
        let ordered = self.tag(id) == "ol";
        let mut n: i64 = self
            .el(id)
            .and_then(|e| e.attr("start"))
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        let items: Vec<Id> = self
            .dom
            .element_children(id)
            .filter(|&c| self.tag(c) == "li")
            .collect();
        let mut children = Vec::with_capacity(items.len());
        for li in items {
            let label = if ordered {
                let l = format!("{n}.");
                n += 1;
                l
            } else {
                "•".into()
            };
            let body = self.blocks(self.dom.children(li), false);
            let item_style = merge(
                &Style {
                    flex_direction: Some(FlexDirection::Row),
                    margin: Some(bottom(2.0)),
                    ..Default::default()
                },
                &self.style(li),
            );
            children.push(view(
                vec![
                    text_node(
                        TextContent::Plain(label),
                        Some(Style {
                            width: Some(Dimension::Points(14.0)),
                            flex_shrink: Some(0.0),
                            ..Default::default()
                        }),
                        Some(Role::ListLabel),
                    ),
                    view(
                        body,
                        Some(Style {
                            flex_grow: Some(1.0),
                            flex_shrink: Some(1.0),
                            ..Default::default()
                        }),
                        Some(Role::ListBody),
                    ),
                ],
                Some(item_style),
                Some(Role::ListItem),
            ));
        }
        let style = merge(
            &Style {
                margin: Some(bottom(8.0)),
                ..Default::default()
            },
            &self.style(id),
        );
        view(
            children,
            Some(style),
            Some(if ordered {
                Role::OrderedList
            } else {
                Role::List
            }),
        )
    }
}

fn is_blank(c: &TextContent) -> bool {
    match c {
        TextContent::Plain(s) => s.trim().is_empty(),
        TextContent::Spans(s) => s.iter().all(|s| s.text.trim().is_empty()),
    }
}

/// Indentation and runs of spaces → non-breaking spaces (the engine collapses ordinary spaces). Single
/// spaces between words stay normal, so copy / paste and search keep working.
pub fn keep_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        out.extend(std::iter::repeat_n('\u{a0}', indent));
        let mut run = 0;
        for c in line[indent..].chars() {
            if c == ' ' {
                run += 1;
                continue;
            }
            if run == 1 {
                out.push(' ');
            } else if run > 1 {
                out.push(' ');
                out.extend(std::iter::repeat_n('\u{a0}', run - 1));
            }
            run = 0;
            out.push(c);
        }
        if run == 1 {
            out.push(' ');
        } else if run > 1 {
            out.push(' ');
            out.extend(std::iter::repeat_n('\u{a0}', run - 1));
        }
    }
    out
}
