//! CSS declarations → tree [`Style`]. A practical subset: flexbox, grid, sizes, spacing, borders, colours and
//! fonts. Unknown properties are ignored. Lengths are converted to points (1 CSS px = 0.75 pt).

use turbopdf_tree::{
    AlignItems, Border, BorderStyle, Color, Dimension, EdgeSides, Edges, FlexDirection, FlexWrap,
    FontStyle, JustifyContent, LayoutMode, Position, Style, TextAlign, TextStyle,
};

/// One declaration: property (lowercase) and value (trimmed, without `!important`).
pub type Decl = (String, String);

/// Default font size in points: the engine's root style.
pub const ROOT_FONT_SIZE: f32 = 11.0;
const A4: (f32, f32) = (595.28, 841.89);

/// `a: b; c: d` → declarations, with `!important` flags. Semicolons inside `( )` or quotes are kept
/// (`url("data:image/png;base64,…")`).
pub fn declarations(body: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for part in split_top(body, ';') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() {
            continue;
        }
        let mut v = v.trim();
        let mut important = false;
        if let Some(i) = v.rfind('!')
            && v[i + 1..].trim().eq_ignore_ascii_case("important")
        {
            important = true;
            v = v[..i].trim_end();
        }
        out.push((k.to_ascii_lowercase(), v.to_string(), important));
    }
    out
}

/// Split at `sep` outside parentheses and quotes.
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, _) if c == sep && depth <= 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Whitespace-separated tokens, keeping `( … )` groups whole.
fn tokens(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, None);
    for (i, c) in value.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(&value[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&value[s..]);
    }
    out
}

// ------------------------------------------------------------------ colours

const NAMED: &[(&str, &str)] = &[
    ("black", "#000000"),
    ("white", "#ffffff"),
    ("red", "#ff0000"),
    ("green", "#008000"),
    ("blue", "#0000ff"),
    ("gray", "#808080"),
    ("grey", "#808080"),
    ("silver", "#c0c0c0"),
    ("orange", "#ffa500"),
    ("yellow", "#ffff00"),
    ("purple", "#800080"),
    ("navy", "#000080"),
    ("teal", "#008080"),
];

pub fn css_color(value: &str) -> Option<Color> {
    let v = value.trim().to_ascii_lowercase();
    if v == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    if let Some(hex) = v.strip_prefix('#') {
        return (matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| Color::parse(&v).ok())
            .flatten();
    }
    if let Some((_, hex)) = NAMED.iter().find(|(n, _)| *n == v) {
        return Color::parse(hex).ok();
    }
    let inner = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let mut rgb = [0u8; 3];
    for (i, p) in parts[..3].iter().enumerate() {
        let n = number_prefix(p)?;
        rgb[i] = n.clamp(0.0, 255.0).round() as u8;
    }
    let alpha = match parts.get(3) {
        None => 1.0,
        Some(a) if a.ends_with('%') => number_prefix(a)? / 100.0,
        Some(a) => number_prefix(a)?,
    };
    let a = if alpha < 1.0 {
        (alpha * 255.0).clamp(0.0, 255.0).round() as u8
    } else {
        255
    };
    Some(Color {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a,
    })
}

/// Leading number of a string, like JavaScript's `parseFloat`.
fn number_prefix(s: &str) -> Option<f32> {
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && i == 0)))
        .map_or(s.len(), |(i, _)| i);
    s[..end].parse().ok()
}

// ------------------------------------------------------------------ lengths

fn unit(u: &str) -> Option<f32> {
    Some(match u {
        "px" => 0.75,
        "pt" => 1.0,
        "mm" => 72.0 / 25.4,
        "cm" => 72.0 / 2.54,
        "in" => 72.0,
        "pc" => 12.0,
        _ => return None,
    })
}

struct LengthParser<'a> {
    s: &'a [u8],
    at: usize,
    font_size: f32,
}

