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

// ------------------------------------------------------------------ selectors

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

fn nth(expr: &str) -> Option<Pseudo> {
    let e: String = expr
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match e.as_str() {
        "odd" => return Some(Pseudo::Nth { a: 2, b: 1 }),
        "even" => return Some(Pseudo::Nth { a: 2, b: 0 }),
        _ => {}
    }
    if let Ok(b) = e.parse::<i64>() {
        return (b >= 0 && !e.starts_with(['+', '-'])).then_some(Pseudo::Nth { a: 0, b });
    }
    let (a, b) = e.split_once('n')?;
    let a = match a {
        "" | "+" => 1,
        "-" => -1,
        _ => a.parse().ok()?,
    };
    let b = if b.is_empty() {
        0
    } else {
        if !b.starts_with(['+', '-']) {
            return None;
        }
        b.parse().ok()?
    };
    Some(Pseudo::Nth { a, b })
}

fn take_ident(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !is_ident(c)).unwrap_or(s.len());
    (&s[..end], &s[end..])
}

fn compound(src: &str) -> Option<Compound> {
    let mut c = Compound::default();
    let mut s = src;
    if let Some(rest) = s.strip_prefix('*') {
        s = rest;
    } else if s.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        let (tag, rest) = take_ident(s);
        c.tag = Some(tag.to_ascii_lowercase());
        s = rest;
    }
    while !s.is_empty() {
        let first = s.chars().next()?;
        let rest = &s[1..];
        match first {
            '.' | '#' => {
                let (name, r) = take_ident(rest);
                if name.is_empty() {
                    return None;
                }
                if first == '.' {
                    c.classes.push(name.to_string());
                } else {
                    c.id = Some(name.to_string());
                }
                s = r;
            }
            '[' => {
                let close = rest.find(']')?;
                let inner = rest[..close].trim();
                let (name, value) = match inner.split_once('=') {
                    Some((n, v)) => (n.trim(), Some(unquote(v))),
                    None => (inner, None),
                };
                if name.is_empty() || !name.chars().all(is_ident) {
                    return None;
                }
                c.attrs.push((name.to_ascii_lowercase(), value));
                s = &rest[close + 1..];
            }
            ':' => {
                let (name, r) = take_ident(rest);
                if name.is_empty() {
                    return None;
                }
                let (arg, r) = match r.strip_prefix('(') {
                    Some(inner) => {
                        let close = inner.find(')')?;
                        (Some(&inner[..close]), &inner[close + 1..])
                    }
                    None => (None, r),
                };
                c.pseudo
                    .push(match (name.to_ascii_lowercase().as_str(), arg) {
                        ("first-child", None) => Pseudo::FirstChild,
                        ("last-child", None) => Pseudo::LastChild,
                        ("nth-child", Some(a)) => nth(a)?,
                        ("root", None) => Pseudo::Root,
                        // :hover, :focus… never match on paper
                        _ => return None,
                    });
                s = r;
            }
            _ => return None,
        }
    }
    Some(c)
}

fn selector(src: &str) -> Option<Selector> {
    let mut src = src.trim();
    let mut generated = None;
    let lower = src.to_ascii_lowercase();
    for (suffix, g) in [
        ("::before", Generated::Before),
        ("::after", Generated::After),
    ] {
        if lower.ends_with(suffix) {
            generated = Some(g);
            src = src[..src.len() - suffix.len()].trim_end();
        }
    }
    // tokens: compounds separated by whitespace or `>`; `( … )` kept whole (`:nth-child( 2n + 1 )`)
    let mut parts = Vec::new();
    let mut combinators = Vec::new();
    let mut pending = Combinator::Descendant;
    let mut current = String::new();
    let mut depth = 0;
    let flush = |current: &mut String,
                 pending: &mut Combinator,
                 parts: &mut Vec<Compound>,
                 combinators: &mut Vec<Combinator>|
     -> Option<()> {
        if current.is_empty() {
            return Some(());
        }
        let c = compound(current)?;
        if !parts.is_empty() {
            combinators.push(*pending);
        }
        *pending = Combinator::Descendant;
        parts.push(c);
        current.clear();
        Some(())
    };
    for ch in src.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            c if depth > 0 => {
                if !c.is_whitespace() {
                    current.push(c);
                }
            }
            c if c.is_whitespace() => {
                flush(&mut current, &mut pending, &mut parts, &mut combinators)?
            }
            '>' => {
                flush(&mut current, &mut pending, &mut parts, &mut combinators)?;
                pending = Combinator::Child;
            }
            // sibling combinators are not supported
            '+' | '~' => return None,
            c => current.push(c),
        }
    }
    flush(&mut current, &mut pending, &mut parts, &mut combinators)?;
    if parts.is_empty() {
        return None;
    }
    let (mut a, mut b, mut c) = (0, 0, 0);
    for p in &parts {
        if p.id.is_some() {
            a += 1;
        }
        b += p.classes.len() + p.attrs.len() + p.pseudo.len();
        if p.tag.is_some() {
            c += 1;
        }
    }
    let mut ancestor_keys = Vec::new();
    for p in &parts[..parts.len() - 1] {
        if let Some(t) = &p.tag {
            ancestor_keys.push(key_hash("", t));
        }
        if let Some(i) = &p.id {
            ancestor_keys.push(key_hash("#", i));
        }
        ancestor_keys.extend(p.classes.iter().map(|c| key_hash(".", c)));
    }
    Some(Selector {
        parts,
        ancestor_keys,
        combinators,
        specificity: (a * 10000 + b * 100 + c) as u32,
        generated,
    })
}

