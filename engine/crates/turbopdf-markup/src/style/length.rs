//! CSS lengths → points: units, `calc()` / `min()` / `max()` / `clamp()`, em / rem / vw / vh.

use super::*;

// ------------------------------------------------------------------ lengths

pub(crate) fn unit(u: &str) -> Option<f32> {
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

pub(crate) struct LengthParser<'a> {
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
            // `2fr`, `10deg`…: not a length, and the letters are not consumed
            u => n * unit(u)?,
        };
        self.at = j;
        Some(v)
    }
}

pub(crate) fn is_plain_number(s: &str) -> bool {
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