impl LengthParser<'_> {
    fn space(&mut self) {
        while self.s.get(self.at).is_some_and(|c| c.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn take(&mut self, c: u8) -> bool {
        self.space();
        if self.s.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn parse(mut self) -> Option<f32> {
        let v = self.sum()?;
        self.space();
        (self.at == self.s.len() && v.is_finite()).then_some(v)
    }

    fn sum(&mut self) -> Option<f32> {
        let mut left = self.product()?;
        loop {
            self.space();
            let op = *self.s.get(self.at).unwrap_or(&0);
            if op != b'+' && op != b'-' {
                return Some(left);
            }
            self.at += 1;
            let right = self.product()?;
            left = if op == b'+' {
                left + right
            } else {
                left - right
            };
        }
    }

    fn product(&mut self) -> Option<f32> {
        let mut left = self.atom()?;
        loop {
            self.space();
            let op = *self.s.get(self.at).unwrap_or(&0);
            if op != b'*' && op != b'/' {
                return Some(left);
            }
            self.at += 1;
            let right = self.atom()?;
            if op == b'/' && right == 0.0 {
                return None;
            }
            left = if op == b'*' {
                left * right
            } else {
                left / right
            };
        }
    }

    fn atom(&mut self) -> Option<f32> {
        self.space();
        if self.take(b'(') {
            let v = self.sum()?;
            return self.take(b')').then_some(v);
        }
        let rest = &self.s[self.at..];
        for name in ["calc", "min", "max", "clamp"] {
            let n = name.len();
            if rest.len() > n && rest[..n].eq_ignore_ascii_case(name.as_bytes()) && rest[n] == b'('
            {
                self.at += n + 1;
                let mut args = Vec::new();
                loop {
                    args.push(self.sum()?);
                    if self.take(b')') {
                        break;
                    }
                    if !self.take(b',') {
                        return None;
                    }
                }
                return match (name, args.as_slice()) {
                    ("calc", [v]) => Some(*v),
                    ("min", a) if !a.is_empty() => a.iter().copied().reduce(f32::min),
                    ("max", a) if !a.is_empty() => a.iter().copied().reduce(f32::max),
                    ("clamp", [lo, v, hi]) => Some(hi.min(lo.max(*v))),
                    _ => None,
                };
            }
        }
        // -?\d*\.?\d+
        let start = self.at;
        let mut i = self.at;
        if self.s.get(i) == Some(&b'-') {
            i += 1;
        }
        let digits = i;
        while self.s.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if self.s.get(i) == Some(&b'.') && self.s.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 1;
            while self.s.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        if i == digits {
            return None;
        }
        let n: f32 = std::str::from_utf8(&self.s[start..i]).ok()?.parse().ok()?;
        let mut j = i;
        while self.s.get(j).is_some_and(u8::is_ascii_alphabetic) {
            j += 1;
        }
        let u = std::str::from_utf8(&self.s[i..j])
            .ok()?
            .to_ascii_lowercase();
        let v = match u.as_str() {
            "" => n,
            "em" => n * self.font_size,
            "rem" => n * ROOT_FONT_SIZE,
            "vw" => n * A4.0 / 100.0,
            "vh" => n * A4.1 / 100.0,
            u => match unit(u) {
                Some(k) => n * k,
                // `2fr`, `10deg`…: not a length, and the letters are not consumed
                None => return None,
            },
        };
        self.at = j;
        Some(v)
    }
}

fn is_plain_number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    !s.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.bytes().all(|b| b.is_ascii_digit())
        && !(s.contains('.') && frac.is_empty())
        && s != "."
}

/// CSS length → points: calc/min/max/clamp, em/rem/vw/vh. A bare number is a CSS pixel.
pub fn css_length(value: &str, font_size: f32) -> Option<f32> {
    let v = value.trim();
    if is_plain_number(v) {
        return v.parse::<f32>().ok().map(|n| n * 0.75);
    }
    LengthParser {
        s: v.as_bytes(),
        at: 0,
        font_size,
    }
    .parse()
}

fn percent(v: &str, allow_negative: bool) -> Option<f32> {
    let n = v.strip_suffix('%')?;
    let digits = if allow_negative {
        n.strip_prefix('-').unwrap_or(n)
    } else {
        n
    };
    is_plain_number(digits).then(|| n.parse().ok()).flatten()
}

fn dimension(value: &str, font_size: f32) -> Option<Dimension> {
    let v = value.trim();
    if v == "auto" {
        return Some(Dimension::Auto);
    }
    if let Some(p) = percent(v, false) {
        return Some(Dimension::Percent(p));
    }
    css_length(v, font_size).map(|n| Dimension::Points(n.max(0.0)))
}

fn grid_tracks(value: &str, font_size: f32) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for t in tokens(value) {
        let lower = t.to_ascii_lowercase();
        if let Some(inner) = lower
            .strip_prefix("repeat(")
            .and_then(|r| r.strip_suffix(')'))
        {
            let (count, tracks) = inner.split_once(',')?;
            let count: usize = count.trim().parse().ok()?;
            let tracks = grid_tracks(tracks, font_size)?;
            if !(1..=1000).contains(&count) {
                return None;
            }
            for _ in 0..count {
                out.extend(tracks.iter().cloned());
            }
            continue;
        }
        let fr = lower
            .strip_suffix("fr")
            .is_some_and(|n| is_plain_number(n) && !n.starts_with('-'));
        if matches!(lower.as_str(), "auto" | "min-content" | "max-content")
            || fr
            || percent(&lower, false).is_some()
        {
            out.push(lower);
            continue;
        }
        let n = css_length(t, font_size)?;
        if n < 0.0 {
            return None;
        }
        out.push(format!("{n}pt"));
    }
    (!out.is_empty()).then_some(out)
}

