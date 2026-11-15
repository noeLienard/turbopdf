//! HTML → a flat DOM (an arena of nodes), parsed by html5ever: the WHATWG algorithm browsers use, so
//! unclosed `<p>` / `<li>`, `<table>` without `<tbody>`, misnested tags and entities behave as in a browser.

use html5ever::interface::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, ParseOpts, QualName, local_name, ns, parse_document};
use std::borrow::Cow;
use std::cell::{Ref, RefCell};

pub type Id = usize;

#[derive(Debug)]
pub struct Node {
    pub parent: Option<Id>,
    pub children: Vec<Id>,
    pub data: Data,
}

#[derive(Debug)]
pub enum Data {
    Document,
    Element(Element),
    Text(String),
    /// Comments, doctypes, processing instructions: ignored.
    Other,
}

#[derive(Debug)]
pub struct Element {
    pub name: QualName,
    /// Local name: lowercase for HTML, as written for SVG (`linearGradient`).
    pub tag: String,
    /// Names lowercase for HTML; `xlink:href` style for namespaced attributes.
    pub attrs: Vec<(String, String)>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.attrs.iter().any(|(k, _)| k == name)
    }

    pub fn is_svg(&self) -> bool {
        self.name.ns == ns!(svg)
    }
}

/// A parsed document. Node 0 is the document itself.
#[derive(Debug)]
pub struct Dom {
    pub nodes: Vec<Node>,
}

impl Dom {
    pub fn parse(html: &str) -> Dom {
        let sink = Sink {
            nodes: RefCell::new(vec![Node {
                parent: None,
                children: Vec::new(),
                data: Data::Document,
            }]),
        };
        parse_document(sink, ParseOpts::default()).one(StrTendril::from(html))
    }

