//! Turn tree nodes into positioned layout nodes ([`LNode`]) using taffy for flexbox and
//! `turbopdf-text` for paragraphs. Tables are laid out here.

use crate::style::{
    self, Inherited, heading_defaults, inherit, insets, merge, merge_opt, spans, to_taffy, visual,
};
use crate::{ImageData, ImageKind, StructRole, Visual};
use base64::Engine as _;
use std::collections::HashMap;
use std::sync::Arc;
use taffy::prelude::*;
use taffy::{TaffyTree, compute_leaf_layout};
use turbopdf_fonts::{FontDb, ResourceLoader};
use turbopdf_text as text;
use turbopdf_tree as t;

/// Shared state of one layout run.
pub struct Cx<'a> {
    pub db: &'a FontDb,
    pub loader: &'a dyn ResourceLoader,
    pub images: Vec<ImageData>,
    pub warnings: Vec<t::Warning>,
    pub lang: Option<String>,
    pub hyphenate: bool,
    pub shapes: text::ShapeCache,
    /// Link targets, deduplicated; runs and boxes refer to them by index.
    pub links: Vec<String>,
    /// Headings of the section bodies, for tables of contents.
    pub toc: Vec<TocEntry>,
    /// Fonts handed to SVG text (built on first use from the document's fonts).
    pub svg_fonts: Option<Arc<usvg::fontdb::Database>>,
    /// PDF/UA: images without `alt` are made decorative (with a warning).
    pub pdfua: bool,
}

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub level: u8,
    pub title: String,
    pub anchor: String,
}

impl Cx<'_> {
    pub fn link_id(&mut self, target: &str) -> u32 {
        match self.links.iter().position(|l| l == target) {
            Some(i) => i as u32,
            None => {
                self.links.push(target.to_string());
                (self.links.len() - 1) as u32
            }
        }
    }
}

/// What a node contributes besides its geometry: an anchor others can link to, a bookmark, a link.
#[derive(Clone, Debug, Default)]
pub struct NodeMeta {
    /// `id` of the node (headings without one get their JSON Pointer, so the TOC can find them).
    pub anchor: Option<String>,
    /// Bookmark title and level, for headings of levels 1–3.
    pub heading: Option<(u8, String)>,
    /// The whole box is clickable.
    pub link: Option<u32>,
    /// `position: absolute`: out of the flow.
    pub absolute: bool,
    /// Structure element of the node (tagged PDF); `None`: a plain group, its content goes to the parent.
    pub role: Option<StructRole>,
    /// Decorative: drawn as an artifact, with everything inside.
    pub artifact: bool,
    /// PDF/UA image without `alt` (made decorative): reported with its page.
    pub missing_alt: bool,
}

/// Structure role of a `view` / `text` from its tree `role` (`default` without one); `true`: decorative.
fn role_of(role: Option<t::Role>, default: Option<StructRole>) -> (Option<StructRole>, bool) {
    let r = match role {
        None => return (default, false),
        Some(t::Role::Artifact) => return (None, true),
        Some(t::Role::Section) => StructRole::Section,
        Some(t::Role::List) => StructRole::List { ordered: false },
        Some(t::Role::OrderedList) => StructRole::List { ordered: true },
        Some(t::Role::ListItem) => StructRole::Li,
        Some(t::Role::ListLabel) => StructRole::Lbl,
        Some(t::Role::ListBody) => StructRole::LBody,
        Some(t::Role::Blockquote) => StructRole::BlockQuote,
        Some(t::Role::Code) => StructRole::Code,
        Some(t::Role::Caption) => StructRole::Caption,
        Some(t::Role::Toc) => StructRole::Toc,
        Some(t::Role::TocItem) => StructRole::Toci,
    };
    (Some(r), false)
}

/// Concatenated text of a text content (for bookmarks and tables of contents).
pub fn plain_text(c: &t::TextContent) -> String {
    match c {
        t::TextContent::Plain(s) => s.clone(),
        t::TextContent::Spans(v) => v.iter().map(|s| s.text.as_str()).collect(),
    }
}

#[derive(Clone, Debug)]
pub struct LLine {
    /// Top of the line within the text content box.
    pub y: f32,
    pub line: text::Line,
}