fn edges(value: &str, font_size: f32) -> Option<Edges> {
    let parts: Option<Vec<f32>> = tokens(value)
        .into_iter()
        .map(|p| {
            if p == "auto" {
                Some(0.0)
            } else {
                css_length(p, font_size)
            }
        })
        .collect();
    let p = parts?;
    let (t, r, b, l) = match p.as_slice() {
        [t] => (*t, *t, *t, *t),
        [t, r] => (*t, *r, *t, *r),
        [t, r, b] => (*t, *r, *b, *r),
        [t, r, b, l] => (*t, *r, *b, *l),
        _ => return None,
    };
    Some(if t == r && r == b && b == l {
        Edges::All(t)
    } else {
        Edges::Sides(EdgeSides {
            top: Some(t),
            right: Some(r),
            bottom: Some(b),
            left: Some(l),
        })
    })
}

fn clamp_edges(e: Edges) -> Edges {
    match e {
        Edges::All(v) => Edges::All(v.max(0.0)),
        Edges::Sides(s) => Edges::Sides(EdgeSides {
            top: Some(s.top.unwrap_or(0.0).max(0.0)),
            right: Some(s.right.unwrap_or(0.0).max(0.0)),
            bottom: Some(s.bottom.unwrap_or(0.0).max(0.0)),
            left: Some(s.left.unwrap_or(0.0).max(0.0)),
        }),
    }
}

fn set_side(e: Option<Edges>, side: &str, v: f32) -> Edges {
    let mut s = match e {
        Some(Edges::All(a)) => EdgeSides {
            top: Some(a),
            right: Some(a),
            bottom: Some(a),
            left: Some(a),
        },
        Some(Edges::Sides(s)) => s,
        None => EdgeSides::default(),
    };
    match side {
        "top" => s.top = Some(v),
        "right" => s.right = Some(v),
        "bottom" => s.bottom = Some(v),
        _ => s.left = Some(v),
    }
    Edges::Sides(s)
}

