//! The CSS cascade: stylesheets (`<style>`, local `<link>`, the `css` option) and inline `style` attributes
//! → the declarations that apply to every element, in cascade order (last wins).
//!
//! Selectors: type, `*`, `.class`, `#id`, attributes (`[a]`, `=`, `~=`, `|=`, `^=`, `$=`, `*=`, and the `i`
//! flag), the descendant, `>`, `+` and `~` combinators, `:first-child`, `:last-child`, `:only-child`,
//! `:nth-child()`, `:nth-last-child()`, `:first-of-type`, `:last-of-type`, `:only-of-type`, `:nth-of-type()`,
//! `:nth-last-of-type()`, `:empty`, `:root`, `:not()` / `:is()` / `:where()` (with compound selectors),
//! `::before` / `::after`, selector lists. Interactive states (`:hover`, `:focus`…) never match on paper;
//! anything else unsupported is reported (see [`Stylesheet::unsupported`]). At-rules: `@media` (print /
//! all applied, screen-only skipped), `@page { size; margin }`, `@font-face`; the rest is ignored.
//!
//! Rules are filed under the id, first class or tag of their rightmost compound (browsers' rule hash), so
//! an element only tests the selectors that can match it.

use crate::dom::{Data, Dom, Element, Id};
use crate::style::{Decl, declarations, split_top};
use std::collections::HashMap;

mod selector;
use selector::*;

#[derive(Debug, Default, Clone)]
pub struct PageRule {
    pub size: Option<String>,
    pub margin: Option<String>,
}

/// An `@font-face` rule. `src` lists the `url(…)`s in order, resolved against the stylesheet's folder.
#[derive(Debug, Clone)]
pub struct FontFaceRule {
    pub family: String,
    pub src: Vec<(String, Option<String>)>,
    pub weight: Option<String>,
    pub style: Option<String>,
}

/// `An+B` among the siblings, counted from the start or the end, all of them or those of the same type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Nth {
    a: i64,
    b: i64,
    from_end: bool,
    of_type: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Pseudo {
    Nth(Nth),
    /// `:only-child` / `:only-of-type`
    Only {
        of_type: bool,
    },
    Empty,
    Root,
    /// `:not(a, b)`: none of them matches.
    Not(Vec<Compound>),
    /// `:is(a, b)` / `:where(a, b)`: one of them matches (`:where` adds no specificity).
    Is {
        list: Vec<Compound>,
        counts: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    /// `=`
    Equals,
    /// `~=`: one of the space-separated words
    Word,
    /// `|=`: equal, or followed by `-`
    Lang,
    /// `^=`
    Prefix,
    /// `$=`
    Suffix,
    /// `*=`
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
struct AttrTest {
    name: String,
    /// `None`: `[name]`, the attribute is present.
    value: Option<(AttrOp, String)>,
    /// `[a="x" i]`
    ignore_case: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrTest>,
    pseudo: Vec<Pseudo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    /// `+`
    Adjacent,
    /// `~`
    Sibling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Generated {
    Before,
    After,
}

#[derive(Debug)]
struct Selector {
    /// Left → right.
    parts: Vec<Compound>,
    /// Keys (tag, `.class`, `#id`) every ancestor compound needs, hashed: checked against the
    /// ancestor filter before walking up the tree.
    ancestor_keys: Vec<u32>,
    combinators: Vec<Combinator>,
    specificity: u32,
    generated: Option<Generated>,
}

#[derive(Debug)]
struct Rule {
    selectors: Vec<Selector>,
    declarations: Vec<(String, String, bool)>,
}

#[derive(Debug, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    pub page: PageRule,
    pub font_faces: Vec<FontFaceRule>,
    /// key (`#id`, `.class`, `tag`, `*`) → (rule, selector) indices.
    index: HashMap<String, Vec<(usize, usize)>>,
    /// What the stylesheets use that is not supported, in order of first use, each with its count:
    /// `("selector", "p::first-line")`, `("property", "box-shadow")`.
    pub unsupported: Vec<(Unsupported, usize)>,
}

/// A selector or property the converter does not support (it is ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    Selector(String),
    Property(String),
}

impl Stylesheet {
    pub(crate) fn report(list: &mut Vec<(Unsupported, usize)>, what: Unsupported) {
        match list.iter_mut().find(|(w, _)| *w == what) {
            Some((_, n)) => *n += 1,
            None => list.push((what, 1)),
        }
    }
}

/// A stylesheet's text, and the folder its relative `url(…)`s are relative to (`""`: the document's).
pub struct StyleSource<'a> {
    pub css: &'a str,
    pub base: &'a str,
}

