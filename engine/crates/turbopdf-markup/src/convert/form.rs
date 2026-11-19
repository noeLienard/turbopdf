//! HTML form controls (`<input>`, `<textarea>`, `<select>`) → boxes carrying a [`FormField`]: drawn like a
//! browser would (a bordered box with the value), reported in the render output so TurboPDF Pro can turn
//! them into fillable PDF fields.

use super::{Converter, hex, text_node, view};
use crate::dom::{Data, Id};
use crate::style::merge;
use turbopdf_tree::{
    AlignItems, Border, Dimension, EdgeSides, Edges, FieldKind, FlexDirection, FormField,
    JustifyContent, Node, Style, TextContent, View,
};

const BORDER: &str = "#767676";
const PLACEHOLDER: &str = "#9ca3af";

impl Converter<'_> {
    pub(super) fn form_control(&mut self, id: Id) -> Vec<Node> {
        let e = self.el(id).expect("an element");
        let tag = e.tag.clone();
        let ty = e.attr("type").unwrap_or("text").to_ascii_lowercase();
        let attr = |n: &str| e.attr(n).map(str::to_string);
        let name = attr("name").or_else(|| attr("id"));
        let required = e.has("required").then_some(true);
        let read_only = (e.has("readonly") || e.has("disabled")).then_some(true);
        let max_length = e.attr("maxlength").and_then(|v| v.trim().parse().ok());
        let placeholder = attr("placeholder");
        let label = self.field_label(id);
        let css = self.style(id);

        let kind = match (tag.as_str(), ty.as_str()) {
            ("textarea", _) => FieldKind::Text,
            ("select", _) => FieldKind::Select,
            ("input", "checkbox") => FieldKind::Checkbox,
            ("input", "radio") => FieldKind::Radio,
            ("input", "hidden" | "file" | "image") => return vec![],
            ("input", "submit" | "button" | "reset") => {
                // a button: drawn, not a field
                let text = attr("value").unwrap_or_else(|| {
                    if ty == "reset" {
                        "Reset".into()
                    } else {
                        "Submit".into()
                    }
                });
                let s = merge(&boxed(Edges::Sides(sides(3.0, 8.0))), &css);
                return vec![view(vec![plain(text, None)], Some(s), None)];
            }
            _ => FieldKind::Text,
        };
        // without a name, a control is drawn but cannot be filled in
        let Some(name) = name else {
            return self.unnamed(kind, &css);
        };
        let mut field = FormField {
            name,
            kind,
            value: None,
            checked: None,
            options: None,
            option_labels: None,
            multiline: None,
            required,
            read_only,
            max_length,
            label,
        };
        let (children, base) = match kind {
            FieldKind::Checkbox | FieldKind::Radio => {
                let checked = e.has("checked");
                field.checked = Some(checked);
                field.value = Some(attr("value").unwrap_or_else(|| "on".into()));
                (mark(kind, checked), tick_box(kind))
            }
            FieldKind::Select => {
                let options = self.options(id);
                let chosen = options.iter().find(|o| o.selected).or(options.first());
                field.value = chosen.map(|o| o.value.clone());
                let shown = chosen.map(|o| o.text.clone()).unwrap_or_default();
                if options.iter().any(|o| o.text != o.value) {
                    field.option_labels = Some(options.iter().map(|o| o.text.clone()).collect());
                }
                field.options = Some(options.into_iter().map(|o| o.value).collect());
                (vec![plain(nbsp(shown), None)], text_box(None))
            }
            _ if tag == "textarea" => {
                field.multiline = Some(true);
                let value = self.raw_text(id);
                let rows = e
                    .attr("rows")
                    .and_then(|r| r.trim().parse::<f32>().ok())
                    .unwrap_or(2.0)
                    .max(1.0);
                field.value = (!value.is_empty()).then(|| value.clone());
                (
                    vec![value_or_placeholder(value, placeholder)],
                    text_box(Some(rows)),
                )
            }
            _ => {
                let value = attr("value").unwrap_or_default();
                field.value = (!value.is_empty()).then(|| value.clone());
                (
                    vec![value_or_placeholder(value, placeholder)],
                    text_box(None),
                )
            }
        };
        vec![Node::View(View {
            style: Some(merge(&base, &css)),
            field: Some(field),
            children,
            ..Default::default()
        })]
    }

    pub(super) fn label_row(&self, id: Id, children: Vec<Node>) -> Node {
        let row = Style {
            flex_direction: Some(FlexDirection::Row),
            align_items: Some(AlignItems::Center),
            gap: Some(4.0),
            margin: Some(Edges::Sides(EdgeSides {
                bottom: Some(3.0),
                ..Default::default()
            })),
            ..Default::default()
        };
        view(children, Some(merge(&row, &self.style(id))), None)
    }

    /// What the field is for: `aria-label`, `title`, `<label for>`, the enclosing `<label>`, `placeholder`.
    fn field_label(&self, id: Id) -> Option<String> {
        let e = self.el(id)?;
        if let Some(l) = e
            .attr("aria-label")
            .or(e.attr("title"))
            .filter(|l| !l.trim().is_empty())
        {
            return Some(l.trim().to_string());
        }
        if let Some(own) = e.attr("id") {
            for (i, n) in self.dom.nodes.iter().enumerate() {
                if let Data::Element(l) = &n.data
                    && l.tag == "label"
                    && l.attr("for") == Some(own)
                {
                    return clean(self.raw_text(i));
                }
            }
        }
        let mut cur = self.dom.nodes[id].parent;
        while let Some(p) = cur {
            if self.el(p).is_some_and(|l| l.tag == "label") {
                return clean(self.raw_text(p));
            }
            cur = self.dom.nodes[p].parent;
        }
        e.attr("placeholder").and_then(|p| clean(p.to_string()))
    }

    /// `<option>`s, also inside `<optgroup>`.
    fn options(&self, id: Id) -> Vec<Choice> {
        let mut out = Vec::new();
        for &c in self.dom.children(id) {
            let Some(o) = self.el(c) else { continue };
            match o.tag.as_str() {
                "option" => {
                    let text = self
                        .raw_text(c)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    let value = o
                        .attr("value")
                        .map(str::to_string)
                        .unwrap_or_else(|| text.clone());
                    out.push(Choice {
                        value,
                        text,
                        selected: o.has("selected"),
                    });
                }
                "optgroup" => out.extend(self.options(c)),
                _ => {}
            }
        }
        out
    }

    /// The text inside an element, as written (form controls are not part of it).
    fn raw_text(&self, id: Id) -> String {
        fn walk(c: &Converter, id: Id, out: &mut String) {
            for &k in c.dom.children(id) {
                match &c.dom.nodes[k].data {
                    Data::Text(t) => out.push_str(t),
                    Data::Element(e)
                        if matches!(e.tag.as_str(), "input" | "select" | "textarea") => {}
                    Data::Element(_) => walk(c, k, out),
                    _ => {}
                }
            }
        }
        let mut s = String::new();
        walk(self, id, &mut s);
        s
    }

    /// A control without a name: drawn, not a field.
    fn unnamed(&self, kind: FieldKind, css: &Style) -> Vec<Node> {
        let base = match kind {
            FieldKind::Checkbox | FieldKind::Radio => tick_box(kind),
            _ => text_box(None),
        };
        vec![view(vec![], Some(merge(&base, css)), None)]
    }
}