fn border(value: &str, font_size: f32) -> Option<Border> {
    let v = value.trim();
    if v == "none" || v == "0" {
        return None;
    }
    let mut b = Border {
        width: 0.75,
        color: Color::BLACK,
        style: Some(BorderStyle::Solid),
    };
    for t in tokens(v) {
        match t {
            "solid" => b.style = Some(BorderStyle::Solid),
            "dashed" => b.style = Some(BorderStyle::Dashed),
            "dotted" => b.style = Some(BorderStyle::Dotted),
            _ => {
                if let Some(w) = css_length(t, font_size) {
                    b.width = w;
                } else if let Some(c) = css_color(t) {
                    b.color = c;
                }
            }
        }
    }
    Some(b)
}

fn align(v: &str) -> Option<AlignItems> {
    Some(match v {
        "flex-start" | "start" => AlignItems::FlexStart,
        "flex-end" | "end" => AlignItems::FlexEnd,
        "center" => AlignItems::Center,
        "stretch" => AlignItems::Stretch,
        "baseline" => AlignItems::Baseline,
        _ => return None,
    })
}

fn justify(v: &str) -> Option<JustifyContent> {
    Some(match v {
        "flex-start" | "start" | "left" => JustifyContent::FlexStart,
        "flex-end" | "end" | "right" => JustifyContent::FlexEnd,
        "center" => JustifyContent::Center,
        "space-between" => JustifyContent::SpaceBetween,
        "space-around" => JustifyContent::SpaceAround,
        "space-evenly" => JustifyContent::SpaceEvenly,
        _ => return None,
    })
}

fn font_weight(v: &str) -> Option<u16> {
    match v {
        "bold" | "bolder" => Some(700),
        "normal" | "lighter" => Some(400),
        _ => {
            let n = number_prefix(v)? as i64;
            (100..=900).contains(&n).then_some(n as u16)
        }
    }
}

fn offset(v: &str, font_size: f32) -> Option<Dimension> {
    let t = v.trim();
    if t == "auto" {
        return Some(Dimension::Auto);
    }
    if let Some(p) = percent(t, true) {
        return Some(Dimension::Percent(p));
    }
    css_length(t, font_size).map(Dimension::Points)
}

