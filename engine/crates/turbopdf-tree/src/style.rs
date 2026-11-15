//! CSS-inspired styles. Every field is optional.
//!
//! **Inherited** (like CSS): `fontFamily`, `fontSize`, `fontWeight`, `fontStyle`, `color`,
//! `textAlign`, `lineHeight`. All others are **not** inherited.

use crate::value::{Color, Dimension, Edges};
use serde::{Deserialize, Serialize};

macro_rules! string_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
        pub enum $name { $(#[serde(rename = $s)] $variant),+ }
    };
}

string_enum!(FlexDirection { Row = "row", RowReverse = "row-reverse", Column = "column", ColumnReverse = "column-reverse" });
string_enum!(FlexWrap { NoWrap = "nowrap", Wrap = "wrap" });
string_enum!(AlignItems { FlexStart = "flex-start", FlexEnd = "flex-end", Center = "center", Stretch = "stretch", Baseline = "baseline" });
string_enum!(JustifyContent {
    FlexStart = "flex-start", FlexEnd = "flex-end", Center = "center",
    SpaceBetween = "space-between", SpaceAround = "space-around", SpaceEvenly = "space-evenly",
});
string_enum!(TextAlign { Left = "left", Center = "center", Right = "right", Justify = "justify" });
string_enum!(FontStyle { Normal = "normal", Italic = "italic" });
string_enum!(BorderStyle { Solid = "solid", Dashed = "dashed", Dotted = "dotted" });
string_enum!(Position { Relative = "relative", Absolute = "absolute" });
string_enum!(LayoutMode { Flex = "flex", Grid = "grid" });
string_enum!(
    /// What a `view` or `text` is, for assistive technology (tagged PDF). Without a role a `view` is a plain
    /// group and a `text` a paragraph. `artifact`: decorative, skipped by screen readers.
    Role {
        Section = "section", List = "list", OrderedList = "ordered_list", ListItem = "list_item",
        ListLabel = "list_label", ListBody = "list_body", Blockquote = "blockquote", Code = "code",
        Caption = "caption", Toc = "toc", TocItem = "toc_item", Artifact = "artifact",
    }
);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Border {
    /// Width in points. `0` = no border.
    pub width: f32,
    pub color: Color,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<BorderStyle>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<LayoutMode>,
    // --- flex container
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_direction: Option<FlexDirection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_wrap: Option<FlexWrap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_items: Option<AlignItems>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_content: Option<JustifyContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify_content: Option<JustifyContent>,
    /// Space between children, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_gap: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_gap: Option<f32>,
    /// CSS Grid tracks, e.g. `["1fr", "2fr", "72pt"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_template_columns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_template_rows: Option<Vec<String>>,

    // --- flex item
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_grow: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_shrink: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_basis: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_self: Option<AlignItems>,

    // --- positioning
    /// `absolute`: taken out of the flow and placed with `top` / `right` / `bottom` / `left` inside the
    /// parent's box (on the page where the parent starts), drawn above its siblings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Dimension>,

    // --- box
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<Dimension>,
    /// Width divided by height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Edges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<Edges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,
    /// Corner radius in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<Color>,

    // --- text (inherited)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// 100..=900.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<FontStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlign>,
    /// Multiplier of the font size (`1.4` = 140%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f32>,
}

/// Text-only subset of [`Style`], used by inline [`Span`](crate::node::Span)s.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct TextStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<FontStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}
