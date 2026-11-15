//! Tables across pages: cells drawn in layers, rows never split, the header repeated on every page.

use super::*;

impl Paginator {
    // ------------------------------------------------------------ cell layering

    pub(super) fn begin_table(&mut self) {
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

    pub(super) fn flush_layer(&mut self) {
        if let Some(l) = &mut self.layer {
            let boxes = std::mem::take(&mut l.boxes);
            let (page, at) = (l.page, l.at);
            self.pages[page].splice(at..at, boxes);
        }
    }

    pub(super) fn end_table(&mut self) {
        self.table_depth -= 1;
        if self.table_depth == 0 {
            self.flush_layer();
            self.layer = None;
        }
    }

    /// Page break in the middle of a top-level table: paint this page's cell boxes, then layer the next page.
    pub(super) fn table_new_page(&mut self) {
        self.flush_layer();
        self.new_page();
        if let Some(l) = &mut self.layer {
            l.page = self.pages.len() - 1;
            l.at = self.pages[l.page].len();
        }
    }

    // ------------------------------------------------------------ emission (no splitting)

    pub(super) fn emit_row(&mut self, content_x: f32, y: f32, row: &LRow) {
        let tr = self.artifact.is_none();
        if tr {
            self.open_elem(format!("{}#tr", row.path), &StructRole::Tr);
        }
        self.emit_cells(content_x, y, row);
        if tr {
            self.stack.pop();
        }
    }

    pub(super) fn emit_cells(&mut self, content_x: f32, y: f32, row: &LRow) {
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

    pub(super) fn split_table(&mut self, n: &LNode, t: &LTable, x: f32, lead: f32) {
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
