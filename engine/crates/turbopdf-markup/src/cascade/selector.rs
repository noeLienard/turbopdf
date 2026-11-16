//! Selectors: parsing, specificity, matching (with the ancestor Bloom filter).

use super::*;

// ------------------------------------------------------------------ selectors

pub(crate) fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// `:hover`, `:focus`… and their pseudo-elements: states of a screen, never true on paper.
pub(crate) fn is_interactive(src: &str) -> bool {
    #[rustfmt::skip]
    const STATES: &[&str] = &[
        ":hover", ":focus", ":active", ":visited", ":link", ":any-link", ":target", ":checked",
        ":disabled", ":enabled", ":placeholder", ":invalid", ":valid", ":required", ":optional",
        ":read-only", ":read-write", ":indeterminate", ":default", ":autofill", ":user-",
        "::selection", "::placeholder", "::-webkit-", "::-moz-", ":-webkit-", ":-moz-", ":-ms-",
        "::backdrop", "::file-selector-button", "::scrollbar",
    ];
    let s = src.to_ascii_lowercase();
    STATES.iter().any(|p| s.contains(p))
}

/// `An+B` (`odd`, `even`, `3`, `2n+1`, `-n+3`). Whitespace is ignored.
pub(crate) fn an_b(expr: &str) -> Option<(i64, i64)> {
    let e: String = expr
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match e.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Ok(b) = e.parse::<i64>() {
        return (b >= 0 && !e.starts_with(['+', '-'])).then_some((0, b));
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
    Some((a, b))
}

pub(crate) fn take_ident(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !is_ident(c)).unwrap_or(s.len());
    (&s[..end], &s[end..])
}

/// Byte index of the `)` / `]` closing the bracket that `s` starts just after (nesting and quotes respected).
fn closing(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 1;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == open => depth += 1,
            None if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            None => {}
        }
    }
    None
}

/// `name`, `name=value`, `name^="value" i`…
fn attr_test(inner: &str) -> Option<AttrTest> {
    let inner = inner.trim();
    let Some(eq) = inner.find('=') else {
        return (!inner.is_empty() && inner.chars().all(is_ident)).then(|| AttrTest {
            name: inner.to_ascii_lowercase(),
            value: None,
            ignore_case: false,
        });
    };
    let (op, name_end) = match inner[..eq].chars().last() {
        Some('~') => (AttrOp::Word, eq - 1),
        Some('|') => (AttrOp::Lang, eq - 1),
        Some('^') => (AttrOp::Prefix, eq - 1),
        Some('$') => (AttrOp::Suffix, eq - 1),
        Some('*') => (AttrOp::Contains, eq - 1),
        _ => (AttrOp::Equals, eq),
    };
    let name = inner[..name_end].trim();
    if name.is_empty() || !name.chars().all(is_ident) {
        return None;
    }
    let mut value = inner[eq + 1..].trim();
    let mut ignore_case = false;
    for flag in [" i", " I", " s", " S"] {
        if let Some(v) = value.strip_suffix(flag) {
            ignore_case = flag.trim().eq_ignore_ascii_case("i");
            value = v.trim_end();
        }
    }
    Some(AttrTest {
        name: name.to_ascii_lowercase(),
        value: Some((op, unquote(value))),
        ignore_case,
    })
}

