//! What the engine returns next to the PDF bytes.

use serde::{Deserialize, Serialize};

/// Why a block was moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum MoveReason {
    /// `keepTogether: true` on the node.
    KeepTogether,
    /// Heading kept with the node that follows it.
    KeepWithNext,
    /// Atomic content (image, table row) that does not fit in the remaining space.
    Atomic,
}

/// A precise, non-fatal problem found while rendering. `path` is a JSON Pointer into the
/// tree (`/children/0/children/3/rows/41`), or the node `id` when it has one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum Warning {
    /// Content wider or taller than its area. `dx` / `dy` = excess in points (0 when none).
    Overflow {
        path: String,
        page: u32,
        dx: f32,
        dy: f32,
    },
    /// A block was moved to the next page so as not to be split.
    MovedToNextPage {
        path: String,
        from_page: u32,
        to_page: u32,
        reason: MoveReason,
    },
    /// A character is missing from the font (rendered with the `.notdef` glyph).
    MissingGlyph {
        path: String,
        page: u32,
        character: String,
        codepoint: u32,
        font_family: String,
    },
    /// An image could not be read or decoded.
    ImageFailed { path: String, reason: String },
    /// PDF/UA: an image without `alt` was marked decorative. Give it an `alt` (or `alt: ""` if decorative).
    MissingAlt { path: String, page: u32 },
    /// A link to `#id` (or a `pageNumber` target) names no node of the document.
    BrokenLink {
        path: String,
        page: u32,
        target: String,
    },
}
