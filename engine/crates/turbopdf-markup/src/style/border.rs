//! Borders: `border`, `border-top`…, `border-width` / `-style` / `-color` (one to four values) and
//! `border-top-width`…, resolved side by side in declaration order, as a browser does. Written as one
//! `border` when the four sides end up the same, else as `borderTop`… for the sides that were set.

use super::{css_color, css_length, tokens};
use turbopdf_tree::{Border, BorderStyle, Color, Style};

/// What the declarations of one rule say about one side. `style: Some(None)` = `none`.
#[derive(Clone, Copy, Default, PartialEq)]
struct Side {
    set: bool,
    width: Option<f32>,
    color: Option<Color>,
    style: Option<Option<BorderStyle>>,
}

#[derive(Default)]
pub(super) struct Borders {
    sides: [Side; 4],
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];
/// `medium`, the width of a border with a style and no width: 3px.
const MEDIUM: f32 = 2.25;

fn style_of(t: &str) -> Option<Option<BorderStyle>> {
    Some(match t {
        "none" | "hidden" => None,
        "solid" | "double" | "groove" | "ridge" | "inset" | "outset" => Some(BorderStyle::Solid),
        "dashed" => Some(BorderStyle::Dashed),
        "dotted" => Some(BorderStyle::Dotted),
        _ => return None,
    })
}

fn width_of(t: &str, font_size: f32) -> Option<f32> {
    match t {
        "thin" => Some(0.75),
        "medium" => Some(MEDIUM),
        "thick" => Some(3.75),
        _ => css_length(t, font_size).map(|w| w.max(0.0)),
    }
}

/// `a`, `a b`, `a b c`, `a b c d` → top, right, bottom, left.
fn four<T: Copy>(v: &[T]) -> Option<[T; 4]> {
    Some(match *v {
        [a] => [a; 4],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d, ..] => [a, b, c, d],
        [] => return None,
    })
}

impl Borders {
    /// Apply one declaration. `false` if it is not about borders.
    pub(super) fn apply(&mut self, key: &str, value: &str, font_size: f32) -> bool {
        let Some(rest) = key.strip_prefix("border") else {
            return false;
        };
        let v = value.trim();
        match rest {
            "" => (0..4).for_each(|i| self.shorthand(i, v, font_size)),
            "-width" => {
                let w: Vec<_> = tokens(v)
                    .iter()
                    .filter_map(|t| width_of(t, font_size))
                    .collect();
                if let Some(w) = four(&w) {
                    (0..4).for_each(|i| self.edit(i, |s| s.width = Some(w[i])));
                }
            }
            "-style" => {
                let st: Vec<_> = tokens(v).iter().filter_map(|t| style_of(t)).collect();
                if let Some(st) = four(&st) {
                    (0..4).for_each(|i| self.edit(i, |s| s.style = Some(st[i])));
                }
            }
            "-color" => {
                let c: Vec<_> = tokens(v).iter().filter_map(|t| css_color(t)).collect();
                if let Some(c) = four(&c) {
                    (0..4).for_each(|i| self.edit(i, |s| s.color = Some(c[i])));
                }
            }
            _ => {
                let rest = &rest[1..];
                let (side, part) = rest.split_once('-').unwrap_or((rest, ""));
                let Some(i) = SIDES.iter().position(|s| *s == side) else {
                    return false; // border-radius, border-collapse, border-spacing…
                };
                match part {
                    "" => self.shorthand(i, v, font_size),
                    "width" => {
                        let w = width_of(v, font_size);
                        self.edit(i, |s| s.width = w.or(s.width));
                    }
                    "style" => {
                        let st = style_of(v);
                        self.edit(i, |s| s.style = st.or(s.style));
                    }
                    "color" => {
                        let c = css_color(v);
                        self.edit(i, |s| s.color = c.or(s.color));
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    fn edit(&mut self, i: usize, f: impl FnOnce(&mut Side)) {
        let s = &mut self.sides[i];
        s.set = true;
        f(s);
    }

    /// `1px solid #ccc` in any order; what is left out is reset (medium, black, none).
    fn shorthand(&mut self, i: usize, v: &str, font_size: f32) {
        let mut side = Side {
            set: true,
            style: Some(None),
            ..Side::default()
        };
        if v != "0" {
            for t in tokens(v) {
                if let Some(st) = style_of(t) {
                    side.style = Some(st);
                } else if let Some(w) = width_of(t, font_size) {
                    side.width = Some(w);
                } else if let Some(c) = css_color(t) {
                    side.color = Some(c);
                }
            }
        }
        self.sides[i] = side;
    }

    /// Write the borders this rule sets into `s`.
    pub(super) fn write(self, s: &mut Style) {
        let resolved = self.sides.map(|side| {
            if !side.set {
                return None;
            }
            // a width or a colour without a style says nothing visible on its own
            let style = side.style?;
            Some(match style {
                None => Border {
                    width: 0.0,
                    color: Color::BLACK,
                    style: None,
                },
                Some(st) => Border {
                    width: side.width.unwrap_or(MEDIUM),
                    color: side.color.unwrap_or(Color::BLACK),
                    style: Some(st),
                },
            })
        });
        if resolved[0].is_some() && resolved.iter().all(|b| *b == resolved[0]) {
            s.border = resolved[0];
            return;
        }
        let [t, r, b, l] = resolved;
        s.border_top = t;
        s.border_right = r;
        s.border_bottom = b;
        s.border_left = l;
    }
}