    pub fn element(&self, id: Id) -> Option<&Element> {
        match &self.nodes[id].data {
            Data::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn tag(&self, id: Id) -> Option<&str> {
        self.element(id).map(|e| e.tag.as_str())
    }

    pub fn children(&self, id: Id) -> &[Id] {
        &self.nodes[id].children
    }

    /// The element children of `id`, in order.
    pub fn element_children(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        self.nodes[id]
            .children
            .iter()
            .copied()
            .filter(|&c| matches!(self.nodes[c].data, Data::Element(_)))
    }

    /// First descendant element (depth first) with this tag.
    pub fn find(&self, from: Id, tag: &str) -> Option<Id> {
        for &c in &self.nodes[from].children {
            if self.tag(c) == Some(tag) {
                return Some(c);
            }
            if let Some(found) = self.find(c, tag) {
                return Some(found);
            }
        }
        None
    }

    /// Concatenated text of the direct text children (`<style>`, `<title>`).
    pub fn own_text(&self, id: Id) -> String {
        self.nodes[id]
            .children
            .iter()
            .filter_map(|&c| match &self.nodes[c].data {
                Data::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Append a new node under `parent`, at `index` (or at the end).
    pub fn insert(&mut self, parent: Id, index: Option<usize>, data: Data) -> Id {
        let id = self.nodes.len();
        self.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            data,
        });
        let children = &mut self.nodes[parent].children;
        match index {
            Some(i) => children.insert(i, id),
            None => children.push(id),
        }
        id
    }

    /// The SVG element `id` as standalone XML (attribute case kept, `xmlns` added when missing).
    pub fn svg_source(&self, id: Id) -> String {
        let mut out = String::new();
        self.write_xml(id, true, &mut out);
        out
    }

    fn write_xml(&self, id: Id, root: bool, out: &mut String) {
        match &self.nodes[id].data {
            Data::Text(t) => escape(t, false, out),
            Data::Element(e) => {
                out.push('<');
                out.push_str(&e.tag);
                if root {
                    if !e.has("xmlns") {
                        out.push_str(r#" xmlns="http://www.w3.org/2000/svg""#);
                    }
                    if !e.has("xmlns:xlink") && self.uses_xlink(id) {
                        out.push_str(r#" xmlns:xlink="http://www.w3.org/1999/xlink""#);
                    }
                }
                for (k, v) in &e.attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    escape(v, true, out);
                    out.push('"');
                }
                if self.nodes[id].children.is_empty() {
                    out.push_str("/>");
                    return;
                }
                out.push('>');
                for &c in &self.nodes[id].children {
                    self.write_xml(c, false, out);
                }
                out.push_str("</");
                out.push_str(&e.tag);
                out.push('>');
            }
            _ => {}
        }
    }

    fn uses_xlink(&self, id: Id) -> bool {
        self.element(id)
            .is_some_and(|e| e.attrs.iter().any(|(k, _)| k.starts_with("xlink:")))
            || self.nodes[id].children.iter().any(|&c| self.uses_xlink(c))
    }
}

fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

// ------------------------------------------------------------------ html5ever tree sink

struct Sink {
    nodes: RefCell<Vec<Node>>,
}

impl Sink {
    fn new_node(&self, data: Data) -> Id {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(Node {
            parent: None,
            children: Vec::new(),
            data,
        });
        nodes.len() - 1
    }

    fn detach(&self, id: Id) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(p) = nodes[id].parent.take() {
            nodes[p].children.retain(|&c| c != id);
        }
    }

    /// Text merges into a preceding text node, as in the DOM.
    fn place(&self, parent: Id, index: Option<usize>, child: NodeOrText<Id>) {
        let id = match child {
            NodeOrText::AppendNode(id) => {
                self.detach(id);
                id
            }
            NodeOrText::AppendText(text) => {
                let mut nodes = self.nodes.borrow_mut();
                let at = index.unwrap_or(nodes[parent].children.len());
                if at > 0 {
                    let prev = nodes[parent].children[at - 1];
                    if let Data::Text(t) = &mut nodes[prev].data {
                        t.push_str(&text);
                        return;
                    }
                }
                drop(nodes);
                self.new_node(Data::Text(text.to_string()))
            }
        };
        let mut nodes = self.nodes.borrow_mut();
        nodes[id].parent = Some(parent);
        let children = &mut nodes[parent].children;
        match index {
            Some(i) => children.insert(i, id),
            None => children.push(id),
        }
    }
}

fn attr_name(a: &Attribute) -> String {
    match &a.name.prefix {
        Some(p) => format!("{p}:{}", a.name.local),
        None => a.name.local.to_string(),
    }
}

impl TreeSink for Sink {
    type Handle = Id;
    type Output = Dom;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Dom {
        Dom {
            nodes: self.nodes.into_inner(),
        }
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Id {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a Id) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |n| match &n[*target].data {
            Data::Element(e) => &e.name,
            _ => panic!("not an element"),
        })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, _flags: ElementFlags) -> Id {
        let tag = name.local.to_string();
        let attrs = attrs
            .iter()
            .map(|a| (attr_name(a), a.value.to_string()))
            .collect();
        self.new_node(Data::Element(Element { name, tag, attrs }))
    }

    fn create_comment(&self, _text: StrTendril) -> Id {
        self.new_node(Data::Other)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Id {
        self.new_node(Data::Other)
    }

    fn append(&self, parent: &Id, child: NodeOrText<Id>) {
        self.place(*parent, None, child);
    }

    fn append_based_on_parent_node(&self, element: &Id, prev: &Id, child: NodeOrText<Id>) {
        if self.nodes.borrow()[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, _target: &Id) -> Id {
        // template contents are never rendered: parse them into a detached node
        self.new_node(Data::Document)
    }

    fn same_node(&self, x: &Id, y: &Id) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &Id, child: NodeOrText<Id>) {
        let (parent, index) = {
            let nodes = self.nodes.borrow();
            let parent = nodes[*sibling].parent.expect("sibling has a parent");
            let index = nodes[parent]
                .children
                .iter()
                .position(|c| c == sibling)
                .expect("sibling is a child of its parent");
            (parent, index)
        };
        if let NodeOrText::AppendNode(id) = &child {
            // detaching first would shift the index when the node comes from before the sibling
            let same_parent_before = {
                let nodes = self.nodes.borrow();
                nodes[*id].parent == Some(parent)
                    && nodes[parent].children.iter().position(|c| c == id) < Some(index)
            };
            self.detach(*id);
            let index = if same_parent_before { index - 1 } else { index };
            self.place(parent, Some(index), child);
            return;
        }
        self.place(parent, Some(index), child);
    }

    fn add_attrs_if_missing(&self, target: &Id, attrs: Vec<Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        if let Data::Element(e) = &mut nodes[*target].data {
            for a in &attrs {
                let name = attr_name(a);
                if !e.has(&name) {
                    e.attrs.push((name, a.value.to_string()));
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Id) {
        self.detach(*target);
    }

    fn reparent_children(&self, node: &Id, new_parent: &Id) {
        let mut nodes = self.nodes.borrow_mut();
        let moved = std::mem::take(&mut nodes[*node].children);
        for &c in &moved {
            nodes[c].parent = Some(*new_parent);
        }
        nodes[*new_parent].children.extend(moved);
    }
}

/// `<html>`, `<head>` and `<body>` of a parsed document (html5ever always creates them).
pub struct Skeleton {
    pub html: Id,
    pub head: Option<Id>,
    pub body: Option<Id>,
}

impl Dom {
    pub fn skeleton(&self) -> Skeleton {
        let html = self
            .element_children(0)
            .find(|&c| {
                self.element(c)
                    .is_some_and(|e| e.name.local == local_name!("html"))
            })
            .unwrap_or(0);
        let child = |tag: &str| {
            self.element_children(html)
                .find(|&c| self.tag(c) == Some(tag))
        };
        Skeleton {
            html,
            head: child("head"),
            body: child("body"),
        }
    }
}
