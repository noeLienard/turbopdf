//! Primitive values shared by styles and nodes. Units are **points** (1pt = 1/72 inch).

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A length: `12` (points), `"auto"`, or `"50%"`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", type = "number | 'auto' | `${number}%`")
)]
pub enum Dimension {
    Auto,
    Points(f32),
    Percent(f32),
}

impl Serialize for Dimension {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match *self {
            Dimension::Auto => s.serialize_str("auto"),
            Dimension::Points(v) => s.serialize_f32(v),
            Dimension::Percent(p) => s.serialize_str(&format!("{p}%")),
        }
    }
}

impl<'de> Deserialize<'de> for Dimension {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Dimension;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number of points, \"auto\" or a percentage like \"50%\"")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Dimension, E> {
                Ok(Dimension::Points(v as f32))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Dimension, E> {
                Ok(Dimension::Points(v as f32))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Dimension, E> {
                Ok(Dimension::Points(v as f32))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Dimension, E> {
                if v == "auto" {
                    return Ok(Dimension::Auto);
                }
                v.strip_suffix('%')
                    .and_then(|n| n.trim().parse::<f32>().ok())
                    .map(Dimension::Percent)
                    .ok_or_else(|| {
                        E::custom(format!(
                            "invalid dimension {v:?}: use a number, \"auto\" or \"50%\""
                        ))
                    })
            }
        }
        d.deserialize_any(V)
    }
}

/// Spacing: one value for all sides, or per side.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum Edges {
    All(f32),
    Sides(EdgeSides),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct EdgeSides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<f32>,
}

impl Edges {
    /// `[top, right, bottom, left]` in points, missing sides = 0.
    pub fn resolve(&self) -> [f32; 4] {
        match *self {
            Edges::All(v) => [v; 4],
            Edges::Sides(s) => [
                s.top.unwrap_or(0.0),
                s.right.unwrap_or(0.0),
                s.bottom.unwrap_or(0.0),
                s.left.unwrap_or(0.0),
            ],
        }
    }
}

/// sRGB color with alpha, written as a string: `#rgb`, `#rrggbb`, `#rrggbbaa` or `transparent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", type = "string")
)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` or `transparent`.
    pub fn parse(s: &str) -> Result<Color, String> {
        if s == "transparent" {
            return Ok(Color::TRANSPARENT);
        }
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| format!("invalid color {s:?}: use #rgb, #rrggbb or #rrggbbaa"))?;
        let digit = |c: char| {
            c.to_digit(16)
                .map(|d| d as u8)
                .ok_or_else(|| format!("invalid color {s:?}: {c:?} is not a hex digit"))
        };
        let chars: Vec<char> = hex.chars().collect();
        let pair =
            |i: usize| -> Result<u8, String> { Ok(digit(chars[i])? * 16 + digit(chars[i + 1])?) };
        let short = |i: usize| -> Result<u8, String> { Ok(digit(chars[i])? * 17) };
        match chars.len() {
            3 => Ok(Color {
                r: short(0)?,
                g: short(1)?,
                b: short(2)?,
                a: 255,
            }),
            4 => Ok(Color {
                r: short(0)?,
                g: short(1)?,
                b: short(2)?,
                a: short(3)?,
            }),
            6 => Ok(Color {
                r: pair(0)?,
                g: pair(2)?,
                b: pair(4)?,
                a: 255,
            }),
            8 => Ok(Color {
                r: pair(0)?,
                g: pair(2)?,
                b: pair(4)?,
                a: pair(6)?,
            }),
            _ => Err(format!(
                "invalid color {s:?}: use #rgb, #rrggbb or #rrggbbaa"
            )),
        }
    }
}

impl fmt::Display for Color {
    /// Canonical form: `#rrggbb`, or `#rrggbbaa` when not opaque. Lowercase.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(
                f,
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a
            )
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).map_err(de::Error::custom)
    }
}
