//! Layout and pagination — the heart of the engine.
//!
//! 1. Resolve styles (CSS-like inheritance) down the tree ([`style`]).
//! 2. Lay each section body out on an infinite-height canvas with taffy (flexbox); text is shaped
//!    by `papier-text`, tables are laid out here ([`build`]).
//! 3. Paginate: cut that canvas into physical pages, following the rules of `SPEC.md` §6
//!    ([`paginate`]): rows never split, headers repeat, `keepTogether`, orphans/widows, headings kept with
//!    what follows.
//! 4. Header / footer are laid out per physical page once the real page count is known.
//!
//! The result, [`Layout`], is a flat list of absolutely-positioned drawing operations per page, shared by
//! the PDF and PNG backends.

mod build;
mod finalize;
mod paginate;
mod style;

use papier_fonts::{FontDb, ResourceLoader};
use papier_text::{Rgba, Run};
use papier_tree::{BorderStyle, Document, ObjectFit, Warning};
use std::collections::BTreeMap;
use std::sync::Arc;

pub use usvg;

pub use papier_fonts::FaceId;

/// URL schemes that are safe to expose as clickable links in every output backend.
/// Internal `#anchor` links are handled separately by each backend.
pub fn is_safe_external_link(target: &str) -> bool {
    const SCHEMES: [&str; 5] = ["http:", "https:", "mailto:", "tel:", "ftp:"];
    let lower = target.trim().to_ascii_lowercase();
    SCHEMES.iter().any(|scheme| lower.starts_with(scheme))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSpec {
    pub width: f32,
    pub color: Rgba,
    pub style: BorderStyle,
}

/// Box decoration.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Visual {
    pub background: Option<Rgba>,
    pub border: Option<BorderSpec>,
    pub radius: f32,
}

impl Visual {
    pub fn is_empty(&self) -> bool {
        self.background.is_none() && self.border.is_none()
    }
}

/// One drawing command. Coordinates are in points, origin top-left of the page.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawOp {
    /// Background and border of a box. `sides` = which borders to draw (top, right, bottom, left):
    /// a box cut by a page break has no bottom border on the first part and no top border on the next.
    Box {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        visual: Visual,
        sides: [bool; 4],
    },
    /// A run of glyphs. `x` is the position of the first glyph, `baseline` the y of the baseline.
    Text { x: f32, baseline: f32, run: Run },
    /// A decoded image placed in its box. `image` indexes [`Layout::images`].
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image: usize,
        fit: ObjectFit,
    },
    /// A run rotated by `angle` degrees (counter-clockwise) around (`cx`, `cy`); `x` / `baseline` are
    /// relative to that center before rotation. Used for watermarks.
    Rotated {
        cx: f32,
        cy: f32,
        angle: f32,
        x: f32,
        baseline: f32,
        run: Run,
    },
}

/// A clickable area. `link` indexes [`Layout::links`]; `elem` is its `Link` structure element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub link: u32,
    pub elem: Option<u32>,
}

/// Logical role of a structure element (tagged PDF).
#[derive(Clone, Debug, PartialEq)]
pub enum StructRole {
    Section,
    P,
    /// Heading level and its text.
    H(u8, String),
    Figure(Option<String>),
    Table,
    Tr,
    Th,
    Td,
    List {
        ordered: bool,
    },
    Li,
    Lbl,
    LBody,
    BlockQuote,
    Code,
    Caption,
    Toc,
    Toci,
    Link,
}

/// A node of the structure tree. Children are the elements whose `parent` is this one, and the content
/// marked with it, ordered by `seq` (reading order).
#[derive(Clone, Debug, PartialEq)]
pub struct StructElem {
    pub role: StructRole,
    pub parent: Option<u32>,
    pub seq: u32,
}

/// Content that is not part of the logical structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// Backgrounds, borders, decorative images.
    Layout,
    Header,
    Footer,
    Watermark,
    /// Repeated table headers.
    Pagination,
}

/// What a drawing operation is, for tagged PDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Artifact(ArtifactKind),
    /// Content of the structure element `elem`; `seq` orders it among the element's children.
    Content {
        elem: u32,
        seq: u32,
    },
}

/// Where a node with an `id` starts: page index (0-based) and position on that page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub page: usize,
    pub x: f32,
    pub y: f32,
}

/// A PDF bookmark, from a heading.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineEntry {
    pub level: u8,
    pub title: String,
    pub page: usize,
    pub y: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageLayout {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<DrawOp>,
    /// One per op.
    pub tags: Vec<Tag>,
    pub links: Vec<LinkRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Webp,
    Gif,
    Svg,
}

#[derive(Clone)]
pub struct ImageData {
    pub bytes: Arc<Vec<u8>>,
    pub kind: ImageKind,
    /// Natural size in CSS pixels (SVG: its width / height or viewBox).
    pub width_px: u32,
    pub height_px: u32,
    /// Parsed SVG, shared by the PDF and PNG backends.
    pub svg: Option<Arc<usvg::Tree>>,
    /// Natural size in points (exact, also for fractional SVG sizes).
    pub natural: (f32, f32),
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageData")
            .field("kind", &self.kind)
            .field("width_px", &self.width_px)
            .field("height_px", &self.height_px)
            .finish()
    }
}

/// Result of layout.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub pages: Vec<PageLayout>,
    pub images: Vec<ImageData>,
    pub warnings: Vec<Warning>,
    /// Link targets: a URL, or `#id` of a node (see [`Layout::anchors`]).
    pub links: Vec<String>,
    /// Nodes with an `id` (and headings), by name.
    pub anchors: BTreeMap<String, Anchor>,
    /// PDF bookmarks, in document order.
    pub outline: Vec<OutlineEntry>,
    /// Structure tree (tagged PDF), indexed by [`Tag::Content::elem`].
    pub structure: Vec<StructElem>,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("{0}")]
    Invalid(String),
}

/// Lay a validated document out. Fonts must already contain the document's own fonts.
pub fn layout_document(
    doc: &Document,
    db: &FontDb,
    loader: &dyn ResourceLoader,
) -> Result<Layout, LayoutError> {
    finalize::run(doc, db, loader)
}