/// Declarations (cascaded, last wins) → style. `inherited_font_size` resolves `em`.
pub fn parse_style(decls: &[Decl], inherited_font_size: f32) -> Style {
    let mut s = Style::default();
    let font_size = decls
        .iter()
        .rev()
        .find(|(k, _)| k == "font-size")
        .and_then(|(_, v)| css_length(v, inherited_font_size))
        .filter(|n| *n != 0.0)
        .unwrap_or(inherited_font_size);
    let fs = font_size;
    let mut flex_display = false;

    for (k, v) in decls {
        let v = v.as_str();
        match k.as_str() {
            "display" => {
                flex_display = matches!(v, "flex" | "inline-flex");
                s.display = match v {
                    "grid" | "inline-grid" => Some(LayoutMode::Grid),
                    "flex" | "inline-flex" => Some(LayoutMode::Flex),
                    _ => None,
                };
            }
            "grid-template-columns" => s.grid_template_columns = grid_tracks(v, fs),
            "grid-template-rows" => s.grid_template_rows = grid_tracks(v, fs),
            "flex-direction" => {
                if let Some(d) = match v {
                    "row" => Some(FlexDirection::Row),
                    "row-reverse" => Some(FlexDirection::RowReverse),
                    "column" => Some(FlexDirection::Column),
                    "column-reverse" => Some(FlexDirection::ColumnReverse),
                    _ => None,
                } {
                    s.flex_direction = Some(d);
                }
            }
            "flex-wrap" => {
                s.flex_wrap = Some(if v == "wrap" {
                    FlexWrap::Wrap
                } else {
                    FlexWrap::NoWrap
                })
            }
            "align-items" => s.align_items = align(v).or(s.align_items),
            "align-self" => s.align_self = align(v).or(s.align_self),
            "align-content" => s.align_content = justify(v).or(s.align_content),
            "justify-content" => s.justify_content = justify(v).or(s.justify_content),
            "gap" => {
                let t = tokens(v);
                let first = t.first().and_then(|x| css_length(x, fs));
                let second = t.get(1).and_then(|x| css_length(x, fs));
                if let Some(a) = first {
                    match second {
                        None => s.gap = Some(a.max(0.0)),
                        Some(b) => {
                            s.row_gap = Some(a.max(0.0));
                            s.column_gap = Some(b.max(0.0));
                        }
                    }
                }
            }
            "row-gap" | "column-gap" => {
                if let Some(n) = tokens(v).first().and_then(|x| css_length(x, fs)) {
                    if k == "row-gap" {
                        s.row_gap = Some(n.max(0.0));
                    } else {
                        s.column_gap = Some(n.max(0.0));
                    }
                }
            }
            "flex" => match v {
                "none" => s.flex_grow = Some(0.0),
                "auto" => s.flex_grow = Some(1.0),
                _ => {
                    let mut it = v.split_whitespace();
                    if let Some(g) = it.next().and_then(|g| g.parse::<f32>().ok()) {
                        s.flex_grow = Some(g);
                    }
                    if let Some(sh) = it.next().and_then(|x| x.parse::<f32>().ok()) {
                        s.flex_shrink = Some(sh);
                    }
                    if let Some(b) = v.split_whitespace().nth(2) {
                        s.flex_basis = dimension(b, fs);
                    }
                }
            },
            "flex-grow" => s.flex_grow = Some(v.parse::<f32>().unwrap_or(0.0).max(0.0)),
            "flex-shrink" => s.flex_shrink = Some(v.parse::<f32>().unwrap_or(0.0).max(0.0)),
            "flex-basis" => s.flex_basis = dimension(v, fs),
            "position" => match v.trim() {
                "absolute" | "fixed" => s.position = Some(Position::Absolute),
                "relative" => s.position = Some(Position::Relative),
                _ => {}
            },
            "top" | "right" | "bottom" | "left" => {
                if let Some(d) = offset(v, fs) {
                    match k.as_str() {
                        "top" => s.top = Some(d),
                        "right" => s.right = Some(d),
                        "bottom" => s.bottom = Some(d),
                        _ => s.left = Some(d),
                    }
                }
            }
            "inset" => {
                if let Some(e) = edges(v, fs) {
                    let [t, r, b, l] = e.resolve();
                    s.top = Some(Dimension::Points(t));
                    s.right = Some(Dimension::Points(r));
                    s.bottom = Some(Dimension::Points(b));
                    s.left = Some(Dimension::Points(l));
                }
            }
            "width" => s.width = dimension(v, fs),
            "height" => s.height = dimension(v, fs),
            "min-width" => s.min_width = dimension(v, fs),
            "max-width" => s.max_width = dimension(v, fs),
            "min-height" => s.min_height = dimension(v, fs),
            "max-height" => s.max_height = dimension(v, fs),
            "aspect-ratio" => {
                let (a, b) = v.split_once('/').unwrap_or((v, "1"));
                if let (Ok(a), Ok(b)) = (a.trim().parse::<f32>(), b.trim().parse::<f32>()) {
                    let r = a / b;
                    if r > 0.0 && r.is_finite() {
                        s.aspect_ratio = Some(r);
                    }
                }
            }
            "padding" => s.padding = edges(v, fs).map(clamp_edges),
            "margin" => s.margin = edges(v, fs),
            "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                let n = css_length(v, fs).unwrap_or(0.0).max(0.0);
                s.padding = Some(set_side(s.padding, &k[8..], n));
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                let n = if v == "auto" {
                    0.0
                } else {
                    css_length(v, fs).unwrap_or(0.0)
                };
                s.margin = Some(set_side(s.margin, &k[7..], n));
            }
            "border" => s.border = border(v, fs),
            "border-color" => {
                if let (Some(b), Some(c)) = (&mut s.border, css_color(v)) {
                    b.color = c;
                }
            }
            "border-width" => {
                if let (Some(b), Some(w)) = (&mut s.border, css_length(v, fs)) {
                    b.width = w;
                }
            }
            "border-radius" => {
                if let Some(n) = tokens(v).first().and_then(|x| css_length(x, fs)) {
                    s.border_radius = Some(n.max(0.0));
                }
            }
            "background" | "background-color" => {
                let first = v.split_whitespace().next().unwrap_or("");
                if let Some(c) = css_color(first).or_else(|| css_color(v)) {
                    s.background_color = Some(c);
                }
            }
            "color" => s.color = css_color(v).or(s.color),
            "font-family" => {
                let first = v.split(',').next().unwrap_or("").trim();
                let first = first
                    .strip_prefix(['"', '\''])
                    .unwrap_or(first)
                    .strip_suffix(['"', '\''])
                    .unwrap_or(first.strip_prefix(['"', '\'']).unwrap_or(first));
                if !first.is_empty() {
                    s.font_family = Some(first.to_string());
                }
            }
            "font-size" => s.font_size = Some(font_size),
            "font-weight" => s.font_weight = font_weight(v).or(s.font_weight),
            "font-style" => {
                s.font_style = Some(if v == "italic" || v == "oblique" {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                })
            }
            "text-align" => {
                s.text_align = match v {
                    "left" | "start" => Some(TextAlign::Left),
                    "center" => Some(TextAlign::Center),
                    "right" | "end" => Some(TextAlign::Right),
                    "justify" => Some(TextAlign::Justify),
                    _ => s.text_align,
                }
            }
            "line-height" => {
                if is_plain_number(v) {
                    s.line_height = v.parse().ok();
                } else if let Some(p) = v.strip_suffix('%') {
                    s.line_height = number_prefix(p).map(|n| n / 100.0);
                } else if let Some(n) = css_length(v, fs).filter(|n| *n != 0.0) {
                    s.line_height = Some(n / font_size);
                }
            }
            _ => {}
        }
    }
    // `display: flex` lays out in a row unless told otherwise (a tree view is a column by default)
    if flex_display && s.flex_direction.is_none() {
        s.flex_direction = Some(FlexDirection::Row);
    }
    s
}