// ------------------------------------------------------------------ matching

/// FNV-1a of a selector key (`""` + tag, `"."` + class, `"#"` + id).
fn key_hash(prefix: &str, name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in prefix.bytes().chain(name.bytes()) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Counting Bloom filter of the keys of the current element's ancestors (as in browsers): a selector
/// whose ancestor compounds need a key that no ancestor has is rejected without walking up the tree.
struct AncestorFilter {
    counts: Vec<u16>,
}

impl AncestorFilter {
    const SIZE: usize = 1 << 12;

    fn new() -> Self {
        Self {
            counts: vec![0; Self::SIZE],
        }
    }

    fn slot(h: u32) -> usize {
        (h as usize ^ (h >> 12) as usize) & (Self::SIZE - 1)
    }

    fn keys(e: &Element) -> impl Iterator<Item = u32> + '_ {
        std::iter::once(key_hash("", &e.tag))
            .chain(e.attr("id").map(|i| key_hash("#", i)))
            .chain(classes(e).map(|c| key_hash(".", c)))
    }

    fn push(&mut self, e: &Element) {
        for h in Self::keys(e) {
            let c = &mut self.counts[Self::slot(h)];
            *c = c.saturating_add(1);
        }
    }

    fn pop(&mut self, e: &Element) {
        for h in Self::keys(e) {
            let c = &mut self.counts[Self::slot(h)];
            *c = c.saturating_sub(1);
        }
    }

    fn may_have(&self, h: u32) -> bool {
        self.counts[Self::slot(h)] > 0
    }
}

#[derive(Clone, Copy)]
struct Ctx {
    id: Id,
    /// 1-based among element siblings.
    index: usize,
    count: usize,
}

fn classes(e: &Element) -> impl Iterator<Item = &str> {
    e.attr("class")
        .or_else(|| e.attr("classname"))
        .unwrap_or("")
        .split_whitespace()
}

fn matches_compound(c: &Compound, dom: &Dom, x: Ctx) -> bool {
    let Some(e) = dom.element(x.id) else {
        return false;
    };
    if c.tag.as_deref().is_some_and(|t| t != e.tag) {
        return false;
    }
    if c.id.as_deref().is_some_and(|id| e.attr("id") != Some(id)) {
        return false;
    }
    if !c.classes.iter().all(|k| classes(e).any(|x| x == k)) {
        return false;
    }
    for (name, value) in &c.attrs {
        match (e.attr(name), value) {
            (None, _) => return false,
            (Some(v), Some(want)) if v != want => return false,
            _ => {}
        }
    }
    c.pseudo.iter().all(|p| match *p {
        Pseudo::FirstChild => x.index == 1,
        Pseudo::LastChild => x.index == x.count,
        Pseudo::Root => e.tag == "html",
        Pseudo::Nth { a, b } => {
            let i = x.index as i64;
            if a == 0 {
                i == b
            } else {
                (i - b) / a >= 0 && (i - b) % a == 0
            }
        }
    })
}

fn matches(s: &Selector, dom: &Dom, path: &[Ctx]) -> bool {
    fn go(s: &Selector, i: usize, dom: &Dom, path: &[Ctx]) -> bool {
        let Some((&node, ancestors)) = path.split_last() else {
            return false;
        };
        if !matches_compound(&s.parts[i], dom, node) {
            return false;
        }
        if i == 0 {
            return true;
        }
        match s.combinators[i - 1] {
            Combinator::Child => go(s, i - 1, dom, ancestors),
            Combinator::Descendant => (0..ancestors.len())
                .rev()
                .any(|end| go(s, i - 1, dom, &ancestors[..=end])),
        }
    }
    go(s, s.parts.len() - 1, dom, path)
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
