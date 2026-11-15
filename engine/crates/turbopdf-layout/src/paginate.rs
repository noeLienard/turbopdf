//! Pagination: cut a laid-out section body into physical pages.
//!
//! Rules (SPEC.md §6): rows never split, table headers repeat, `keepTogether` moves a block that would be
//! cut, paragraphs split between lines (orphans/widows = 2), a heading is kept with what follows, a block
//! taller than a page is placed at the top of a page and reported as `overflow`.

use crate::build::{DynamicText, LKind, LNode, LRow, LTable};
use crate::{ArtifactKind, DrawOp, LinkRect, StructElem, StructRole, Tag, Visual};
use std::collections::HashMap;
use turbopdf_text::Run;
use turbopdf_tree::{MoveReason, Warning};

const EPS: f32 = 0.01;

/// A drawing operation, or a page-number text resolved once the page count is known.
#[derive(Clone, Debug)]
pub enum Op {
    Draw(DrawOp, Tag),
    Dynamic {
        x: f32,
        y: f32,
        w: f32,
        text: DynamicText,
        tag: Tag,
    },
    /// A clickable area.
    Link(LinkRect),
    /// A node with an `id` starts here.
    Anchor {
        name: String,
        x: f32,
        y: f32,
    },
    /// A heading starts here (bookmark).
    Heading {
        level: u8,
        title: String,
        y: f32,
    },
}

/// The structure tree being built, shared by the sections of a document (tagged PDF).
#[derive(Default)]
pub struct Structure {
    pub elems: Vec<StructElem>,
    by_key: HashMap<String, u32>,
    seq: u32,
}

impl Structure {
    fn next(&mut self) -> u32 {
        self.seq += 1;
        self.seq
    }

    fn create(&mut self, role: StructRole, parent: Option<u32>) -> u32 {
        let seq = self.next();
        self.elems.push(StructElem { role, parent, seq });
        self.elems.len() as u32 - 1
    }

    /// The element of `key` (a node met again on a later page keeps its element).
    fn open(&mut self, key: String, role: &StructRole, parent: Option<u32>) -> u32 {
        if let Some(&id) = self.by_key.get(&key) {
            return id;
        }
        let id = self.create(role.clone(), parent);
        self.by_key.insert(key, id);
        id
    }
}

struct Open {
    x: f32,
    w: f32,
    visual: Visual,
    page: usize,
    op: usize,
    top: f32,
    first: bool,
}

/// Cell boxes of the table segment being emitted on the current page. They are painted before the cell
/// contents: cells never overlap, so the picture is the same, but consecutive boxes can then be drawn as
/// one PDF path instead of one graphics block each (the content stream shrinks ~3x).
struct CellLayer {
    page: usize,
    at: usize,
    boxes: Vec<Op>,
}

pub struct Paginator {
    body_h: f32,
    /// Document-wide number of the first page of this run (1-based).
    base_page: u32,
    pub pages: Vec<Vec<Op>>,
    pub warnings: Vec<Warning>,
    cursor: f32,
    open: Vec<Open>,
    after_break: bool,
    layer: Option<CellLayer>,
    /// Number of tables being emitted (nested tables inside cells are not layered).
    table_depth: u32,
    pub structure: Structure,
    /// Open structure elements, innermost last.
    stack: Vec<u32>,
    /// Everything drawn now is an artifact of this kind (headers, footers, repeated table headers…).
    pub artifact: Option<ArtifactKind>,
    /// `Link` element of the last linked run: (parent, link, element). Consecutive runs share it.
    last_link: Option<(Option<u32>, u32, u32)>,
}

/// What [`Paginator::enter`] opened, to close it again.
struct Entered {
    pushed: usize,
    artifact: Option<ArtifactKind>,
}

impl Paginator {
    pub fn new(body_h: f32, base_page: u32) -> Self {
        Paginator {
            body_h,
            base_page,
            pages: vec![Vec::new()],
            warnings: Vec::new(),
            cursor: 0.0,
            open: Vec::new(),
            after_break: false,
            layer: None,
            table_depth: 0,
            structure: Structure::default(),
            stack: Vec::new(),
            artifact: None,
            last_link: None,
        }
    }