/// The compound selectors of `:not(…)` / `:is(…)` / `:where(…)`; complex selectors are not supported.
fn compound_list(arg: &str) -> Option<Vec<Compound>> {
    split_top(arg, ',')
        .into_iter()
        .map(|c| {
            let c = c.trim();
            if c.is_empty() || c.contains(|ch: char| ch.is_whitespace() || "+~>".contains(ch)) {
                return None;
            }
            compound(c)
        })
        .collect()
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
                let close = closing(rest, '[', ']')?;
                c.attrs.push(attr_test(&rest[..close])?);
                s = &rest[close + 1..];
            }
            ':' => {
                if rest.starts_with(':') {
                    return None; // pseudo-elements other than ::before / ::after
                }
                let (name, r) = take_ident(rest);
                if name.is_empty() {
                    return None;
                }
                let (arg, r) = match r.strip_prefix('(') {
                    Some(inner) => {
                        let close = closing(inner, '(', ')')?;
                        (Some(&inner[..close]), &inner[close + 1..])
                    }
                    None => (None, r),
                };
                let nth = |a: &str, from_end, of_type| {
                    an_b(a).map(|(a, b)| {
                        Pseudo::Nth(Nth {
                            a,
                            b,
                            from_end,
                            of_type,
                        })
                    })
                };
                c.pseudo
                    .push(match (name.to_ascii_lowercase().as_str(), arg) {
                        ("first-child", None) => nth("1", false, false)?,
                        ("last-child", None) => nth("1", true, false)?,
                        ("first-of-type", None) => nth("1", false, true)?,
                        ("last-of-type", None) => nth("1", true, true)?,
                        ("nth-child", Some(a)) => nth(a, false, false)?,
                        ("nth-last-child", Some(a)) => nth(a, true, false)?,
                        ("nth-of-type", Some(a)) => nth(a, false, true)?,
                        ("nth-last-of-type", Some(a)) => nth(a, true, true)?,
                        ("only-child", None) => Pseudo::Only { of_type: false },
                        ("only-of-type", None) => Pseudo::Only { of_type: true },
                        ("empty", None) => Pseudo::Empty,
                        ("root", None) => Pseudo::Root,
                        ("not", Some(a)) => Pseudo::Not(compound_list(a)?),
                        ("is" | "matches" | "any", Some(a)) => Pseudo::Is {
                            list: compound_list(a)?,
                            counts: true,
                        },
                        ("where", Some(a)) => Pseudo::Is {
                            list: compound_list(a)?,
                            counts: false,
                        },
                        _ => return None,
                    });
                s = r;
            }
            _ => return None,
        }
    }
    Some(c)
}

/// (ids, classes / attributes / pseudo-classes, tags) of a compound.
fn specificity_of(c: &Compound) -> (usize, usize, usize) {
    let (mut a, mut b, mut t) = (
        usize::from(c.id.is_some()),
        c.classes.len() + c.attrs.len(),
        usize::from(c.tag.is_some()),
    );
    for p in &c.pseudo {
        match p {
            // the most specific argument counts
            Pseudo::Not(list) | Pseudo::Is { list, counts: true } => {
                let max = list.iter().map(specificity_of).max().unwrap_or((0, 0, 0));
                a += max.0;
                b += max.1;
                t += max.2;
            }
            Pseudo::Is { counts: false, .. } => {}
            _ => b += 1,
        }
    }
    (a, b, t)
}

pub(crate) fn selector(src: &str) -> Option<Selector> {
    let mut src = src.trim();
    let mut generated = None;
    let lower = src.to_ascii_lowercase();
    for (suffix, g) in [
        ("::before", Generated::Before),
        ("::after", Generated::After),
        (":before", Generated::Before),
        (":after", Generated::After),
    ] {
        if generated.is_none() && lower.ends_with(suffix) {
            generated = Some(g);
            src = src[..src.len() - suffix.len()].trim_end();
        }
    }
    // tokens: compounds separated by whitespace, `>`, `+` or `~`; `( … )` and `[ … ]` kept whole
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
    let mut quote: Option<char> = None;
    for ch in src.chars() {
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(ch);
            }
            c if depth > 0 => current.push(c),
            c if c.is_whitespace() => {
                flush(&mut current, &mut pending, &mut parts, &mut combinators)?
            }
            '>' | '+' | '~' => {
                flush(&mut current, &mut pending, &mut parts, &mut combinators)?;
                if parts.is_empty() || pending != Combinator::Descendant {
                    return None; // `> a`, `a > > b`
                }
                pending = match ch {
                    '>' => Combinator::Child,
                    '+' => Combinator::Adjacent,
                    _ => Combinator::Sibling,
                };
            }
            c => current.push(c),
        }
    }
    flush(&mut current, &mut pending, &mut parts, &mut combinators)?;
    if parts.is_empty() || pending != Combinator::Descendant || depth != 0 {
        return None;
    }
    let (mut a, mut b, mut c) = (0, 0, 0);
    for p in &parts {
        let (x, y, z) = specificity_of(p);
        a += x;
        b += y;
        c += z;
    }
    // keys every ancestor of the subject needs: compounds left of a descendant or child combinator
    // (one only reached through `+` / `~` is a sibling, not an ancestor)
    let mut ancestor_keys = Vec::new();
    for (j, p) in parts[..parts.len() - 1].iter().enumerate() {
        if !combinators[j..]
            .iter()
            .any(|c| matches!(c, Combinator::Descendant | Combinator::Child))
        {
            continue;
        }
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
    pub(crate) parent: Option<Id>,
}