#[derive(Clone, Debug)]
pub struct DynamicText {
    pub format: String,
    pub inh: Inherited,
    /// `{page}` is the page of this anchor instead of the current page.
    pub target: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LRow {
    /// Top of the row within its group (header or body).
    pub y: f32,
    pub h: f32,
    /// Cell boxes, `x` relative to the table content box, `y` = 0 relative to the row.
    pub cells: Vec<LNode>,
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct LTable {
    pub header: Vec<LRow>,
    pub header_h: f32,
    pub rows: Vec<LRow>,
    pub rows_h: f32,
    pub repeat_header: bool,
}

#[derive(Clone, Debug)]
pub enum LKind {
    Box {
        children: Vec<LNode>,
        splittable: bool,
    },
    Text {
        lines: Vec<LLine>,
        missing: Vec<(char, String)>,
        dynamic: Option<DynamicText>,
    },
    Image {
        image: Option<usize>,
        fit: t::ObjectFit,
    },
    Table(LTable),
    PageBreak,
}

/// A laid-out node. `x`, `y` are relative to the parent's border box.
#[derive(Clone, Debug)]
pub struct LNode {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// padding + border: top, right, bottom, left.
    pub inset: [f32; 4],
    pub visual: Option<Visual>,
    pub keep_together: bool,
    pub keep_with_next: bool,
    /// `keepWithNext` was set in the tree (not a heading default): moving the node is worth a warning.
    pub keep_with_next_asked: bool,
    pub path: String,
    pub overflow_dx: f32,
    pub meta: NodeMeta,
    pub kind: LKind,
}

impl LNode {
    /// Children of a container node (empty for leaves).
    pub fn kind_children(&self) -> &[LNode] {
        match &self.kind {
            LKind::Box { children, .. } => children,
            _ => &[],
        }
    }
}

// ------------------------------------------------------------------ build state

enum Item {
    Text(TextItem),
    Image(ImageItem),
    Table(Box<TableItem>),
    PageBreak,
}

struct TextItem {
    /// Shaped once; `None` for empty text.
    prepared: Option<text::Prepared>,
    align: text::Align,
    dynamic: Option<DynamicText>,
}

struct ImageItem {
    image: Option<usize>,
    fit: t::ObjectFit,
    /// Natural size in points.
    natural: (f32, f32),
}

struct TableItem {
    table: t::Table,
    inh: Inherited,
}

enum Skel {
    View {
        id: NodeId,
        style: t::Style,
        children: Vec<Skel>,
        path: String,
        keep_together: bool,
        splittable: bool,
        meta: NodeMeta,
    },
    Leaf {
        id: NodeId,
        item: usize,
        style: t::Style,
        path: String,
        keep_together: bool,
        keep_with_next: bool,
        keep_with_next_asked: bool,
        meta: NodeMeta,
    },
}

struct State<'c, 'a> {
    cx: &'c mut Cx<'a>,
    items: Vec<Item>,
    paragraphs: HashMap<(usize, u32), text::Paragraph>,
    tables: HashMap<(usize, u32), (LTable, f32)>,
    /// Path of each table item, for the paths of its cells.
    table_paths: HashMap<usize, String>,
}

fn is_splittable(s: &t::Style) -> bool {
    if s.display == Some(t::LayoutMode::Grid) {
        return false;
    }
    let column = matches!(s.flex_direction, None | Some(t::FlexDirection::Column));
    let nowrap = !matches!(s.flex_wrap, Some(t::FlexWrap::Wrap));
    let auto_h = matches!(s.height, None | Some(t::Dimension::Auto))
        && s.min_height.is_none()
        && s.max_height.is_none();
    let flow = !matches!(
        s.justify_content,
        Some(
            t::JustifyContent::SpaceBetween
                | t::JustifyContent::SpaceAround
                | t::JustifyContent::SpaceEvenly
        )
    );
    column && nowrap && auto_h && flow
}

pub fn substitute_page_tokens(format: &str, page: &str, pages: &str) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(stripped) = tail.strip_prefix("{{") {
            out.push('{');
            rest = stripped;
        } else if let Some(stripped) = tail.strip_prefix("}}") {
            out.push('}');
            rest = stripped;
        } else if let Some(stripped) = tail.strip_prefix("{page}") {
            out.push_str(page);
            rest = stripped;
        } else if let Some(stripped) = tail.strip_prefix("{pages}") {
            out.push_str(pages);
            rest = stripped;
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

pub fn has_page_tokens(format: &str) -> bool {
    substitute_page_tokens(format, "\u{1}", "\u{1}").contains('\u{1}')
}

pub fn fill_page_tokens(format: &str, page: u32, pages: u32) -> String {
    substitute_page_tokens(format, &page.to_string(), &pages.to_string())
}

fn image_info(bytes: &[u8]) -> Result<(ImageKind, u32, u32), String> {
    if bytes.len() > 16 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))
            .map_err(|e| format!("invalid WebP: {e}"))?;
        let (w, h) = d.dimensions();
        return Ok((ImageKind::Webp, w, h));
    }
    if bytes.len() > 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        if w == 0 || h == 0 {
            return Err("GIF with a zero dimension".into());
        }
        return Ok((ImageKind::Gif, w, h));
    }
    if bytes.len() > 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if w == 0 || h == 0 {
            return Err("PNG with a zero dimension".into());
        }
        return Ok((ImageKind::Png, w, h));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let mut d = jpeg_decoder::Decoder::new(bytes);
        d.read_info().map_err(|e| format!("invalid JPEG: {e}"))?;
        let info = d.info().ok_or("invalid JPEG")?;
        return Ok((ImageKind::Jpeg, info.width as u32, info.height as u32));
    }
    Err("unsupported image format (use PNG, JPEG, WebP, GIF or SVG)".into())
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let head = String::from_utf8_lossy(head);
    let t = head.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg")
        || ((t.starts_with("<?xml") || t.starts_with("<!--") || t.starts_with("<!DOCTYPE"))
            && head.contains("<svg"))
}

/// Fonts for SVG text: the document's own fonts only (never the system's), Inter by default.
fn svg_fonts(cx: &mut Cx) -> Arc<usvg::fontdb::Database> {
    if let Some(db) = &cx.svg_fonts {
        return db.clone();
    }
    let mut db = usvg::fontdb::Database::new();
    for i in 0..cx.db.len() {
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> = cx.db.face(i).data.clone();
        db.load_font_source(usvg::fontdb::Source::Binary(data));
    }
    db.set_sans_serif_family("Inter");
    db.set_serif_family("Inter");
    db.set_monospace_family("Inter");
    let db = Arc::new(db);
    cx.svg_fonts = Some(db.clone());
    db
}