    // ------------------------------------------------------------ structure (tagged PDF)

    /// Tag of content drawn now.
    fn tag(&mut self) -> Tag {
        match (self.artifact, self.stack.last().copied()) {
            (Some(k), _) => Tag::Artifact(k),
            (None, Some(elem)) => Tag::Content {
                elem,
                seq: self.structure.next(),
            },
            (None, None) => Tag::Artifact(ArtifactKind::Layout),
        }
    }

    /// Backgrounds and borders are never content.
    fn box_tag(&self) -> Tag {
        Tag::Artifact(self.artifact.unwrap_or(ArtifactKind::Layout))
    }

    fn open_elem(&mut self, key: String, role: &StructRole) {
        let parent = self.stack.last().copied();
        let id = self.structure.open(key, role, parent);
        self.stack.push(id);
    }

    /// Open the structure elements of `n` (its role, and a `Link` around it for a linked box).
    fn enter(&mut self, n: &LNode) -> Entered {
        let artifact = self.artifact;
        if n.meta.artifact && self.artifact.is_none() {
            self.artifact = Some(ArtifactKind::Layout);
        }
        self.last_link = None;
        if self.artifact.is_some() {
            return Entered {
                pushed: 0,
                artifact,
            };
        }
        let mut pushed = 0;
        let link = n.meta.link.is_some();
        // a linked image: Link > Figure (the figure's alt names the link); a linked box: role > Link
        let image = matches!(n.kind, LKind::Image { .. });
        if link && image {
            self.open_elem(format!("{}#link", n.path), &StructRole::Link);
            pushed += 1;
        }
        if let Some(role) = &n.meta.role {
            self.open_elem(n.path.clone(), role);
            pushed += 1;
        }
        if link && !image {
            self.open_elem(format!("{}#link", n.path), &StructRole::Link);
            pushed += 1;
        }
        Entered { pushed, artifact }
    }

    fn leave(&mut self, e: Entered) {
        for _ in 0..e.pushed {
            self.stack.pop();
        }
        self.artifact = e.artifact;
        self.last_link = None;
    }

    /// Document-wide number of the current page.
    fn page_no(&self) -> u32 {
        self.base_page + self.pages.len() as u32 - 1
    }

    fn push(&mut self, op: DrawOp) {
        let tag = if matches!(op, DrawOp::Box { .. }) {
            self.box_tag()
        } else {
            self.tag()
        };
        self.pages.last_mut().unwrap().push(Op::Draw(op, tag));
    }

    fn push_op(&mut self, op: Op) {
        self.pages.last_mut().unwrap().push(op);
    }

    /// A text run; a linked run also gets its clickable area (the line box over the run).
    fn push_run(&mut self, run: &Run, x: f32, line_top: f32, baseline: f32, line_h: f32) {
        let mut r = run.clone();
        r.x = 0.0;
        let (tag, elem) = match (r.link, self.artifact) {
            (Some(link), None) => {
                let parent = self.stack.last().copied();
                let elem = match self.last_link {
                    Some((p, l, e)) if p == parent && l == link => e,
                    _ => {
                        let e = self.structure.create(StructRole::Link, parent);
                        self.last_link = Some((parent, link, e));
                        e
                    }
                };
                (
                    Tag::Content {
                        elem,
                        seq: self.structure.next(),
                    },
                    Some(elem),
                )
            }
            _ => {
                self.last_link = None;
                (self.tag(), None)
            }
        };
        if let Some(link) = r.link {
            let w = r.width();
            if w > 0.0 {
                self.push_op(Op::Link(LinkRect {
                    x,
                    y: line_top,
                    w,
                    h: line_h,
                    link,
                    elem,
                }));
            }
        }
        self.push_op(Op::Draw(
            DrawOp::Text {
                x,
                baseline: line_top + baseline,
                run: r,
            },
            tag,
        ));
    }