/// The element siblings before `x`, nearest last.
fn previous(dom: &Dom, x: Ctx) -> Vec<Ctx> {
    let Some(parent) = x.parent else {
        return Vec::new();
    };
    dom.element_children(parent)
        .take(x.index.saturating_sub(1))
        .enumerate()
        .map(|(i, id)| Ctx {
            id,
            index: i + 1,
            count: x.count,
            parent: Some(parent),
        })
        .collect()
}

/// 1-based position of `x` among its siblings of the same tag, and their number.
fn type_position(dom: &Dom, x: Ctx, tag: &str) -> (usize, usize) {
    let Some(parent) = x.parent else {
        return (1, 1);
    };
    let (mut index, mut count) = (0, 0);
    for id in dom.element_children(parent) {
        if dom.tag(id) == Some(tag) {
            count += 1;
            if id == x.id {
                index = count;
            }
        }
    }
    (index, count)
}

fn nth_matches(a: i64, b: i64, i: i64) -> bool {
    if a == 0 {
        i == b
    } else {
        (i - b) / a >= 0 && (i - b) % a == 0
    }
}

fn attr_matches(t: &AttrTest, e: &Element) -> bool {
    let Some(v) = e.attr(&t.name) else {
        return false;
    };
    let Some((op, want)) = &t.value else {
        return true;
    };
    let (v, want) = if t.ignore_case {
        (v.to_lowercase(), want.to_lowercase())
    } else {
        (v.to_string(), want.clone())
    };
    match op {
        AttrOp::Equals => v == want,
        AttrOp::Word => !want.is_empty() && v.split_whitespace().any(|w| w == want),
        AttrOp::Lang => v == want || v.starts_with(&format!("{want}-")),
        AttrOp::Prefix => !want.is_empty() && v.starts_with(&want),
        AttrOp::Suffix => !want.is_empty() && v.ends_with(&want),
        AttrOp::Contains => !want.is_empty() && v.contains(&want),
    }
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
    if !c.attrs.iter().all(|t| attr_matches(t, e)) {
        return false;
    }
    c.pseudo.iter().all(|p| match p {
        Pseudo::Root => e.tag == "html",
        Pseudo::Nth(n) => {
            let (index, count) = if n.of_type {
                type_position(dom, x, &e.tag)
            } else {
                (x.index, x.count)
            };
            let i = if n.from_end { count + 1 - index } else { index };
            nth_matches(n.a, n.b, i as i64)
        }
        Pseudo::Only { of_type } => {
            if *of_type {
                type_position(dom, x, &e.tag).1 == 1
            } else {
                x.count == 1
            }
        }
        // no element child and no text (comments do not count)
        Pseudo::Empty => dom
            .children(x.id)
            .iter()
            .all(|&ch| match &dom.nodes[ch].data {
                Data::Element(_) => false,
                Data::Text(t) => t.is_empty(),
                _ => true,
            }),
        Pseudo::Not(list) => !list.iter().any(|c| matches_compound(c, dom, x)),
        Pseudo::Is { list, .. } => list.iter().any(|c| matches_compound(c, dom, x)),
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
        // a sibling: the same ancestors, then the sibling in place of the node
        let as_sibling = |sib: Ctx| {
            let mut p = ancestors.to_vec();
            p.push(sib);
            go(s, i - 1, dom, &p)
        };
        match s.combinators[i - 1] {
            Combinator::Child => go(s, i - 1, dom, ancestors),
            Combinator::Descendant => (0..ancestors.len())
                .rev()
                .any(|end| go(s, i - 1, dom, &ancestors[..=end])),
            Combinator::Adjacent => previous(dom, node).last().is_some_and(|&p| as_sibling(p)),
            Combinator::Sibling => previous(dom, node).into_iter().rev().any(as_sibling),
        }
    }
    go(s, s.parts.len() - 1, dom, path)
}