impl Stylesheet {
    pub fn parse(sources: &[StyleSource]) -> Stylesheet {
        let mut sheet = Stylesheet::default();
        for s in sources {
            sheet.walk(&strip_comments(s.css), s.base);
        }
        for (r, rule) in sheet.rules.iter().enumerate() {
            for (i, sel) in rule.selectors.iter().enumerate() {
                let last = sel.parts.last().expect("a selector has a compound");
                let key = if let Some(id) = &last.id {
                    format!("#{id}")
                } else if let Some(c) = last.classes.first() {
                    format!(".{c}")
                } else {
                    last.tag.clone().unwrap_or_else(|| "*".into())
                };
                sheet.index.entry(key).or_default().push((r, i));
            }
        }
        sheet
    }

    fn walk(&mut self, css: &str, base: &str) {
        for (prelude, body) in blocks_of(css) {
            if let Some(at) = prelude.strip_prefix('@') {
                let name: String = at
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect::<String>()
                    .to_ascii_lowercase();
                match name.as_str() {
                    "media" => {
                        let q = at[5..].to_ascii_lowercase();
                        if q.contains("print") || q.contains("all") || !q.contains("screen") {
                            self.walk(body, base);
                        }
                    }
                    "font-face" => {
                        if let Some(f) = font_face(body, base) {
                            self.font_faces.push(f);
                        }
                    }
                    "page" => {
                        for (k, v, _) in declarations(body) {
                            match k.as_str() {
                                "size" => self.page.size = Some(v),
                                "margin" => self.page.margin = Some(v),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let mut selectors: Vec<Selector> = Vec::new();
            for src in split_top(prelude, ',') {
                match selector(src) {
                    Some(sel) => selectors.push(sel),
                    // `:hover` and co. never match on paper: not worth a warning
                    None if !is_interactive(src) => Self::report(
                        &mut self.unsupported,
                        Unsupported::Selector(src.trim().to_string()),
                    ),
                    None => {}
                }
            }
            if !selectors.is_empty() {
                let declarations = declarations(body);
                for (k, _, _) in &declarations {
                    if crate::style::support(k) == crate::style::Support::No {
                        Self::report(&mut self.unsupported, Unsupported::Property(k.clone()));
                    }
                }
                self.rules.push(Rule {
                    selectors,
                    declarations,
                });
            }
        }
    }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Top-level `prelude { body }` blocks. Statement at-rules (`@import …;`) are skipped.
fn blocks_of(css: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let b = css.as_bytes();
    let mut i = 0;
    while i < css.len() {
        let open = css[i..].find('{').map(|x| x + i);
        // only up to the next block: searching the whole rest of the text for each rule is quadratic
        let semi = css[i..open.unwrap_or(css.len())].find(';').map(|x| x + i);
        if let Some(s) = semi
            && css[i..s].trim().starts_with('@')
        {
            i = s + 1;
            continue;
        }
        let Some(open) = open else { break };
        let mut depth = 1;
        let mut j = open + 1;
        while j < b.len() && depth > 0 {
            match b[j] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let end = if depth == 0 { j - 1 } else { j };
        out.push((css[i..open].trim(), &css[open + 1..end]));
        i = j;
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

fn is_absolute_url(url: &str) -> bool {
    url.starts_with('/') || scheme(url).is_some()
}

/// `https:` in `https://…`, `data:` in `data:…`.
pub fn scheme(url: &str) -> Option<&str> {
    let i = url.find(':')?;
    let s = &url[..i];
    (s.len() > 1
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)))
    .then_some(s)
}

fn font_face(body: &str, base: &str) -> Option<FontFaceRule> {
    let mut rule = FontFaceRule {
        family: String::new(),
        src: Vec::new(),
        weight: None,
        style: None,
    };
    for (k, v, _) in declarations(body) {
        match k.as_str() {
            "font-family" => rule.family = unquote(&v),
            "font-weight" => rule.weight = Some(v),
            "font-style" => rule.style = Some(v),
            "src" => {
                for part in split_top(&v, ',') {
                    let part = part.trim();
                    let Some(start) = part.to_ascii_lowercase().find("url(") else {
                        continue;
                    };
                    let after = &part[start + 4..];
                    let Some(close) = after.find(')') else {
                        continue;
                    };
                    let url = unquote(&after[..close]);
                    let rest = after[close + 1..].trim();
                    let format = rest
                        .to_ascii_lowercase()
                        .strip_prefix("format(")
                        .and_then(|f| f.split(')').next().map(|f| unquote(f).to_ascii_lowercase()));
                    let url = if !base.is_empty() && !is_absolute_url(&url) {
                        format!("{}/{url}", base.trim_end_matches('/'))
                    } else {
                        url
                    };
                    rule.src.push((url, format));
                }
            }
            _ => {}
        }
    }
    (!rule.family.is_empty() && !rule.src.is_empty()).then_some(rule)
}

// ------------------------------------------------------------------ variables, generated content

/// Expand `var(--x, fallback)`. Cycles and missing variables invalidate the declaration.
fn expand(value: &str, vars: &HashMap<String, String>, stack: &mut Vec<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("var(") {
        out.push_str(&rest[..at]);
        let body_start = at + 4;
        let b = rest.as_bytes();
        let mut depth = 1;
        let mut end = body_start;
        while end < b.len() && depth > 0 {
            match b[end] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        if depth > 0 {
            return None;
        }
        let body = &rest[body_start..end - 1];
        let (name, fallback) = match split_top(body, ',').first() {
            Some(first) if first.len() < body.len() => {
                (first.trim(), Some(body[first.len() + 1..].trim()))
            }
            _ => (body.trim(), None),
        };
        if !name.starts_with("--") || stack.iter().any(|s| s == name) {
            return None;
        }
        let raw = vars.get(name).map(String::as_str).or(fallback)?;
        stack.push(name.to_string());
        let expanded = expand(raw, vars, stack);
        stack.pop();
        out.push_str(&expanded?);
        rest = &rest[end..];
    }
    out.push_str(rest);
    Some(out)
}

fn generated_content(value: &str, e: &Element) -> Option<String> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("normal") {
        return None;
    }
    let mut out = String::new();
    let mut found = false;
    let mut chars = v.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '"' || c == '\'' {
            found = true;
            let mut escaped = false;
            for (_, d) in chars.by_ref() {
                if escaped {
                    out.push(d);
                    escaped = false;
                } else if d == '\\' {
                    escaped = true;
                } else if d == c {
                    break;
                } else {
                    out.push(d);
                }
            }
        } else if v[i..].to_ascii_lowercase().starts_with("attr(") {
            let inner = &v[i + 5..];
            if let Some(close) = inner.find(')') {
                found = true;
                let name = inner[..close].trim().to_ascii_lowercase();
                out.push_str(e.attr(&name).unwrap_or(""));
                for _ in 0..(5 + close) {
                    chars.next();
                }
            }
        }
    }
    found.then_some(out)
}

// ------------------------------------------------------------------ cascade

/// The cascaded declarations of every element (indexed by node id), variables expanded.
/// `::before` / `::after` become `<span aria-hidden>` children of their element.
pub struct Computed {
    pub decls: Vec<Vec<Decl>>,
}

impl Computed {
    pub fn get(&self, id: Id) -> &[Decl] {
        self.decls.get(id).map_or(&[], Vec::as_slice)
    }

    /// Last value of one of these properties, lowercase.
    pub fn prop(&self, id: Id, names: &[&str]) -> Option<String> {
        self.get(id)
            .iter()
            .rev()
            .find(|(k, _)| names.contains(&k.as_str()))
            .map(|(_, v)| v.to_ascii_lowercase())
    }
}

struct Hit<'a> {
    decl: &'a (String, String, bool),
    spec: u32,
    order: usize,
}

fn ordered(mut hits: Vec<Hit>, inline: &[(String, String, bool)]) -> Vec<(String, String, bool)> {
    hits.sort_by_key(|h| (h.spec, h.order));
    let normal = hits.iter().filter(|h| !h.decl.2).map(|h| h.decl.clone());
    let important = hits.iter().filter(|h| h.decl.2).map(|h| h.decl.clone());
    normal
        .chain(inline.iter().cloned())
        .chain(important)
        .collect()
}

fn resolve(
    decls: Vec<(String, String, bool)>,
    inherited: &HashMap<String, String>,
) -> (Vec<Decl>, HashMap<String, String>) {
    let mut vars = inherited.clone();
    for (k, v, _) in &decls {
        if k.starts_with("--") {
            vars.insert(k.clone(), v.clone());
        }
    }
    let mut out = Vec::with_capacity(decls.len());
    for (k, v, _) in decls {
        if k.starts_with("--") {
            continue;
        }
        if let Some(e) = expand(&v, &vars, &mut Vec::new()) {
            out.push((k, e));
        }
    }
    (out, vars)
}

pub fn cascade(dom: &mut Dom, sheet: &Stylesheet, roots: &[Id]) -> Computed {
    let mut computed = Computed {
        decls: Vec::with_capacity(dom.nodes.len()),
    };
    let mut filter = AncestorFilter::new();
    for &root in roots {
        apply(
            dom,
            sheet,
            root,
            &mut vec![],
            &mut filter,
            1,
            1,
            &HashMap::new(),
            &mut computed,
        );
    }
    computed
}

#[allow(clippy::too_many_arguments)]
fn apply(
    dom: &mut Dom,
    sheet: &Stylesheet,
    id: Id,
    path: &mut Vec<Ctx>,
    filter: &mut AncestorFilter,
    index: usize,
    count: usize,
    vars: &HashMap<String, String>,
    out: &mut Computed,
) {
    let parent = path.last().map(|c| c.id);
    path.push(Ctx {
        id,
        index,
        count,
        parent,
    });
    let (own, own_vars, before, after) = {
        let e = dom.element(id).expect("an element");
        // candidate selectors: filed under `*`, the tag, the classes or the id of the element
        let mut keys: Vec<String> = vec!["*".into(), e.tag.clone()];
        keys.extend(classes(e).map(|c| format!(".{c}")));
        if let Some(i) = e.attr("id") {
            keys.push(format!("#{i}"));
        }
        keys.sort();
        keys.dedup();
        // per rule and pseudo-element: the highest specificity among its matching selectors
        let mut best: HashMap<(usize, Option<Generated>), u32> = HashMap::new();
        for key in &keys {
            for &(r, s) in sheet.index.get(key).into_iter().flatten() {
                let sel = &sheet.rules[r].selectors[s];
                let k = (r, sel.generated);
                if !sel.ancestor_keys.iter().all(|&h| filter.may_have(h))
                    || best.get(&k).is_some_and(|&spec| spec >= sel.specificity)
                {
                    continue;
                }
                if matches(sel, dom, path) {
                    best.insert(k, sel.specificity);
                }
            }
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut generated: [Vec<Hit>; 2] = [Vec::new(), Vec::new()];
        for (&(r, g), &spec) in &best {
            for d in &sheet.rules[r].declarations {
                let h = Hit {
                    decl: d,
                    spec,
                    order: r,
                };
                match g {
                    None => hits.push(h),
                    Some(Generated::Before) => generated[0].push(h),
                    Some(Generated::After) => generated[1].push(h),
                }
            }
        }
        let inline = declarations(e.attr("style").unwrap_or(""));
        let (own, own_vars) = resolve(ordered(hits, &inline), vars);
        let [b, a] = generated;
        let pseudo = |list: Vec<Hit>| -> Option<(String, Vec<Decl>)> {
            if list.is_empty() {
                return None;
            }
            let (decls, _) = resolve(ordered(list, &[]), &own_vars);
            let content = decls
                .iter()
                .rev()
                .find(|(k, _)| k == "content")
                .and_then(|(_, v)| generated_content(v, e))?;
            Some((
                content,
                decls.into_iter().filter(|(k, _)| k != "content").collect(),
            ))
        };
        let (b, a) = (pseudo(b), pseudo(a));
        (own, own_vars, b, a)
    };
    set(out, id, own);
    // generated content: a `<span aria-hidden>` child, its declarations written inline (it is cascaded
    // like any other child, as in the TypeScript converter it replaces)
    let span = |dom: &mut Dom, at: Option<usize>, (text, decls): (String, Vec<Decl>)| {
        let style = decls
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";");
        let mut attrs = vec![("aria-hidden".to_string(), "true".to_string())];
        if !style.is_empty() {
            attrs.insert(0, ("style".into(), style));
        }
        let el = dom.insert(
            id,
            at,
            Data::Element(Element {
                name: html5ever::QualName::new(
                    None,
                    html5ever::ns!(html),
                    html5ever::local_name!("span"),
                ),
                tag: "span".into(),
                attrs,
            }),
        );
        dom.insert(el, None, Data::Text(text));
    };
    if let Some(b) = before {
        span(dom, Some(0), b);
    }
    if let Some(a) = after {
        span(dom, None, a);
    }
    let children: Vec<Id> = dom.element_children(id).collect();
    let n = children.len();
    filter.push(dom.element(id).expect("an element"));
    for (i, c) in children.into_iter().enumerate() {
        apply(dom, sheet, c, path, filter, i + 1, n, &own_vars, out);
    }
    filter.pop(dom.element(id).expect("an element"));
    path.pop();
}

fn set(out: &mut Computed, id: Id, decls: Vec<Decl>) {
    if out.decls.len() <= id {
        out.decls.resize_with(id + 1, Vec::new);
    }
    out.decls[id] = decls;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `color` each element with an `id` gets from `css`.
    fn colors(html: &str, css: &str) -> HashMap<String, String> {
        let mut dom = Dom::parse(html);
        let sheet = Stylesheet::parse(&[StyleSource { css, base: "" }]);
        let root = dom.skeleton().html;
        let computed = cascade(&mut dom, &sheet, &[root]);
        let mut out = HashMap::new();
        for id in 0..dom.nodes.len() {
            if let Some(e) = dom.element(id)
                && let Some(name) = e.attr("id")
                && let Some(c) = computed.prop(id, &["color"])
            {
                out.insert(name.to_string(), c);
            }
        }
        out
    }

    const LIST: &str = r#"<ul><li id="a">a</li><li id="b" class="x">b</li><li id="c" data-k="pre-v">c</li><li id="d"></li></ul>"#;

    #[test]
    fn sibling_combinators() {
        let c = colors(LIST, "#a + li { color: red } .x ~ li { color: blue }");
        assert_eq!(c.get("b").map(String::as_str), Some("red"));
        assert_eq!(c.get("c").map(String::as_str), Some("blue"));
        assert_eq!(c.get("d").map(String::as_str), Some("blue"));
        assert!(!c.contains_key("a"));
    }

    #[test]
    fn structural_pseudo_classes() {
        let html =
            r#"<div><p id="p1">1</p><span id="s1">s</span><p id="p2">2</p><p id="p3"></p></div>"#;
        let c = colors(
            html,
            "p:first-of-type { color: red } p:last-of-type { color: blue } span:only-of-type { color: green } p:nth-last-child(3) { color: navy } p:empty { color: gray }",
        );
        assert_eq!(c.get("p1").map(String::as_str), Some("red"));
        assert_eq!(c.get("s1").map(String::as_str), Some("green"));
        // p3 is both last-of-type and empty: `:empty` comes later with the same specificity
        assert_eq!(c.get("p3").map(String::as_str), Some("gray"));
        assert!(!c.contains_key("p2"));
        let c = colors(html, "p:nth-of-type(2) { color: red }");
        assert_eq!(c.get("p2").map(String::as_str), Some("red"));
    }

    #[test]
    fn not_is_where_and_attribute_operators() {
        let c = colors(
            LIST,
            "li:not(.x, #d) { color: red } li:is(#d) { color: blue }",
        );
        assert_eq!(c.get("a").map(String::as_str), Some("red"));
        assert_eq!(c.get("c").map(String::as_str), Some("red"));
        assert!(!c.contains_key("b"));
        assert_eq!(c.get("d").map(String::as_str), Some("blue"));
        // `:not(.x, #d)` is as specific as its most specific argument (an id), as in browsers
        let c = colors(
            LIST,
            "li:not(.x, #d) { color: red } [data-k^=pre] { color: green }",
        );
        assert_eq!(c.get("c").map(String::as_str), Some("red"));
        let c = colors(LIST, "[data-k^=pre] { color: green }");
        assert_eq!(c.get("c").map(String::as_str), Some("green"));
        let c = colors(
            LIST,
            r#"[data-k$="-v"] { color: red } [data-k*="E-V" i] { color: blue }"#,
        );
        assert_eq!(c.get("c").map(String::as_str), Some("blue"));
        // `:where()` adds no specificity: the later plain rule wins
        let c = colors(LIST, "li:where(.x) { color: red } li { color: blue }");
        assert_eq!(c.get("b").map(String::as_str), Some("blue"));
    }

    #[test]
    fn unsupported_selectors_and_properties_are_listed_but_not_hover() {
        let sheet = Stylesheet::parse(&[StyleSource {
            css: "a:hover { color: red } p::first-line { color: red } p { box-shadow: 0 0 1px red; cursor: pointer; color: red } div { box-shadow: none; -webkit-font-smoothing: auto }",
            base: "",
        }]);
        assert_eq!(
            sheet.unsupported,
            vec![
                (Unsupported::Selector("p::first-line".into()), 1),
                (Unsupported::Property("box-shadow".into()), 2),
            ]
        );
    }

    #[test]
    fn the_built_in_stylesheet_is_fully_supported() {
        let sheet = Stylesheet::parse(&[StyleSource {
            css: crate::UA_CSS,
            base: "",
        }]);
        assert_eq!(sheet.unsupported, vec![]);
    }
}