    /// Where a node starts: its anchor, its bookmark, the clickable area of a linked box.
    fn mark(&mut self, n: &LNode, x: f32, y: f32) {
        if let Some(name) = &n.meta.anchor {
            self.push_op(Op::Anchor {
                name: name.clone(),
                x,
                y,
            });
        }
        if let Some((level, title)) = &n.meta.heading {
            self.push_op(Op::Heading {
                level: *level,
                title: title.clone(),
                y,
            });
        }
        if let Some(link) = n.meta.link {
            let h = n.h.min(self.body_h - y).max(0.0);
            if n.w > 0.0 && h > 0.0 {
                let elem = if self.artifact.is_none() {
                    self.structure
                        .by_key
                        .get(&format!("{}#link", n.path))
                        .copied()
                } else {
                    None
                };
                self.push_op(Op::Link(LinkRect {
                    x,
                    y,
                    w: n.w,
                    h,
                    link,
                    elem,
                }));
            }
        }
    }

    /// Draw `n` whole on an earlier page (absolutely positioned children of a box that started there).
    fn emit_on_page(&mut self, page: usize, n: &LNode, x: f32, y: f32) {
        let later = self.pages.split_off(page + 1);
        self.emit_whole(n, x, y);
        self.pages.extend(later);
    }

    fn advance(&mut self, dy: f32) {
        self.cursor += dy;
        self.after_break = false;
    }

    fn moved(&mut self, path: &str, reason: MoveReason) {
        let from = self.page_no();
        self.warnings.push(Warning::MovedToNextPage {
            path: path.to_string(),
            from_page: from,
            to_page: from + 1,
            reason,
        });
    }

    fn close_fragment(&mut self, o: &Open, end_y: f32, last: bool) {
        let h = (end_y - o.top).max(0.0);
        let tag = match &self.pages[o.page][o.op] {
            Op::Draw(_, tag) => *tag,
            _ => Tag::Artifact(ArtifactKind::Layout),
        };
        self.pages[o.page][o.op] = Op::Draw(
            DrawOp::Box {
                x: o.x,
                y: o.top,
                w: o.w,
                h,
                visual: o.visual,
                sides: [o.first, true, last, true],
            },
            tag,
        );
    }

    /// Start a new physical page; fragments of open boxes are closed here and reopened there.
    fn new_page(&mut self) {
        let opens = std::mem::take(&mut self.open);
        for o in &opens {
            self.close_fragment(o, self.body_h, false);
        }
        self.pages.push(Vec::new());
        self.cursor = 0.0;
        self.after_break = true;
        let page = self.pages.len() - 1;
        for o in opens {
            let op = self.pages[page].len();
            let tag = self.box_tag();
            self.pages[page].push(Op::Draw(
                DrawOp::Box {
                    x: o.x,
                    y: 0.0,
                    w: o.w,
                    h: 0.0,
                    visual: o.visual,
                    sides: [false; 4],
                },
                tag,
            ));
            self.open.push(Open {
                page,
                op,
                top: 0.0,
                first: false,
                ..o
            });
        }
    }

    fn open_fragment(&mut self, n: &LNode, x: f32, y: f32) -> bool {
        let Some(visual) = n.visual else { return false };
        let page = self.pages.len() - 1;
        let op = self.pages[page].len();
        let tag = self.box_tag();
        self.pages[page].push(Op::Draw(
            DrawOp::Box {
                x,
                y,
                w: n.w,
                h: 0.0,
                visual,
                sides: [true; 4],
            },
            tag,
        ));
        self.open.push(Open {
            x,
            w: n.w,
            visual,
            page,
            op,
            top: y,
            first: true,
        });
        true
    }

    fn close_last(&mut self, opened: bool) {
        if opened {
            let o = self.open.pop().expect("an open fragment");
            self.close_fragment(&o, self.cursor, true);
        }
    }

    // ------------------------------------------------------------ cell layering

