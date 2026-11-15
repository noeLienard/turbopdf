//! Selectors: parsing, specificity, matching (with the ancestor Bloom filter).

use super::*;

// ------------------------------------------------------------------ selectors

pub(crate) fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

pub(crate) fn nth(expr: &str) -> Option<Pseudo> {
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

pub(crate) fn take_ident(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !is_ident(c)).unwrap_or(s.len());
    (&s[..end], &s[end..])
}

pub(crate) fn compound(src: &str) -> Option<Compound> {
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

pub(crate) fn selector(src: &str) -> Option<Selector> {
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
pub(crate) fn key_hash(prefix: &str, name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in prefix.bytes().chain(name.bytes()) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Counting Bloom filter of the keys of the current element's ancestors (as in browsers): a selector
/// whose ancestor compounds need a key that no ancestor has is rejected without walking up the tree.
pub(crate) struct AncestorFilter {
    counts: Vec<u16>,
}

impl AncestorFilter {
    const SIZE: usize = 1 << 12;

    pub(crate) fn new() -> Self {
        Self {
            counts: vec![0; Self::SIZE],
        }
    }

    pub(crate) fn slot(h: u32) -> usize {
        (h as usize ^ (h >> 12) as usize) & (Self::SIZE - 1)
    }

    pub(crate) fn keys(e: &Element) -> impl Iterator<Item = u32> + '_ {
        std::iter::once(key_hash("", &e.tag))
            .chain(e.attr("id").map(|i| key_hash("#", i)))
            .chain(classes(e).map(|c| key_hash(".", c)))
    }

    pub(crate) fn push(&mut self, e: &Element) {
        for h in Self::keys(e) {
            let c = &mut self.counts[Self::slot(h)];
            *c = c.saturating_add(1);
        }
    }

    pub(crate) fn pop(&mut self, e: &Element) {
        for h in Self::keys(e) {
            let c = &mut self.counts[Self::slot(h)];
            *c = c.saturating_sub(1);
        }
    }

    pub(crate) fn may_have(&self, h: u32) -> bool {
        self.counts[Self::slot(h)] > 0
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Ctx {
    pub(crate) id: Id,
    /// 1-based among element siblings.
    pub(crate) index: usize,
    pub(crate) count: usize,
}

pub(crate) fn classes(e: &Element) -> impl Iterator<Item = &str> {
    e.attr("class")
        .or_else(|| e.attr("classname"))
        .unwrap_or("")
        .split_whitespace()
}

pub(crate) fn matches_compound(c: &Compound, dom: &Dom, x: Ctx) -> bool {
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

pub(crate) fn matches(s: &Selector, dom: &Dom, path: &[Ctx]) -> bool {
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