/// A QR code as an SVG: one path for all the dark modules (crisp edges), a 4-module quiet zone.
/// 1 module = 3 px, so the natural size is readable; the image's `width` usually sets the size.
fn qr_svg(q: &t::QrCode) -> Result<String, String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let ecc = match q.ecc.unwrap_or(t::QrEcc::M) {
        t::QrEcc::L => QrCodeEcc::Low,
        t::QrEcc::M => QrCodeEcc::Medium,
        t::QrEcc::Q => QrCodeEcc::Quartile,
        t::QrEcc::H => QrCodeEcc::High,
    };
    let code = QrCode::encode_text(&q.qr, ecc)
        .map_err(|_| "QR code text is too long for this error correction level".to_string())?;
    let n = code.size();
    let quiet = 4;
    let total = n + 2 * quiet;
    let hex = |c: t::Color| {
        if c.a == 255 {
            format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
        }
    };
    let fg = q.color.map_or_else(|| "#000000".to_string(), hex);
    let bg = q.background.map_or_else(|| "#ffffff".to_string(), hex);
    let mut d = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if code.get_module(x, y) {
                let start = x;
                while x < n && code.get_module(x, y) {
                    x += 1;
                }
                d.push_str(&format!(
                    "M{} {}h{}v1h-{}z",
                    start + quiet,
                    y + quiet,
                    x - start,
                    x - start
                ));
            } else {
                x += 1;
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{w}\" viewBox=\"0 0 {total} {total}\" shape-rendering=\"crispEdges\"><rect width=\"{total}\" height=\"{total}\" fill=\"{bg}\"/><path fill=\"{fg}\" d=\"{d}\"/></svg>",
        w = total * 3
    ))
}