    fn begin_table(&mut self) {
        if self.table_depth == 0 {
            let page = self.pages.len() - 1;
            self.layer = Some(CellLayer {
                page,
                at: self.pages[page].len(),
                boxes: Vec::new(),
            });
        }
        self.table_depth += 1;
    }

    fn flush_layer(&mut self) {
        if let Some(l) = &mut self.layer {
            let boxes = std::mem::take(&mut l.boxes);
            let (page, at) = (l.page, l.at);
            self.pages[page].splice(at..at, boxes);
        }
    }

    fn end_table(&mut self) {
        self.table_depth -= 1;
        if self.table_depth == 0 {
            self.flush_layer();
            self.layer = None;
        }
    }

    /// Page break in the middle of a top-level table: paint this page's cell boxes, then layer the next page.
    fn table_new_page(&mut self) {
        self.flush_layer();
        self.new_page();
        if let Some(l) = &mut self.layer {
            l.page = self.pages.len() - 1;
            l.at = self.pages[l.page].len();
        }
    }

    // ------------------------------------------------------------ emission (no splitting)

    fn emit_row(&mut self, content_x: f32, y: f32, row: &LRow) {
        let tr = self.artifact.is_none();
        if tr {
            self.open_elem(format!("{}#tr", row.path), &StructRole::Tr);
        }
        self.emit_cells(content_x, y, row);
        if tr {
            self.stack.pop();
        }
    }

    fn emit_cells(&mut self, content_x: f32, y: f32, row: &LRow) {
        let layered = self.table_depth == 1 && self.layer.is_some();
        for cell in &row.cells {
            let cx = content_x + cell.x;
            if layered && let Some(v) = cell.visual {
                let op = Op::Draw(
                    DrawOp::Box {
                        x: cx,
                        y,
                        w: cell.w,
                        h: cell.h,
                        visual: v,
                        sides: [true; 4],
                    },
                    self.box_tag(),
                );
                self.layer.as_mut().unwrap().boxes.push(op);
            }
            self.emit_node(cell, cx, y, !layered);
        }
    }

    pub fn emit_whole(&mut self, n: &LNode, x: f32, y: f32) {
        self.emit_node(n, x, y, true);
    }

    fn emit_node(&mut self, n: &LNode, x: f32, y: f32, own_box: bool) {
        let entered = self.enter(n);
        self.emit_node_inner(n, x, y, own_box);
        self.leave(entered);
    }

    fn emit_node_inner(&mut self, n: &LNode, x: f32, y: f32, own_box: bool) {
        self.mark(n, x, y);
        if n.meta.missing_alt {
            let page = self.page_no();
            self.warnings.push(Warning::MissingAlt {
                path: n.path.clone(),
                page,
            });
        }
        if let Some(v) = n.visual.filter(|_| own_box) {
            self.push(DrawOp::Box {
                x,
                y,
                w: n.w,
                h: n.h,
                visual: v,
                sides: [true; 4],
            });
        }
        if n.overflow_dx > 0.0 {
            let page = self.page_no();
            self.warnings.push(Warning::Overflow {
                path: n.path.clone(),
                page,
                dx: n.overflow_dx,
                dy: 0.0,
            });
        }
        let (il, it) = (n.inset[3], n.inset[0]);
        match &n.kind {
            LKind::Box { children, .. } => {
                // in-flow children first, absolutely positioned ones above them
                for c in children
                    .iter()
                    .filter(|c| !c.meta.absolute)
                    .chain(children.iter().filter(|c| c.meta.absolute))
                {
                    self.emit_whole(c, x + c.x, y + c.y);
                }
            }
            LKind::Text {
                lines,
                missing,
                dynamic,
            } => {
                let page = self.page_no();
                for (ch, family) in missing {
                    self.warnings.push(Warning::MissingGlyph {
                        path: n.path.clone(),
                        page,
                        character: ch.to_string(),
                        codepoint: *ch as u32,
                        font_family: family.clone(),
                    });
                }
                if let Some(d) = dynamic {
                    let w = (n.w - n.inset[1] - il).max(0.0);
                    let tag = self.tag();
                    self.pages.last_mut().unwrap().push(Op::Dynamic {
                        x: x + il,
                        y: y + it,
                        w,
                        text: d.clone(),
                        tag,
                    });
                } else {
                    for l in lines {
                        for run in &l.line.runs {
                            self.push_run(
                                run,
                                x + il + run.x,
                                y + it + l.y,
                                l.line.baseline,
                                l.line.height,
                            );
                        }
                    }
                }
            }
            LKind::Image { image, fit } => {
                if let Some(i) = image {
                    self.push(DrawOp::Image {
                        x: x + il,
                        y: y + it,
                        w: (n.w - il - n.inset[1]).max(0.0),
                        h: (n.h - it - n.inset[2]).max(0.0),
                        image: *i,
                        fit: *fit,
                    });
                }
            }
            LKind::Table(t) => {
                self.begin_table();
                for r in &t.header {
                    self.emit_row(x + il, y + it + r.y, r);
                }
                for r in &t.rows {
                    self.emit_row(x + il, y + it + t.header_h + r.y, r);
                }
                self.end_table();
            }
            LKind::PageBreak => {}
        }
    }