struct Choice {
    value: String,
    text: String,
    selected: bool,
}

fn clean(s: String) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_end_matches([':', ' ']).to_string();
    (!s.is_empty()).then_some(s)
}

fn sides(v: f32, h: f32) -> EdgeSides {
    EdgeSides {
        top: Some(v),
        right: Some(h),
        bottom: Some(v),
        left: Some(h),
    }
}

fn boxed(padding: Edges) -> Style {
    Style {
        margin: Some(Edges::Sides(EdgeSides {
            bottom: Some(4.0),
            ..Default::default()
        })),
        border: Some(Border {
            width: 0.75,
            color: hex(BORDER),
            style: None,
        }),
        border_radius: Some(2.0),
        padding: Some(padding),
        ..Default::default()
    }
}

/// A text box: 160 pt wide like a browser's, `rows` lines tall for a textarea.
fn text_box(rows: Option<f32>) -> Style {
    Style {
        width: Some(Dimension::Points(160.0)),
        min_height: rows.map(|r| Dimension::Points(r * 13.2 + 6.0)),
        ..boxed(Edges::Sides(sides(3.0, 4.0)))
    }
}

fn tick_box(kind: FieldKind) -> Style {
    Style {
        width: Some(Dimension::Points(10.0)),
        height: Some(Dimension::Points(10.0)),
        border: Some(Border {
            width: 0.75,
            color: hex(BORDER),
            style: None,
        }),
        border_radius: Some(if kind == FieldKind::Radio { 5.0 } else { 1.5 }),
        align_items: Some(AlignItems::Center),
        justify_content: Some(JustifyContent::Center),
        ..Default::default()
    }
}

/// The mark of a checked box: a filled square, or a dot for a radio.
fn mark(kind: FieldKind, checked: bool) -> Vec<Node> {
    if !checked {
        return vec![];
    }
    let s = Style {
        width: Some(Dimension::Points(5.0)),
        height: Some(Dimension::Points(5.0)),
        background_color: Some(hex("#111827")),
        border_radius: Some(if kind == FieldKind::Radio { 2.5 } else { 0.5 }),
        ..Default::default()
    };
    vec![view(vec![], Some(s), None)]
}

fn plain(text: String, style: Option<Style>) -> Node {
    text_node(TextContent::Plain(text), style, None)
}

/// An empty box keeps the height of one line.
fn nbsp(s: String) -> String {
    if s.is_empty() { "\u{a0}".into() } else { s }
}

fn value_or_placeholder(value: String, placeholder: Option<String>) -> Node {
    match placeholder {
        Some(p) if value.is_empty() => plain(
            p,
            Some(Style {
                color: Some(hex(PLACEHOLDER)),
                ..Default::default()
            }),
        ),
        _ => plain(nbsp(value), None),
    }
}
