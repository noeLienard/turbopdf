//! Style resolution: inheritance, merging, conversion to taffy.

use papier_fonts::FontDb;
use papier_text::{Align, Rgba, Span, TextStyle};
use papier_tree as t;
use taffy::prelude::*;

/// Properties that cascade down the tree (like CSS inherited properties).
#[derive(Clone, Debug, PartialEq)]
pub struct Inherited {
    pub family: String,
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub color: Rgba,
    pub align: Align,
    pub line_height: f32,
}

impl Default for Inherited {
    fn default() -> Self {
        Inherited {
            family: "Inter".into(),
            size: 11.0,
            weight: 400,
            italic: false,
            color: [0x11, 0x11, 0x11, 255],
            align: Align::Left,
            line_height: 1.4,
        }
    }
}

pub fn rgba(c: t::Color) -> Rgba {
    [c.r, c.g, c.b, c.a]
}

pub fn align_of(a: t::TextAlign) -> Align {
    match a {
        t::TextAlign::Left => Align::Left,
        t::TextAlign::Center => Align::Center,
        t::TextAlign::Right => Align::Right,
        t::TextAlign::Justify => Align::Justify,
    }
}

/// Apply the text properties of `s` on top of `parent`.
pub fn inherit(parent: &Inherited, s: Option<&t::Style>) -> Inherited {
    let mut i = parent.clone();
    let Some(s) = s else { return i };
    if let Some(v) = &s.font_family {
        i.family = v.clone()
    }
    if let Some(v) = s.font_size {
        i.size = v
    }
    if let Some(v) = s.font_weight {
        i.weight = v
    }
    if let Some(v) = s.font_style {
        i.italic = v == t::FontStyle::Italic
    }
    if let Some(v) = s.color {
        i.color = rgba(v)
    }
    if let Some(v) = s.text_align {
        i.align = align_of(v)
    }
    if let Some(v) = s.line_height {
        i.line_height = v
    }
    i
}

/// Spans of a text node with the inherited style applied (and span overrides). Span links are
/// registered in `links` (deduplicated) and referred to by index.
pub fn spans(
    db: &FontDb,
    content: &t::TextContent,
    inh: &Inherited,
    links: &mut Vec<String>,
) -> Vec<Span> {
    let mk = |i: &Inherited, text: &str, link: Option<u32>| Span {
        text: text.to_string(),
        style: TextStyle {
            face: db.select(&i.family, i.weight, i.italic),
            size: i.size,
            color: i.color,
            line_height: i.line_height,
            link,
        },
    };
    let mut link_id = |target: &str| -> u32 {
        match links.iter().position(|l| l == target) {
            Some(i) => i as u32,
            None => {
                links.push(target.to_string());
                (links.len() - 1) as u32
            }
        }
    };
    match content {
        t::TextContent::Plain(s) => vec![mk(inh, s, None)],
        t::TextContent::Spans(list) => list
            .iter()
            .map(|sp| {
                let mut i = inh.clone();
                if let Some(st) = &sp.style {
                    if let Some(v) = &st.font_family {
                        i.family = v.clone()
                    }
                    if let Some(v) = st.font_size {
                        i.size = v
                    }
                    if let Some(v) = st.font_weight {
                        i.weight = v
                    }
                    if let Some(v) = st.font_style {
                        i.italic = v == t::FontStyle::Italic
                    }
                    if let Some(v) = st.color {
                        i.color = rgba(v)
                    }
                }
                let link = sp.link.as_deref().map(&mut link_id);
                mk(&i, &sp.text, link)
            })
            .collect(),
    }
}

macro_rules! merge_fields {
    ($a:expr, $b:expr, $($f:ident),+ $(,)?) => {
        t::Style { $($f: $b.$f.clone().or_else(|| $a.$f.clone())),+ }
    };
}

/// `over` wins over `base`, field by field.
pub fn merge(base: &t::Style, over: &t::Style) -> t::Style {
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
        line_height
    )
}

pub fn merge_opt(base: Option<&t::Style>, over: Option<&t::Style>) -> t::Style {
    match (base, over) {
        (Some(a), Some(b)) => merge(a, b),
        (Some(a), None) => a.clone(),
        (None, Some(b)) => b.clone(),
        (None, None) => t::Style::default(),
    }
}

fn dim(d: t::Dimension) -> Dimension {
    match d {
        t::Dimension::Auto => Dimension::auto(),
        t::Dimension::Points(v) => Dimension::length(v),
        t::Dimension::Percent(p) => Dimension::percent(p / 100.0),
    }
}