    // ------------------------------------------------------------ flow

    fn first_unit(&self, n: &LNode) -> f32 {
        match &n.kind {
            LKind::Text { lines, .. } if lines.len() >= 4 && !n.keep_together => {
                n.inset[0] + lines.iter().take(2).map(|l| l.line.height).sum::<f32>()
            }
            LKind::Table(t) if !n.keep_together => {
                n.inset[0] + t.header_h + t.rows.first().map_or(0.0, |r| r.h)
            }
            LKind::Box {
                children,
                splittable: true,
            } if !n.keep_together => {
                n.inset[0] + children.first().map_or(0.0, |c| self.first_unit(c))
            }
            _ => n.h,
        }
    }

    /// Place the children of a column container (or of the section body).
    pub fn place_children(&mut self, children: &[LNode], x: f32, inset_top: f32) {
        // absolutely positioned children: relative to the box, on the page where it starts
        let (start_page, origin_y) = (self.pages.len() - 1, self.cursor - inset_top);
        let flow: Vec<&LNode> = children.iter().filter(|c| !c.meta.absolute).collect();
        let mut prev_bottom = inset_top;
        for (i, c) in flow.iter().copied().enumerate() {
            let mut lead = (c.y - prev_bottom).max(0.0);
            if c.keep_with_next
                && let Some(next) = flow.get(i + 1).copied()
            {
                let lead_next = (next.y - (c.y + c.h)).max(0.0);
                let block = c.h + lead_next + self.first_unit(next);
                let lead_now = if self.after_break { 0.0 } else { lead };
                if self.cursor > 0.0
                    && self.cursor + lead_now + block > self.body_h + EPS
                    && block <= self.body_h + EPS
                {
                    if c.keep_with_next_asked {
                        self.moved(&c.path, MoveReason::KeepWithNext);
                    }
                    self.new_page();
                    lead = 0.0;
                }
            }
            self.place(c, x + c.x, lead);
            prev_bottom = c.y + c.h;
        }
        for c in children.iter().filter(|c| c.meta.absolute) {
            self.emit_on_page(start_page, c, x + c.x, origin_y + c.y);
        }
    }

