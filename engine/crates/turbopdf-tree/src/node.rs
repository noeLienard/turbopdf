//! The nodes of the document tree.
//!
//! ```text
//! document
//! └── page            (a section: page size, margins, header, footer; flows onto N physical pages)
//!     ├── view        (flex container)
//!     ├── text / heading / image
//!     ├── table       (header rows repeat; rows are never split)
//!     ├── page_break
//!     ├── page_number
//!     └── table_of_contents
//! ```

use crate::style::{FontStyle, Role, Style, TextAlign, TextStyle};
use crate::value::{Color, Dimension};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- document

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum DocumentTag {
    #[default]
    #[serde(rename = "document")]
    Document,
}

/// A PDF/A conformance level: part (1 to 4) and level (`b` basic, `u` Unicode text, `a` accessible).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum PdfA {
    #[serde(rename = "1b")]
    A1b,
    #[serde(rename = "1a")]
    A1a,
    #[serde(rename = "2b")]
    A2b,
    #[serde(rename = "2u")]
    A2u,
    #[serde(rename = "2a")]
    A2a,
    #[serde(rename = "3b")]
    A3b,
    #[serde(rename = "3u")]
    A3u,
    #[serde(rename = "3a")]
    A3a,
    #[serde(rename = "4")]
    A4,
}

impl PdfA {
    /// The `a` levels: the PDF must be tagged.
    pub fn needs_tagging(self) -> bool {
        matches!(self, Self::A1a | Self::A2a | Self::A3a)
    }

    /// As written: `"PDF/A-3b"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::A1b => "PDF/A-1b",
            Self::A1a => "PDF/A-1a",
            Self::A2b => "PDF/A-2b",
            Self::A2u => "PDF/A-2u",
            Self::A2a => "PDF/A-2a",
            Self::A3b => "PDF/A-3b",
            Self::A3u => "PDF/A-3u",
            Self::A3a => "PDF/A-3a",
            Self::A4 => "PDF/A-4",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum PageTag {
    #[default]
    #[serde(rename = "page")]
    Page,
}

/// Root of the tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Document {
    #[serde(rename = "type")]
    #[cfg_attr(feature = "ts", ts(rename = "type"))]
    pub kind: DocumentTag,
    /// Must equal [`TREE_VERSION`](crate::TREE_VERSION).
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Root style, inherited by everything. Defaults: Inter 11pt, weight 400, `#111111`, lineHeight 1.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_style: Option<Style>,
    /// Extra fonts. Inter (400/600/700, normal/italic) is always available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fonts: Option<Vec<FontFace>>,
    /// PDF bookmarks (outline) generated from the headings of levels 1–3. Default `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmarks: Option<bool>,
    /// Tagged PDF: the logical structure (headings, paragraphs, lists, tables, figures with their `alt`,
    /// reading order) for screen readers and reflow. Default `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagged: Option<bool>,
    /// Conform to PDF/UA-1 (accessibility standard): requires `meta.title` and `meta.lang`, images without
    /// `alt` become decorative (with a `missing_alt` warning), a missing glyph is an error. Default `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdfua: Option<bool>,
    /// Conform to a PDF/A level (ISO 19005, long-term archiving): fonts and colors fully embedded, nothing
    /// external, an sRGB output intent, XMP metadata. Needs `meta.creationDate`; the `a` levels need tagging.
    /// PDF/A-1 forbids transparency (opacity, watermarks). Default: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdfa: Option<PdfA>,
    /// At least one page.
    pub children: Vec<Page>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    /// BCP 47, e.g. `"fr"`. Drives hyphenation and the PDF language tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// ISO 8601. **Never defaulted to "now"**: absent = no date written (deterministic output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<String>,
}

// ---------------------------------------------------------------- fonts

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct FontFace {
    pub family: String,
    /// 100..=900
    pub weight: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<FontStyle>,
    pub source: BinarySource,
}

/// Bytes given inline (base64) or by file path. The engine core does no I/O itself:
/// `file` is resolved by the host (Node binding) through a resource loader; in WebAssembly
/// without a loader it yields a warning (`image_failed`) or an error (fonts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum BinarySource {
    Data {
        data: String,
    },
    File {
        file: String,
    },
    /// Index of a buffer handed to the engine next to the tree (Node binding): raw bytes, no base64.
    Buffer {
        buffer: u32,
    },
}

/// Where an image comes from: bytes (base64 or file), or a QR code drawn by the engine (vector).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum ImageSource {
    Data {
        data: String,
    },
    File {
        file: String,
    },
    /// Index of a buffer handed to the engine next to the tree (Node binding): raw bytes, no base64.
    Buffer {
        buffer: u32,
    },
    Qr(QrCode),
}

/// A QR code: `{ "qr": "https://…" }`. Drawn as vector shapes, sharp at any size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct QrCode {
    /// Text to encode: a URL, a payment string (EPC/SEPA), a vCard…
    pub qr: String,
    /// Error correction: `L` 7 %, `M` 15 % (default), `Q` 25 %, `H` 30 % of the code can be damaged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ecc: Option<QrEcc>,
    /// Colour of the modules. Default black.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Colour behind the code. Default white (scanners need contrast).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum QrEcc {
    L,
    M,
    Q,
    H,
}