fn parse_svg(cx: &mut Cx, bytes: &[u8]) -> Result<usvg::Tree, String> {
    let opts = usvg::Options {
        font_family: "Inter".into(),
        fontdb: svg_fonts(cx),
        // no file and no network: only `data:` images inside the SVG are drawn
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    usvg::Tree::from_data(bytes, &opts).map_err(|e| format!("invalid SVG: {e}"))
}

fn load_image(cx: &mut Cx, src: &t::ImageSource, path: &str) -> Option<usize> {
    let bytes = match src {
        t::ImageSource::Data { data } => base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .map_err(|e| format!("invalid base64: {e}")),
        t::ImageSource::File { file } => cx.loader.load(file),
        t::ImageSource::Buffer { buffer } => cx.loader.buffer(*buffer),
        t::ImageSource::Qr(q) => qr_svg(q).map(String::into_bytes),
    };
    let result = bytes.and_then(|b| {
        if looks_like_svg(&b) {
            let tree = parse_svg(cx, &b)?;
            let (w, h) = (tree.size().width(), tree.size().height());
            return Ok(ImageData {
                bytes: Arc::new(b),
                kind: ImageKind::Svg,
                width_px: w.ceil() as u32,
                height_px: h.ceil() as u32,
                svg: Some(Arc::new(tree)),
                natural: (w * 0.75, h * 0.75),
            });
        }
        image_info(&b).map(|(kind, w, h)| ImageData {
            bytes: Arc::new(b),
            kind,
            width_px: w,
            height_px: h,
            svg: None,
            natural: (w as f32 * 0.75, h as f32 * 0.75),
        })
    });
    match result {
        Ok(img) => {
            cx.images.push(img);
            Some(cx.images.len() - 1)
        }
        Err(reason) => {
            cx.warnings.push(t::Warning::ImageFailed {
                path: path.to_string(),
                reason,
            });
            None
        }
    }
}

fn node_id(n: &t::Node) -> Option<&str> {
    match n {
        t::Node::View(v) => v.id.as_deref(),
        t::Node::Text(v) => v.id.as_deref(),
        t::Node::Heading(v) => v.id.as_deref(),
        t::Node::Image(v) => v.id.as_deref(),
        t::Node::Table(v) => v.id.as_deref(),
        t::Node::PageBreak(v) => v.id.as_deref(),
        t::Node::PageNumber(v) => v.id.as_deref(),
        t::Node::TableOfContents(v) => v.id.as_deref(),
    }
}

fn is_absolute(s: &t::Style) -> bool {
    s.position == Some(t::Position::Absolute)
}

/// The rows of a table of contents, as ordinary nodes.
fn toc_view(toc: &t::TableOfContents, entries: &[TocEntry]) -> t::Node {
    let levels = toc.levels.unwrap_or(3);
    let rows = entries
        .iter()
        .filter(|e| e.level <= levels)
        .map(|e| {
            let row = t::Style {
                flex_direction: Some(t::FlexDirection::Row),
                justify_content: Some(t::JustifyContent::SpaceBetween),
                align_items: Some(t::AlignItems::FlexStart),
                gap: Some(12.0),
                padding: Some(t::Edges::Sides(t::EdgeSides {
                    top: Some(2.0),
                    bottom: Some(2.0),
                    left: Some((e.level - 1) as f32 * 14.0),
                    ..Default::default()
                })),
                font_weight: (e.level == 1).then_some(600),
                ..Default::default()
            };
            t::Node::View(t::View {
                id: None,
                style: Some(merge_opt(Some(&row), toc.entry_style.as_ref())),
                keep_together: None,
                link: Some(format!("#{}", e.anchor)),
                role: Some(t::Role::TocItem),
                children: vec![
                    t::Node::Text(t::Text {
                        id: None,
                        style: Some(t::Style {
                            flex_shrink: Some(1.0),
                            ..Default::default()
                        }),
                        keep_together: None,
                        role: None,
                        content: t::TextContent::Plain(e.title.clone()),
                    }),
                    t::Node::PageNumber(t::PageNumber {
                        id: None,
                        style: Some(t::Style {
                            flex_shrink: Some(0.0),
                            text_align: Some(t::TextAlign::Right),
                            ..Default::default()
                        }),
                        format: None,
                        target: Some(e.anchor.clone()),
                    }),
                ],
            })
        })
        .collect();
    t::Node::View(t::View {
        id: toc.id.clone(),
        style: toc.style.clone(),
        keep_together: None,
        link: None,
        role: Some(t::Role::Toc),
        children: rows,
    })
}

fn child_path(base: &str, i: usize) -> String {
    format!("{base}/children/{i}")
}

impl State<'_, '_> {
    /// `pointer` locates the node in the tree (children paths derive from it); warnings name the node by
    /// its `id` when it has one, by its pointer otherwise.
    fn build(
        &mut self,
        tree: &mut TaffyTree<usize>,
        node: &t::Node,
        pointer: &str,
        inh: &Inherited,
    ) -> (NodeId, Skel) {
        let label = node_id(node).unwrap_or(pointer);
        let path = label;
        match node {
            t::Node::TableOfContents(toc) => {
                let view = toc_view(toc, &self.cx.toc);
                self.build(tree, &view, pointer, inh)
            }
            t::Node::View(v) => {
                let st = v.style.clone().unwrap_or_default();
                let inh2 = inherit(inh, Some(&st));
                let (role, artifact) = role_of(v.role, None);
                let meta = NodeMeta {
                    anchor: v.id.clone(),
                    link: v.link.as_deref().map(|l| self.cx.link_id(l)),
                    absolute: is_absolute(&st),
                    role,
                    artifact,
                    ..Default::default()
                };
                let mut ids = Vec::new();
                let mut kids = Vec::new();
                for (i, c) in v.children.iter().enumerate() {
                    let (id, sk) = self.build(tree, c, &child_path(pointer, i), &inh2);
                    ids.push(id);
                    kids.push(sk);
                }
                let id = tree.new_with_children(to_taffy(&st), &ids).expect("taffy");
                let splittable = is_splittable(&st);
                (
                    id,
                    Skel::View {
                        id,
                        style: st,
                        children: kids,
                        path: path.to_string(),
                        keep_together: v.keep_together.unwrap_or(false),
                        splittable,
                        meta,
                    },
                )
            }
            t::Node::Text(tx) => {
                let st = tx.style.clone().unwrap_or_default();
                let (role, artifact) = role_of(tx.role, Some(StructRole::P));
                let meta = NodeMeta {
                    anchor: tx.id.clone(),
                    absolute: is_absolute(&st),
                    role,
                    artifact,
                    ..Default::default()
                };
                self.text_leaf(
                    tree,
                    &tx.content,
                    st,
                    inh,
                    path,
                    tx.keep_together.unwrap_or(false),
                    (false, false),
                    None,
                    meta,
                )
            }
            t::Node::Heading(h) => {
                let st = merge_opt(Some(&heading_defaults(h.level)), h.style.as_ref());
                let meta = NodeMeta {
                    anchor: Some(h.id.clone().unwrap_or_else(|| pointer.to_string())),
                    heading: (h.level <= 3)
                        .then(|| (h.level, plain_text(&h.content).trim().to_string()))
                        .filter(|(_, t)| !t.is_empty()),
                    absolute: is_absolute(&st),
                    role: Some(StructRole::H(
                        h.level.clamp(1, 6),
                        plain_text(&h.content).trim().to_string(),
                    )),
                    ..Default::default()
                };
                self.text_leaf(
                    tree,
                    &h.content,
                    st,
                    inh,
                    path,
                    h.keep_together.unwrap_or(false),
                    (
                        h.keep_with_next.unwrap_or(true),
                        h.keep_with_next == Some(true),
                    ),
                    None,
                    meta,
                )
            }
            t::Node::PageNumber(p) => {
                let st = p.style.clone().unwrap_or_default();
                let format = p.format.clone().unwrap_or_else(|| "{page}".into());
                // a target page can be far: room for three digits
                let shown = substitute_page_tokens(
                    &format,
                    if p.target.is_some() { "888" } else { "88" },
                    "88",
                );
                let dynamic =
                    (has_page_tokens(&format) || p.target.is_some()).then(|| DynamicText {
                        format,
                        inh: inherit(inh, Some(&st)),
                        target: p.target.clone(),
                    });
                let meta = NodeMeta {
                    anchor: p.id.clone(),
                    absolute: is_absolute(&st),
                    role: Some(StructRole::P),
                    ..Default::default()
                };
                self.text_leaf(
                    tree,
                    &t::TextContent::Plain(shown),
                    st,
                    inh,
                    path,
                    false,
                    (false, false),
                    dynamic,
                    meta,
                )
            }
            t::Node::Image(img) => {
                let st = img.style.clone().unwrap_or_default();
                // `alt: ""` = decorative; no alt in PDF/UA: decorative too, reported
                let alt = img.alt.as_deref().map(str::trim);
                let missing_alt = alt.is_none() && self.cx.pdfua;
                let (role, artifact) = match alt {
                    Some("") => (None, true),
                    Some(a) => (Some(StructRole::Figure(Some(a.to_string()))), false),
                    None if missing_alt => (None, true),
                    None => (Some(StructRole::Figure(None)), false),
                };
                let meta = NodeMeta {
                    anchor: img.id.clone(),
                    link: img.link.as_deref().map(|l| self.cx.link_id(l)),
                    absolute: is_absolute(&st),
                    role,
                    artifact,
                    missing_alt,
                    ..Default::default()
                };
                let image = load_image(self.cx, &img.src, path);
                let natural = image.map_or((0.0, 0.0), |i| self.cx.images[i].natural); // 96 dpi → points
                let mut ts = to_taffy(&st);
                if image.is_some() {
                    ts.aspect_ratio = Some(natural.0 / natural.1);
                    if st.max_width.is_none() {
                        ts.max_size.width = LengthPercentageAuto::percent(1.0);
                    }
                    // like <img>: natural size unless a width is given (a column would stretch it otherwise)
                    if st.width.is_none() && st.flex_grow.is_none() {
                        ts.align_self = Some(AlignSelf::FLEX_START);
                    }
                }
                self.items.push(Item::Image(ImageItem {
                    image,
                    fit: img.fit.unwrap_or(t::ObjectFit::Contain),
                    natural,
                }));
                let idx = self.items.len() - 1;
                let id = tree.new_leaf_with_context(ts, idx).expect("taffy");
                (
                    id,
                    Skel::Leaf {
                        id,
                        item: idx,
                        style: st,
                        path: path.to_string(),
                        keep_together: true,
                        keep_with_next: false,
                        keep_with_next_asked: false,
                        meta,
                    },
                )
            }
            t::Node::Table(tb) => {
                let st = tb.style.clone().unwrap_or_default();
                let meta = NodeMeta {
                    anchor: tb.id.clone(),
                    absolute: is_absolute(&st),
                    role: Some(StructRole::Table),
                    ..Default::default()
                };
                let inh2 = inherit(inh, Some(&st));
                self.items.push(Item::Table(Box::new(TableItem {
                    table: tb.clone(),
                    inh: inh2,
                })));
                let idx = self.items.len() - 1;
                self.table_paths.insert(idx, pointer.to_string());
                let id = tree
                    .new_leaf_with_context(to_taffy(&st), idx)
                    .expect("taffy");
                (
                    id,
                    Skel::Leaf {
                        id,
                        item: idx,
                        style: st,
                        path: path.to_string(),
                        keep_together: tb.keep_together.unwrap_or(false),
                        keep_with_next: false,
                        keep_with_next_asked: false,
                        meta,
                    },
                )
            }
            t::Node::PageBreak(b) => {
                let meta = NodeMeta {
                    anchor: b.id.clone(),
                    ..Default::default()
                };
                self.items.push(Item::PageBreak);
                let idx = self.items.len() - 1;
                let mut ts = Style::DEFAULT;
                ts.size = Size {
                    width: Dimension::length(0.0),
                    height: Dimension::length(0.0),
                };
                let id = tree.new_leaf_with_context(ts, idx).expect("taffy");
                (
                    id,
                    Skel::Leaf {
                        id,
                        item: idx,
                        style: t::Style::default(),
                        path: path.to_string(),
                        keep_together: true,
                        keep_with_next: false,
                        keep_with_next_asked: false,
                        meta,
                    },
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text_leaf(
        &mut self,
        tree: &mut TaffyTree<usize>,
        content: &t::TextContent,
        st: t::Style,
        inh: &Inherited,
        path: &str,
        keep_together: bool,
        keep_with_next: (bool, bool),
        dynamic: Option<DynamicText>,
        meta: NodeMeta,
    ) -> (NodeId, Skel) {
        let inh2 = inherit(inh, Some(&st));
        let sp = spans(self.cx.db, content, &inh2, &mut self.cx.links);
        let prepared = text::prepare(self.cx.db, &mut self.cx.shapes, &sp);
        self.items.push(Item::Text(TextItem {
            prepared,
            align: inh2.align,
            dynamic,
        }));
        let idx = self.items.len() - 1;
        let id = tree
            .new_leaf_with_context(to_taffy(&st), idx)
            .expect("taffy");
        (
            id,
            Skel::Leaf {
                id,
                item: idx,
                style: st,
                path: path.to_string(),
                keep_together,
                keep_with_next: keep_with_next.0,
                keep_with_next_asked: keep_with_next.1,
                meta,
            },
        )
    }

    fn paragraph(&mut self, idx: usize, width: f32) -> &text::Paragraph {
        let key = (idx, width.max(0.0).to_bits());
        if !self.paragraphs.contains_key(&key) {
            let Item::Text(item) = &self.items[idx] else {
                unreachable!()
            };
            let mut opts = text::ParagraphOptions::new(width.max(0.0));
            opts.align = item.align;
            opts.lang = self.cx.lang.clone();
            opts.hyphenate = self.cx.hyphenate;
            let p = item
                .prepared
                .as_ref()
                .map_or_else(text::Paragraph::default, |pr| {
                    text::layout_prepared(self.cx.db, pr, &opts)
                });
            self.paragraphs.insert(key, p);
        }
        &self.paragraphs[&key]
    }

    fn intrinsic(&self, idx: usize) -> (f32, f32) {
        let Item::Text(item) = &self.items[idx] else {
            unreachable!()
        };
        item.prepared
            .as_ref()
            .map_or((0.0, 0.0), |p| p.intrinsic_widths())
    }

    fn table(&mut self, idx: usize, width: f32) -> &(LTable, f32) {
        let key = (idx, width.max(0.0).to_bits());
        if !self.tables.contains_key(&key) {
            let (table, inh) = match &self.items[idx] {
                Item::Table(t) => (t.table.clone(), t.inh.clone()),
                _ => unreachable!(),
            };
            let path = self.table_paths.get(&idx).cloned().unwrap_or_default();
            let lt = layout_table(self.cx, &table, &path, width.max(0.0), &inh);
            let h = lt.header_h + lt.rows_h;
            self.tables.insert(key, (lt, h));
        }
        &self.tables[&key]
    }

    /// Size of a leaf for taffy (content box).
    fn measure(
        &mut self,
        idx: usize,
        known: Size<Option<f32>>,
        avail: Size<AvailableSpace>,
    ) -> Size<f32> {
        match &self.items[idx] {
            Item::PageBreak => Size::ZERO,
            Item::Text(_) => {
                let (min, max) = self.intrinsic(idx);
                let width = known.width.unwrap_or(match avail.width {
                    AvailableSpace::Definite(w) => w.min(max).max(0.0),
                    AvailableSpace::MinContent => min,
                    AvailableSpace::MaxContent => max,
                });
                let p = self.paragraph(idx, width);
                Size {
                    width: known.width.unwrap_or(p.width),
                    height: known.height.unwrap_or(p.height),
                }
            }
            Item::Image(img) => {
                let (nw, nh) = img.natural;
                if nw <= 0.0 || nh <= 0.0 {
                    return Size {
                        width: known.width.unwrap_or(0.0),
                        height: known.height.unwrap_or(0.0),
                    };
                }
                match (known.width, known.height) {
                    (Some(w), Some(h)) => Size {
                        width: w,
                        height: h,
                    },
                    (Some(w), None) => Size {
                        width: w,
                        height: w * nh / nw,
                    },
                    (None, Some(h)) => Size {
                        width: h * nw / nh,
                        height: h,
                    },
                    (None, None) => {
                        let w = match avail.width {
                            AvailableSpace::Definite(a) if a < nw => a,
                            _ => nw,
                        };
                        Size {
                            width: w,
                            height: w * nh / nw,
                        }
                    }
                }
            }
            Item::Table(_) => {
                // Laying a table out is the expensive part: only do it for a definite width.
                match known.width.or(match avail.width {
                    AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                }) {
                    Some(width) => {
                        let h = self.table(idx, width).1;
                        Size {
                            width,
                            height: known.height.unwrap_or(h),
                        }
                    }
                    None => Size {
                        width: if matches!(avail.width, AvailableSpace::MaxContent) {
                            480.0
                        } else {
                            0.0
                        },
                        height: known.height.unwrap_or(0.0),
                    },
                }
            }
        }
    }

    fn finish(&mut self, tree: &TaffyTree<usize>, sk: &Skel, parent_w: f32) -> LNode {
        let _ = parent_w;
        match sk {
            Skel::View {
                id,
                style,
                children,
                path,
                keep_together,
                splittable,
                meta,
            } => {
                let l = tree.layout(*id).expect("taffy");
                let inset = [
                    l.padding.top + l.border.top,
                    l.padding.right + l.border.right,
                    l.padding.bottom + l.border.bottom,
                    l.padding.left + l.border.left,
                ];
                let w = l.size.width;
                let mut kids: Vec<LNode> =
                    children.iter().map(|c| self.finish(tree, c, w)).collect();
                // Taffy places an absolute child anchored by `right` / `bottom` against the size its parent had
                // before flexing (a `flexGrow` box gets it wrong). Anchor it again on the final padding box.
                let (pad_w, pad_h) = (
                    w - l.border.left - l.border.right,
                    l.size.height - l.border.top - l.border.bottom,
                );
                for (k, c) in kids.iter_mut().zip(children) {
                    if !k.meta.absolute {
                        continue;
                    }
                    let cs = match c {
                        Skel::View { style, .. } | Skel::Leaf { style, .. } => style,
                    };
                    let resolve = |d: Option<t::Dimension>, base: f32| match d {
                        Some(t::Dimension::Points(v)) => Some(v),
                        Some(t::Dimension::Percent(p)) => Some(base * p / 100.0),
                        _ => None,
                    };
                    if let (None, Some(r)) = (resolve(cs.left, pad_w), resolve(cs.right, pad_w)) {
                        k.x = w - l.border.right - r - k.w;
                    }
                    if let (None, Some(b)) = (resolve(cs.top, pad_h), resolve(cs.bottom, pad_h)) {
                        k.y = l.size.height - l.border.bottom - b - k.h;
                    }
                }
                let right_limit = w - inset[1];
                for k in &mut kids {
                    // an absolutely positioned child lives in the padding box (it may cover the padding)
                    let limit = if k.meta.absolute {
                        w - (l.border.right)
                    } else {
                        right_limit
                    };
                    let excess = k.x + k.w - limit;
                    if excess > 0.5 {
                        k.overflow_dx = k.overflow_dx.max(excess);
                    }
                }
                LNode {
                    x: l.location.x,
                    y: l.location.y,
                    w,
                    h: l.size.height,
                    inset,
                    visual: visual(style),
                    keep_together: *keep_together,
                    keep_with_next: false,
                    keep_with_next_asked: false,
                    path: path.clone(),
                    overflow_dx: 0.0,
                    meta: meta.clone(),
                    kind: LKind::Box {
                        children: kids,
                        splittable: *splittable,
                    },
                }
            }
            Skel::Leaf {
                id,
                item,
                style,
                path,
                keep_together,
                keep_with_next,
                keep_with_next_asked,
                meta,
            } => {
                let l = tree.layout(*id).expect("taffy");
                let inset = [
                    l.padding.top + l.border.top,
                    l.padding.right + l.border.right,
                    l.padding.bottom + l.border.bottom,
                    l.padding.left + l.border.left,
                ];
                let (w, h) = (l.size.width, l.size.height);
                let content_w = (w - inset[1] - inset[3]).max(0.0);
                let mut node = LNode {
                    x: l.location.x,
                    y: l.location.y,
                    w,
                    h,
                    inset,
                    visual: visual(style),
                    keep_together: *keep_together,
                    keep_with_next: *keep_with_next,
                    keep_with_next_asked: *keep_with_next_asked,
                    path: path.clone(),
                    overflow_dx: 0.0,
                    meta: meta.clone(),
                    kind: LKind::PageBreak,
                };
                match &self.items[*item] {
                    Item::PageBreak => {}
                    Item::Text(it) => {
                        let dynamic = it.dynamic.clone();
                        let p = self.paragraph(*item, content_w).clone();
                        let mut y = 0.0;
                        let lines = p
                            .lines
                            .into_iter()
                            .map(|line| {
                                let l = LLine { y, line };
                                y += l.line.height;
                                l
                            })
                            .collect();
                        if p.width > content_w + 0.5 {
                            node.overflow_dx = p.width - content_w;
                        }
                        let missing = p
                            .missing
                            .iter()
                            .map(|m| (m.character, self.cx.db.face(m.face).family.clone()))
                            .collect();
                        node.kind = LKind::Text {
                            lines,
                            missing,
                            dynamic,
                        };
                    }
                    Item::Image(img) => {
                        node.kind = LKind::Image {
                            image: img.image,
                            fit: img.fit,
                        }
                    }
                    Item::Table(_) => {
                        self.table(*item, content_w); // make sure it is laid out
                        let key = (*item, content_w.max(0.0).to_bits());
                        let (lt, _) = self.tables.remove(&key).expect("laid out above"); // move it out: no deep clone
                        node.kind = LKind::Table(lt);
                    }
                }
                node
            }
        }
    }
}

/// Lay out `children` as a flex container of the given `width`. The result is positioned at (0, 0).
pub fn layout_box(
    cx: &mut Cx,
    style: &t::Style,
    children: &[t::Node],
    path: &str,
    width: f32,
    inh: &Inherited,
) -> LNode {
    let inh2 = inherit(inh, Some(style));
    let mut tree: TaffyTree<usize> = TaffyTree::new();
    tree.disable_rounding();
    let mut state = State {
        cx,
        items: Vec::new(),
        paragraphs: HashMap::new(),
        tables: HashMap::new(),
        table_paths: HashMap::new(),
    };

    let mut ids = Vec::new();
    let mut kids = Vec::new();
    for (i, c) in children.iter().enumerate() {
        let (id, sk) = state.build(&mut tree, c, &child_path(path, i), &inh2);
        ids.push(id);
        kids.push(sk);
    }
    let mut root_style = to_taffy(style);
    root_style.size.width = Dimension::length(width);
    root_style.max_size.width = LengthPercentageAuto::auto();
    root_style.min_size.width = LengthPercentageAuto::auto();
    let root = tree.new_with_children(root_style, &ids).expect("taffy");
    let skel = Skel::View {
        id: root,
        style: style.clone(),
        children: kids,
        path: path.to_string(),
        keep_together: false,
        splittable: is_splittable(style),
        meta: NodeMeta::default(),
    };

    tree.compute_layout_with_measure(
        root,
        Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::MaxContent,
        },
        |inputs, _id, ctx, st| match ctx {
            Some(idx) => compute_leaf_layout(
                inputs,
                st,
                |_, _| 0.0,
                |known, avail| state.measure(*idx, known, avail),
            ),
            None => compute_leaf_layout(inputs, st, |_, _| 0.0, |_, _| Size::ZERO),
        },
    )
    .expect("taffy layout");

    state.finish(&tree, &skel, width)
}

// ------------------------------------------------------------------ tables

fn column_widths(cols: &[t::Column], total: f32) -> Vec<f32> {
    let mut widths: Vec<Option<f32>> = cols
        .iter()
        .map(|c| match c.width {
            Some(t::Dimension::Points(v)) => Some(v),
            Some(t::Dimension::Percent(p)) => Some(total * p / 100.0),
            _ => None,
        })
        .collect();
    let fixed: f32 = widths.iter().flatten().sum();
    let autos = widths.iter().filter(|w| w.is_none()).count();
    let share = if autos > 0 {
        ((total - fixed) / autos as f32).max(0.0)
    } else {
        0.0
    };
    for w in &mut widths {
        w.get_or_insert(share);
    }
    widths.into_iter().map(|w| w.unwrap()).collect()
}

#[allow(clippy::too_many_arguments)]
fn layout_rows(
    cx: &mut Cx,
    tb: &t::Table,
    rows: &[t::Row],
    key: &str,
    path: &str,
    inh: &Inherited,
    widths: &[f32],
    header: bool,
) -> (Vec<LRow>, f32) {
    let mut out = Vec::new();
    let mut y = 0.0;
    for (ri, row) in rows.iter().enumerate() {
        let row_path = format!("{path}/{key}/{ri}");
        // header cells: cellStyle, then headerStyle on top (like CSS `td` + `th` rules)
        let mut base = tb.cell_style.clone().unwrap_or_default();
        if header {
            base = merge_opt(Some(&base), tb.header_style.as_ref());
        }
        if !header
            && ri % 2 == 1
            && let Some(alt) = &tb.alt_row_style
        {
            base = merge(&base, alt);
        }
        if let Some(rs) = &row.style {
            base = merge(&base, rs);
        }
        let mut cells = Vec::new();
        let mut col = 0usize;
        let mut x = 0.0;
        for (ci, cell) in row.cells.iter().enumerate() {
            let span = cell.col_span.unwrap_or(1).max(1) as usize;
            let w: f32 = widths.iter().skip(col).take(span).sum();
            let cell_style = merge_opt(Some(&base), cell.style.as_ref());
            let mut cinh = inh.clone();
            if let Some(a) = tb.columns.get(col).and_then(|c| c.align) {
                cinh.align = style::align_of(a);
            }
            let cell_path = format!("{row_path}/cells/{ci}");
            let mut node =
                match fast_text_cell(cx, &cell_style, &cell.children, &cell_path, w, &cinh) {
                    Some(n) => n,
                    None => layout_box(cx, &cell_style, &cell.children, &cell_path, w, &cinh),
                };
            node.x = x;
            node.y = 0.0;
            node.meta.role = Some(if header {
                StructRole::Th
            } else {
                StructRole::Td
            });
            cells.push(node);
            x += w;
            col += span;
        }
        let h = cells.iter().map(|c| c.h).fold(0.0, f32::max);
        for c in &mut cells {
            c.h = h; // backgrounds fill the whole row
        }
        out.push(LRow {
            y,
            h,
            cells,
            path: row_path,
        });
        y += h;
    }
    (out, y)
}

pub fn layout_table(cx: &mut Cx, tb: &t::Table, path: &str, width: f32, inh: &Inherited) -> LTable {
    let widths = column_widths(&tb.columns, width);
    let (header, header_h) = layout_rows(
        cx,
        tb,
        tb.header.as_deref().unwrap_or_default(),
        "header",
        path,
        inh,
        &widths,
        true,
    );
    let (rows, rows_h) = layout_rows(cx, tb, &tb.rows, "rows", path, inh, &widths, false);
    LTable {
        header,
        header_h,
        rows,
        rows_h,
        repeat_header: tb.repeat_header.unwrap_or(true),
    }
}

/// A table cell holding a single unstyled text is by far the most common case. Laying it out with a full
/// flexbox tree costs ~10x more than computing it directly; the result is identical.
fn fast_text_cell(
    cx: &mut Cx,
    style: &t::Style,
    children: &[t::Node],
    path: &str,
    width: f32,
    inh: &Inherited,
) -> Option<LNode> {
    let [t::Node::Text(tx)] = children else {
        return None;
    };
    let simple_container = style.height.is_none()
        && style.min_height.is_none()
        && style.max_height.is_none()
        && style.max_width.is_none()
        && style.min_width.is_none()
        && style.width.is_none()
        && !matches!(
            style.flex_direction,
            Some(
                t::FlexDirection::Row
                    | t::FlexDirection::RowReverse
                    | t::FlexDirection::ColumnReverse
            )
        )
        && matches!(style.align_items, None | Some(t::AlignItems::Stretch))
        && !matches!(
            style.justify_content,
            Some(
                t::JustifyContent::Center
                    | t::JustifyContent::FlexEnd
                    | t::JustifyContent::SpaceBetween
                    | t::JustifyContent::SpaceAround
                    | t::JustifyContent::SpaceEvenly
            )
        );
    if !simple_container
        || tx.style.is_some()
        || tx.keep_together == Some(true)
        || is_absolute(style)
    {
        return None;
    }
    let inset = insets(style);
    let content_w = (width - inset[1] - inset[3]).max(0.0);
    let cinh = inherit(inh, Some(style));
    let sp = spans(cx.db, &tx.content, &cinh, &mut cx.links);
    let prepared = text::prepare(cx.db, &mut cx.shapes, &sp);
    let mut opts = text::ParagraphOptions::new(content_w);
    opts.align = cinh.align;
    opts.lang = cx.lang.clone();
    opts.hyphenate = cx.hyphenate;
    let p = prepared
        .as_ref()
        .map_or_else(text::Paragraph::default, |pr| {
            text::layout_prepared(cx.db, pr, &opts)
        });

    let mut y = 0.0;
    let lines = p
        .lines
        .into_iter()
        .map(|line| {
            let l = LLine { y, line };
            y += l.line.height;
            l
        })
        .collect();
    let missing = p
        .missing
        .iter()
        .map(|m| (m.character, cx.db.face(m.face).family.clone()))
        .collect();
    let text_node = LNode {
        x: inset[3],
        y: inset[0],
        w: content_w,
        h: p.height,
        inset: [0.0; 4],
        visual: None,
        keep_together: false,
        keep_with_next: false,
        keep_with_next_asked: false,
        path: format!("{path}/children/0"),
        overflow_dx: if p.width > content_w + 0.5 {
            p.width - content_w
        } else {
            0.0
        },
        // the text of a simple cell goes straight into its TD / TH (no P around it: smaller structure tree)
        meta: NodeMeta {
            anchor: tx.id.clone(),
            role: role_of(tx.role, None).0,
            artifact: tx.role == Some(t::Role::Artifact),
            ..Default::default()
        },
        kind: LKind::Text {
            lines,
            missing,
            dynamic: None,
        },
    };
    Some(LNode {
        x: 0.0,
        y: 0.0,
        w: width,
        h: inset[0] + p.height + inset[2],
        inset,
        visual: visual(style),
        keep_together: false,
        keep_with_next: false,
        keep_with_next_asked: false,
        path: path.to_string(),
        overflow_dx: 0.0,
        meta: NodeMeta::default(),
        kind: LKind::Box {
            children: vec![text_node],
            splittable: true,
        },
    })
}