    fn place(&mut self, n: &LNode, x: f32, lead: f32) {
        if matches!(n.kind, LKind::PageBreak) {
            if self.cursor > 0.0 {
                self.new_page();
                self.after_break = false; // an explicit break keeps the next margin, like a new section
            }
            return;
        }
        let lead = if self.after_break { 0.0 } else { lead };
        let remaining = self.body_h - self.cursor;
        let need = lead + n.h;
        if need <= remaining + EPS {
            self.emit_whole(n, x, self.cursor + lead);
            self.advance(need);
            return;
        }

        let fits_a_page = n.h <= self.body_h + EPS;
        let short_text = matches!(&n.kind, LKind::Text { lines, .. } if lines.len() < 4);
        let rigid = matches!(
            &n.kind,
            LKind::Image { .. }
                | LKind::Box {
                    splittable: false,
                    ..
                }
        ) || short_text;
        let keep = n.keep_together && fits_a_page;

        if rigid || keep {
            if self.cursor > 0.0 && fits_a_page {
                // images and page breaks are atomic by nature; anything else moved whole was asked to be.
                // A short paragraph moving on is ordinary text flow: no warning.
                if n.keep_together && !matches!(n.kind, LKind::Image { .. } | LKind::PageBreak) {
                    self.moved(&n.path, MoveReason::KeepTogether);
                } else if !short_text {
                    self.moved(&n.path, MoveReason::Atomic);
                }
                self.new_page();
                self.emit_whole(n, x, 0.0);
                self.advance(n.h);
            } else {
                // taller than a page: top of a fresh page, report the overflow
                if self.cursor > 0.0 {
                    self.new_page();
                }
                let page = self.page_no();
                self.warnings.push(Warning::Overflow {
                    path: n.path.clone(),
                    page,
                    dx: 0.0,
                    dy: (n.h - self.body_h).max(0.0),
                });
                self.emit_whole(n, x, 0.0);
                self.advance(n.h.min(self.body_h));
            }
            return;
        }

        let entered = self.enter(n);
        match &n.kind {
            LKind::Text { .. } => self.split_text(n, x, lead),
            LKind::Box { .. } => self.split_box(n, x, lead),
            LKind::Table(t) => self.split_table(n, t, x, lead),
            _ => unreachable!(),
        }
        self.leave(entered);
    }

    // ------------------------------------------------------------ splitting

    fn split_box(&mut self, n: &LNode, x: f32, lead: f32) {
        let LKind::Box { children, .. } = &n.kind else {
            unreachable!()
        };
        let mut lead = lead;
        if self.cursor + lead >= self.body_h - EPS {
            self.new_page();
            lead = 0.0;
        }
        let y0 = self.cursor + lead;
        self.mark(n, x, y0);
        let opened = self.open_fragment(n, x, y0);
        self.cursor = y0 + n.inset[0];
        self.after_break = false;
        self.place_children(children, x, n.inset[0]);
        let last_bottom = children.last().map_or(n.inset[0], |c| c.y + c.h);
        let trailing = (n.h - n.inset[2] - last_bottom).max(0.0);
        self.advance(trailing + n.inset[2]);
        self.close_last(opened);
    }