/// The part of a style that applies to a span of text.
pub fn text_style_of(s: &Style) -> TextStyle {
    TextStyle {
        font_family: s.font_family.clone(),
        font_size: s.font_size.filter(|n| *n != 0.0),
        font_weight: s.font_weight,
        font_style: s.font_style,
        color: s.color,
    }
}

pub fn text_style_is_empty(t: &TextStyle) -> bool {
    *t == TextStyle::default()
}

/// `over` on top of `base`: every field set in `over` wins.
pub fn merge_text(base: &TextStyle, over: &TextStyle) -> TextStyle {
    TextStyle {
        font_family: over
            .font_family
            .clone()
            .or_else(|| base.font_family.clone()),
        font_size: over.font_size.or(base.font_size),
        font_weight: over.font_weight.or(base.font_weight),
        font_style: over.font_style.or(base.font_style),
        color: over.color.or(base.color),
    }
}

macro_rules! merge_fields {
    ($base:expr, $over:expr, $($f:ident),+ $(,)?) => {
        Style { $($f: $over.$f.clone().or_else(|| $base.$f.clone())),+ }
    };
}

/// `over` on top of `base`: every field set in `over` wins.
pub fn merge(base: &Style, over: &Style) -> Style {
    merge_fields!(
        base,
        over,
        display,
        flex_direction,
        flex_wrap,
        align_items,
        align_content,
        justify_content,
        gap,
        row_gap,
        column_gap,
        grid_template_columns,
        grid_template_rows,
        flex_grow,
        flex_shrink,
        flex_basis,
        align_self,
        position,
        top,
        right,
        bottom,
        left,
        width,
        height,
        min_width,
        max_width,
        min_height,
        max_height,
        aspect_ratio,
        padding,
        margin,
        border,
        border_radius,
        background_color,
        font_family,
        font_size,
        font_weight,
        font_style,
        color,
        text_align,
        line_height,
    )
}