fn dim_lpa(d: t::Dimension) -> LengthPercentageAuto {
    match d {
        t::Dimension::Auto => LengthPercentageAuto::auto(),
        t::Dimension::Points(v) => LengthPercentageAuto::length(v),
        t::Dimension::Percent(p) => LengthPercentageAuto::percent(p / 100.0),
    }
}

/// Visual (background / border / radius) of a style.
pub fn visual(s: &t::Style) -> Option<crate::Visual> {
    let border = s
        .border
        .filter(|b| b.width > 0.0 && b.color.a > 0)
        .map(|b| crate::BorderSpec {
            width: b.width,
            color: rgba(b.color),
            style: b.style.unwrap_or(t::BorderStyle::Solid),
        });
    let background = s.background_color.filter(|c| c.a > 0).map(rgba);
    let v = crate::Visual {
        background,
        border,
        radius: s.border_radius.unwrap_or(0.0),
    };
    if v.is_empty() { None } else { Some(v) }
}

/// padding + border thickness on each side: `[top, right, bottom, left]`.
pub fn insets(s: &t::Style) -> [f32; 4] {
    let p = s.padding.map_or([0.0; 4], |e| e.resolve());
    let b = s.border.map_or(0.0, |b| b.width.max(0.0));
    [p[0] + b, p[1] + b, p[2] + b, p[3] + b]
}

/// Convert to a taffy style. `column` = default flex direction (containers are columns).
pub fn to_taffy(s: &t::Style) -> Style {
    let mut ts = Style::DEFAULT;
    ts.display = match s.display {
        Some(t::LayoutMode::Grid) => Display::Grid,
        _ => Display::Flex,
    };
    ts.flex_direction = match s.flex_direction {
        Some(t::FlexDirection::Row) => FlexDirection::Row,
        Some(t::FlexDirection::RowReverse) => FlexDirection::RowReverse,
        Some(t::FlexDirection::ColumnReverse) => FlexDirection::ColumnReverse,
        Some(t::FlexDirection::Column) | None => FlexDirection::Column,
    };
    ts.flex_wrap = match s.flex_wrap {
        Some(t::FlexWrap::Wrap) => FlexWrap::Wrap,
        _ => FlexWrap::NoWrap,
    };
    ts.align_items = match s.align_items {
        Some(t::AlignItems::FlexStart) => AlignItems::FLEX_START,
        Some(t::AlignItems::FlexEnd) => AlignItems::FLEX_END,
        Some(t::AlignItems::Center) => AlignItems::CENTER,
        Some(t::AlignItems::Stretch) => AlignItems::STRETCH,
        Some(t::AlignItems::Baseline) => AlignItems::BASELINE,
        None => AlignItems::NORMAL,
    };
    ts.align_self = match s.align_self {
        Some(t::AlignItems::FlexStart) => Some(AlignSelf::FLEX_START),
        Some(t::AlignItems::FlexEnd) => Some(AlignSelf::FLEX_END),
        Some(t::AlignItems::Center) => Some(AlignSelf::CENTER),
        Some(t::AlignItems::Stretch) => Some(AlignSelf::STRETCH),
        Some(t::AlignItems::Baseline) => Some(AlignSelf::BASELINE),
        None => None,
    };
    ts.align_content = match s.align_content {
        Some(t::JustifyContent::FlexStart) => AlignContent::FLEX_START,
        Some(t::JustifyContent::FlexEnd) => AlignContent::FLEX_END,
        Some(t::JustifyContent::Center) => AlignContent::CENTER,
        Some(t::JustifyContent::SpaceBetween) => AlignContent::SPACE_BETWEEN,
        Some(t::JustifyContent::SpaceAround) => AlignContent::SPACE_AROUND,
        Some(t::JustifyContent::SpaceEvenly) => AlignContent::SPACE_EVENLY,
        None => AlignContent::NORMAL,
    };
    ts.justify_content = match s.justify_content {
        Some(t::JustifyContent::FlexStart) => AlignContent::FLEX_START,
        Some(t::JustifyContent::FlexEnd) => AlignContent::FLEX_END,
        Some(t::JustifyContent::Center) => AlignContent::CENTER,
        Some(t::JustifyContent::SpaceBetween) => AlignContent::SPACE_BETWEEN,
        Some(t::JustifyContent::SpaceAround) => AlignContent::SPACE_AROUND,
        Some(t::JustifyContent::SpaceEvenly) => AlignContent::SPACE_EVENLY,
        None => AlignContent::NORMAL,
    };
    let row_gap = s.row_gap.or(s.gap).unwrap_or(0.0);
    let column_gap = s.column_gap.or(s.gap).unwrap_or(0.0);
    ts.gap = Size {
        width: LengthPercentage::length(column_gap),
        height: LengthPercentage::length(row_gap),
    };
    if ts.display == Display::Grid {
        ts.grid_template_columns = s
            .grid_template_columns
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| grid_track(v))
            .map(GridTemplateComponent::Single)
            .collect();
        ts.grid_template_rows = s
            .grid_template_rows
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| grid_track(v))
            .map(GridTemplateComponent::Single)
            .collect();
    }
    if let Some(v) = s.flex_grow {
        ts.flex_grow = v
    }
    if let Some(v) = s.flex_shrink {
        ts.flex_shrink = v
    }
    if let Some(v) = s.flex_basis {
        ts.flex_basis = dim(v)
    }
    ts.size = Size {
        width: s.width.map_or(Dimension::auto(), dim),
        height: s.height.map_or(Dimension::auto(), dim),
    };
    ts.min_size = Size {
        width: s.min_width.map_or(LengthPercentageAuto::auto(), dim_lpa),
        height: s.min_height.map_or(LengthPercentageAuto::auto(), dim_lpa),
    };
    ts.max_size = Size {
        width: s.max_width.map_or(LengthPercentageAuto::auto(), dim_lpa),
        height: s.max_height.map_or(LengthPercentageAuto::auto(), dim_lpa),
    };
    ts.aspect_ratio = s.aspect_ratio;
    if let Some(m) = s.margin {
        let [t_, r, b, l] = m.resolve();
        ts.margin = Rect {
            top: LengthPercentageAuto::length(t_),
            right: LengthPercentageAuto::length(r),
            bottom: LengthPercentageAuto::length(b),
            left: LengthPercentageAuto::length(l),
        };
    }
    if let Some(p) = s.padding {
        let [t_, r, b, l] = p.resolve();
        ts.padding = Rect {
            top: LengthPercentage::length(t_),
            right: LengthPercentage::length(r),
            bottom: LengthPercentage::length(b),
            left: LengthPercentage::length(l),
        };
    }
    if let Some(b) = s.border {
        let w = LengthPercentage::length(b.width.max(0.0));
        ts.border = Rect {
            top: w,
            right: w,
            bottom: w,
            left: w,
        };
    }
    ts.position = match s.position {
        Some(t::Position::Absolute) => Position::Absolute,
        Some(t::Position::Relative) => Position::Relative,
        None => Position::Static,
    };
    let inset = |d: Option<t::Dimension>| d.map_or(LengthPercentageAuto::auto(), dim_lpa);
    ts.inset = Rect {
        top: inset(s.top),
        right: inset(s.right),
        bottom: inset(s.bottom),
        left: inset(s.left),
    };
    ts
}