    fn split_text(&mut self, n: &LNode, x: f32, lead: f32) {
        let LKind::Text { lines, missing, .. } = &n.kind else {
            unreachable!()
        };
        let total = lines.len();
        let mut lead = lead;
        let mut i = 0;
        let mut first = true;
        loop {
            let top_inset = if first { n.inset[0] } else { 0.0 };
            let avail = self.body_h - self.cursor - lead - top_inset;
            let mut k = 0;
            let mut sum = 0.0;
            while i + k < total && sum + lines[i + k].line.height <= avail + EPS {
                sum += lines[i + k].line.height;
                k += 1;
            }
            let mut rest = total - i - k;
            if rest == 0 && sum + n.inset[2] > avail + EPS && k > 1 {
                // the bottom padding does not fit: leave the last line for the next page
                k -= 1;
                sum -= lines[i + k].line.height;
                rest = 1;
            }
            if rest == 1 && k > 2 {
                // widow: bring one more line along
                k -= 1;
                sum -= lines[i + k].line.height;
                rest = 2;
            }
            let need_two = first && k < 2;
            if (need_two || k == 0) && self.cursor > 0.0 {
                // orphan control (at least two lines at the bottom of a page) is ordinary text flow: no warning
                self.new_page();
                lead = 0.0;
                continue;
            }
            if k == 0 {
                k = 1; // a page too small for even one line: place it anyway
                sum = lines[i].line.height;
                rest = total - i - 1;
            }

            // draw this fragment
            let y0 = self.cursor + lead;
            let last = rest == 0;
            let h = top_inset + sum + if last { n.inset[2] } else { 0.0 };
            if let Some(v) = n.visual {
                self.push(DrawOp::Box {
                    x,
                    y: y0,
                    w: n.w,
                    h,
                    visual: v,
                    sides: [first, true, last, true],
                });
            }
            if first {
                self.mark(n, x, y0);
                let page = self.page_no();
                for (ch, family) in missing {
                    self.warnings.push(Warning::MissingGlyph {
                        path: n.path.clone(),
                        page,
                        character: ch.to_string(),
                        codepoint: *ch as u32,
                        font_family: family.clone(),
                    });
                }
                if n.overflow_dx > 0.0 {
                    self.warnings.push(Warning::Overflow {
                        path: n.path.clone(),
                        page,
                        dx: n.overflow_dx,
                        dy: 0.0,
                    });
                }
            }
            let mut ly = y0 + top_inset;
            for l in &lines[i..i + k] {
                for run in &l.line.runs {
                    self.push_run(
                        run,
                        x + n.inset[3] + run.x,
                        ly,
                        l.line.baseline,
                        l.line.height,
                    );
                }
                ly += l.line.height;
            }
            self.advance(lead + h);
            i += k;
            first = false;
            lead = 0.0;
            if last {
                break;
            }
            self.new_page();
        }
    }

    fn split_table(&mut self, n: &LNode, t: &LTable, x: f32, lead: f32) {
        let mut lead = lead;
        let content_x = x + n.inset[3];
        let first_unit = t.header_h + t.rows.first().map_or(0.0, |r| r.h);
        if self.cursor > 0.0
            && self.cursor + lead + n.inset[0] + first_unit > self.body_h + EPS
            && first_unit + n.inset[0] <= self.body_h
        {
            // header + first row moving on is ordinary table flow: no warning
            self.new_page();
            lead = 0.0;
        }
        let lead = if self.after_break { 0.0 } else { lead };
        let y0 = self.cursor + lead;
        self.mark(n, x, y0);
        let page = self.page_no();
        if n.overflow_dx > 0.0 {
            self.warnings.push(Warning::Overflow {
                path: n.path.clone(),
                page,
                dx: n.overflow_dx,
                dy: 0.0,
            });
        }
        let opened = self.open_fragment(n, x, y0);
        self.cursor = y0 + n.inset[0];
        self.after_break = false;
        self.begin_table();

        for r in &t.header {
            self.emit_row(content_x, self.cursor + r.y, r);
        }
        self.cursor += t.header_h;

        for row in &t.rows {
            if row.h > self.body_h - self.cursor + EPS {
                if self.cursor <= EPS {
                    // a row taller than a page, alone at the top
                    let page = self.page_no();
                    self.warnings.push(Warning::Overflow {
                        path: n.path.clone(),
                        page,
                        dx: 0.0,
                        dy: row.h - self.body_h,
                    });
                } else {
                    self.table_new_page();
                    if t.repeat_header && t.header_h > 0.0 {
                        // a repeated header is read once: the copies are artifacts
                        let artifact = self.artifact.replace(ArtifactKind::Pagination);
                        for r in &t.header {
                            self.emit_row(content_x, self.cursor + r.y, r);
                        }
                        self.artifact = artifact;
                        self.cursor += t.header_h;
                    }
                    if row.h > self.body_h - self.cursor + EPS {
                        let page = self.page_no();
                        self.warnings.push(Warning::Overflow {
                            path: n.path.clone(),
                            page,
                            dx: 0.0,
                            dy: row.h + self.cursor - self.body_h,
                        });
                    }
                }
            }
            self.emit_row(content_x, self.cursor, row);
            self.advance(row.h);
        }
        self.end_table();
        self.advance(n.inset[2]);
        self.close_last(opened);
    }
}