// ---------------------------------------------------------------- page

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum PageFormat {
    A3,
    A4,
    A5,
    Letter,
    Legal,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum PageSize {
    Format(PageFormat),
    /// Points.
    Custom {
        width: f32,
        height: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// A section. Its `children` flow onto as many physical pages as needed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Page {
    #[serde(rename = "type")]
    #[cfg_attr(feature = "ts", ts(rename = "type"))]
    pub kind: PageTag,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Default: A4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<PageSize>,
    /// Default: portrait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<Orientation>,
    /// Default: 36pt on every side (≈ 12.7 mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<crate::value::Edges>,
    /// Page background / default text style for this section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Repeated on every physical page, inside the margin, above the body.
    /// `{page}` and `{pages}` are substituted in text (use `{{` / `}}` for literal braces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<Node>>,
    /// Repeated on every physical page, inside the margin, below the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<Vec<Node>>,
    /// Text stamped across every physical page of this section (`DRAFT`, `PAID`…), above the content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<Watermark>,
    pub children: Vec<Node>,
}

/// Large rotated text drawn across a page, centered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Watermark {
    pub text: String,
    /// Default: black at 12 % opacity. Use the alpha channel for the opacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<crate::value::Color>,
    /// Points. Default: sized to span ~75 % of the page diagonal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// Default 700.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    /// Default Inter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Degrees, counter-clockwise. Default 45 (bottom-left to top-right).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
}

// ---------------------------------------------------------------- nodes

// Boxing the variants would leak into the JSON-facing types for no real gain: trees are built once.
#[allow(clippy::large_enum_variant)]
/// Any node that can appear in a page body, a view, a cell, a header or a footer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum Node {
    View(View),
    Text(Text),
    Heading(Heading),
    Image(Image),
    Table(Table),
    PageBreak(PageBreak),
    PageNumber(PageNumber),
    TableOfContents(TableOfContents),
}

/// Flex container (`display: flex`, default direction `column`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct View {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Never split this node across pages (moved to the next page if it fits there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_together: Option<bool>,
    /// Makes the whole box clickable: a URL (`https:`, `mailto:`…) or `#id` of a node of the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// What the box is (list, list item, section…), for tagged PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub children: Vec<Node>,
}

/// Plain string or a list of styled spans.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum TextContent {
    Plain(String),
    Spans(Vec<Span>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Span {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TextStyle>,
    /// Makes the span clickable: a URL (`https:`, `mailto:`…) or `#id` of a node of the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Text {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Never split the lines of this paragraph across pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_together: Option<bool>,
    /// What the text is (code, quote, list label…), for tagged PDF. Default: a paragraph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub content: TextContent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Heading {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 1..=6. Default sizes: 28, 22, 18, 15, 13, 11pt, weight 700 (600 for 4–6).
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Default `true`: a heading is never left alone at the bottom of a page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_together: Option<bool>,
    pub content: TextContent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    /// Drawn as vectors in the PDF. External references (`<image href>` to files or URLs) are ignored.
    Svg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tree.ts"))]
pub enum ObjectFit {
    Contain,
    Cover,
    Fill,
    None,
}

/// Image: PNG, JPEG, WebP, GIF (first frame) or SVG. No network: the SDK/host provides the bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Image {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub src: ImageSource,
    /// Detected from the bytes when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ImageFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    /// Default `contain`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<ObjectFit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Makes the image clickable: a URL or `#id` of a node of the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Column {
    /// Default `auto` (shares the remaining width equally).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<TextAlign>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Cell {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col_span: Option<u32>,
    pub children: Vec<Node>,
}

/// A table row. **Rows are atomic: never split across pages.**
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Row {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// The sum of `colSpan` must equal the number of columns.
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct Table {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    pub columns: Vec<Column>,
    /// Header rows, repeated on every page the table spans (unless `repeatHeader` is false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<Row>>,
    pub rows: Vec<Row>,
    /// Default `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_header: Option<bool>,
    /// Style applied to every header cell, on top of `cellStyle` and under the cell's own style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_style: Option<Style>,
    /// Style applied to every cell (header cells too), under the cell's own style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_style: Option<Style>,
    /// Style applied to every second body row (zebra), under the cell's own style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_row_style: Option<Style>,
    /// Keep the whole table on one page (if it fits on one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_together: Option<bool>,
}

/// Forces the following content onto a new physical page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct PageBreak {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Text node showing the page number. Equivalent to `text` with `{page}` in a header/footer,
/// but also valid in the page body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct PageNumber {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Default `"{page}"`. Example: `"Page {page} / {pages}"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// `id` of a node: `{page}` is then the page where that node starts (cross references, "see page 4").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Table of contents: one line per heading of the document (levels 1 to `levels`), with its page
/// number, each line linking to its heading. Headings inside tables, headers and footers are left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tree.ts", optional_fields)
)]
pub struct TableOfContents {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Deepest heading level listed. Default 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<u8>,
    /// Style of every line (a row: title on the left, page number on the right). Level 2+ lines are indented.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_style: Option<Style>,
}