fn grid_track(value: &str) -> Option<TrackSizingFunction> {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(TrackSizingFunction::AUTO);
    }
    if v == "min-content" {
        return Some(TrackSizingFunction::MIN_CONTENT);
    }
    if v == "max-content" {
        return Some(TrackSizingFunction::MAX_CONTENT);
    }
    if let Some(n) = v
        .strip_suffix("fr")
        .and_then(|n| n.parse::<f32>().ok())
        .filter(|n| *n >= 0.0)
    {
        return Some(TrackSizingFunction::from_fr(n));
    }
    if let Some(n) = v.strip_suffix('%').and_then(|n| n.parse::<f32>().ok()) {
        return Some(TrackSizingFunction::from_percent(n / 100.0));
    }
    if let Some(n) = v
        .strip_suffix("pt")
        .and_then(|n| n.parse::<f32>().ok())
        .filter(|n| *n >= 0.0)
    {
        return Some(TrackSizingFunction::from_length(n));
    }
    None
}

/// Default style of a heading, merged *under* the node's own style.
pub fn heading_defaults(level: u8) -> t::Style {
    let size = [28.0, 22.0, 18.0, 15.0, 13.0, 11.0][(level.clamp(1, 6) - 1) as usize];
    t::Style {
        font_size: Some(size),
        font_weight: Some(if level <= 3 { 700 } else { 600 }),
        margin: Some(t::Edges::Sides(t::EdgeSides {
            bottom: Some((size * 0.35).round()),
            ..Default::default()
        })),
        ..Default::default()
    }
}