pub fn is_empty(s: &Style) -> bool {
    *s == Style::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(css: &str) -> Style {
        let d: Vec<Decl> = declarations(css)
            .into_iter()
            .map(|(k, v, _)| (k, v))
            .collect();
        parse_style(&d, ROOT_FONT_SIZE)
    }

    fn json(css: &str) -> serde_json::Value {
        serde_json::to_value(style(css)).unwrap()
    }

    #[test]
    fn inline_css_becomes_a_style() {
        assert_eq!(
            json(
                "padding: 8px 16px; margin-top: 1cm; border: 1px solid #ccc; width: 50%; line-height: 18px; font-size: 12px"
            ),
            serde_json::json!({
                "padding": { "top": 6.0, "right": 12.0, "bottom": 6.0, "left": 12.0 },
                "margin": { "top": (72.0f32 / 2.54) as f64 },
                "border": { "width": 0.75, "color": "#cccccc", "style": "solid" },
                "width": "50%",
                "lineHeight": 1.5,
                "fontSize": 9.0,
            })
        );
        assert_eq!(
            json("background: rgba(255, 0, 0, 0.5); font-weight: bold; flex: 1"),
            serde_json::json!({ "backgroundColor": "#ff000080", "fontWeight": 700, "flexGrow": 1.0 })
        );
    }

    #[test]
    fn flex_grid_and_positions() {
        let s = style(
            "display:flex;align-content:space-between;align-self:center;gap:4pt 8pt;position:relative;left:2pt;aspect-ratio:16/9",
        );
        assert_eq!(s.display, Some(LayoutMode::Flex));
        assert_eq!(s.flex_direction, Some(FlexDirection::Row));
        assert_eq!(s.align_content, Some(JustifyContent::SpaceBetween));
        assert_eq!(s.align_self, Some(AlignItems::Center));
        assert_eq!((s.row_gap, s.column_gap), (Some(4.0), Some(8.0)));
        assert_eq!(s.position, Some(Position::Relative));
        assert_eq!(s.left, Some(Dimension::Points(2.0)));
        assert!((s.aspect_ratio.unwrap() - 16.0 / 9.0).abs() < 1e-6);
        let g = style("display:grid;grid-template-columns:repeat(2, 1fr) 20px");
        assert_eq!(
            g.grid_template_columns.unwrap(),
            ["1fr", "1fr", "15pt"].map(String::from)
        );
    }

    #[test]
    fn math_functions_and_relative_units() {
        let s = style(
            "width:calc(2in - 12pt); height:min(50vh, 300pt); padding:clamp(4pt, 1rem, 20pt)",
        );
        assert_eq!(s.width, Some(Dimension::Points(132.0)));
        assert_eq!(s.height, Some(Dimension::Points(300.0)));
        assert_eq!(s.padding, Some(Edges::All(11.0)));
        assert_eq!(css_length("2em", 10.0), Some(20.0));
        assert_eq!(css_length("10", 11.0), Some(7.5));
        assert_eq!(css_length("2fr", 11.0), None);
    }

    #[test]
    fn the_last_font_size_wins() {
        assert_eq!(style("font-size:10pt;font-size:20pt").font_size, Some(20.0));
    }

    #[test]
    fn semicolons_inside_urls_do_not_split_declarations() {
        let d = declarations(
            "background:url(\"data:image/png;base64,AAA\") red; color: blue !important",
        );
        assert_eq!(d.len(), 2);
        assert_eq!(d[1], ("color".into(), "blue".into(), true));
    }

    #[test]
    fn colours() {
        assert_eq!(css_color("#ABC").unwrap().to_string(), "#aabbcc");
        assert_eq!(
            css_color("rgb(0 128 255 / 50%)").unwrap().to_string(),
            "#0080ff80"
        );
        assert_eq!(css_color("teal").unwrap().to_string(), "#008080");
        assert_eq!(css_color("hsl(0, 0%, 0%)"), None);
    }
}
