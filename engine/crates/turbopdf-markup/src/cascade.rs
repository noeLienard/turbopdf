//! The CSS cascade: stylesheets (`<style>`, local `<link>`, the `css` option) and inline `style` attributes
//! → the declarations that apply to every element, in cascade order (last wins).
//!
//! Selectors: type, `*`, `.class`, `#id`, `[attr]`, `[attr=value]`, descendant and `>` combinators,
//! `:first-child`, `:last-child`, `:nth-child(odd | even | N | An+B)`, `:root`, `::before` / `::after`,
//! selector lists. Rules using anything else (`+`, `~`, `:hover`…) never match. At-rules: `@media` (print /
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pseudo {
    FirstChild,
    LastChild,
    Nth { a: i64, b: i64 },
    Root,
}

#[derive(Debug, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
    pseudo: Vec<Pseudo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
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
            let selectors: Vec<Selector> = split_top(prelude, ',')
                .into_iter()
                .filter_map(selector)
                .collect();
            if !selectors.is_empty() {
                self.rules.push(Rule {
                    selectors,
                    declarations: declarations(body),
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
    path.push(Ctx { id, index, count });
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
