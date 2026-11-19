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

mod images;
mod table;
mod toc;
mod tokens;

use images::*;
use toc::*;

mod finish;
mod measure;
pub use table::*;
pub use tokens::*;

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
    /// Form fields, in document order; boxes refer to them by index.
    pub fields: Vec<crate::FieldInfo>,
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
    /// A form field occupies the box (index into [`Cx::fields`]).
    pub field: Option<u32>,
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
    indent: f32,
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
                let field = v.field.as_ref().map(|f| {
                    self.cx.fields.push(crate::FieldInfo {
                        field: f.clone(),
                        font_size: inh2.size,
                        color: inh2.color,
                        background: st
                            .background_color
                            .filter(|c| c.a > 0)
                            .map(crate::style::rgba),
                        border_width: crate::style::border_widths(&st)
                            .into_iter()
                            .fold(0.0, f32::max),
                    });
                    (self.cx.fields.len() - 1) as u32
                });
                let meta = NodeMeta {
                    anchor: v.id.clone(),
                    link: v.link.as_deref().map(|l| self.cx.link_id(l)),
                    field,
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
