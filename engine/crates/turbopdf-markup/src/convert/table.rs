//! HTML tables and `<Table>` components → table nodes.

use super::*;

impl Converter<'_> {
    pub(super) fn cell(&mut self, id: Id, header: bool) -> Cell {
        let mut style = self.style(id);
        // HTML defaults for <th>: bold, centred
        if header {
            style = merge(
                &Style {
                    font_weight: Some(700),
                    text_align: Some(TextAlign::Center),
                    ..Default::default()
                },
                &style,
            );
        }
        let span = self
            .el(id)
            .and_then(|e| e.attr("colspan"))
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1);
        let children = self.blocks(self.dom.children(id), false);
        Cell {
            style: some_style(style),
            col_span: (span > 1).then_some(span),
            children,
        }
    }

    pub(super) fn row(&mut self, tr: Id, header: bool) -> Row {
        let cells: Vec<Id> = self
            .dom
            .element_children(tr)
            .filter(|&c| matches!(self.tag(c), "td" | "th"))
            .collect();
        let cells = cells
            .into_iter()
            .map(|c| {
                let th = self.tag(c) == "th";
                self.cell(c, header || th)
            })
            .collect();
        Row {
            id: None,
            style: some_style(self.style(tr)),
            cells,
        }
    }

    pub(super) fn html_table(&mut self, id: Id) -> Node {
        let style = self.style(id);
        let mut header = Vec::new();
        let mut rows = Vec::new();
        fn visit(
            me: &mut Converter<'_>,
            parent: Id,
            head: bool,
            header: &mut Vec<Row>,
            rows: &mut Vec<Row>,
        ) {
            let kids: Vec<Id> = me.dom.element_children(parent).collect();
            for c in kids {
                match me.tag(c) {
                    "thead" => visit(me, c, true, header, rows),
                    "tbody" | "tfoot" => visit(me, c, false, header, rows),
                    "tr" => {
                        let r = me.row(c, head);
                        if head { header.push(r) } else { rows.push(r) }
                    }
                    _ => {}
                }
            }
        }
        visit(self, id, false, &mut header, &mut rows);
        // no <thead>: a first row made only of <th> is the header
        let first = self
            .dom
            .element_children(id)
            .find(|&c| matches!(self.tag(c), "tr" | "tbody"));
        let first_tr = match first {
            Some(t) if self.tag(t) == "tbody" => {
                self.dom.element_children(t).find(|&c| self.tag(c) == "tr")
            }
            other => other,
        };
        if header.is_empty()
            && rows.len() > 1
            && first_tr.is_some_and(|tr| self.dom.element_children(tr).all(|c| self.tag(c) == "th"))
        {
            header.push(rows.remove(0));
        }
        let span = |r: &Row| r.cells.iter().map(|c| c.col_span.unwrap_or(1)).sum::<u32>() as usize;
        let count = header
            .iter()
            .chain(&rows)
            .map(span)
            .max()
            .unwrap_or(0)
            .max(1);
        // widths: from the first row whose cells all span one column
        let sample = header.iter().chain(&rows).find(|r| r.cells.len() == count);
        let columns = (0..count)
            .map(|i| {
                let s = sample.and_then(|r| r.cells[i].style.as_ref());
                Column {
                    width: Some(s.and_then(|s| s.width).unwrap_or(Dimension::Auto)),
                    align: s.and_then(|s| s.text_align),
                }
            })
            .collect();
        // pad rows that cover fewer columns than the table
        for r in header.iter_mut().chain(rows.iter_mut()) {
            for _ in span(r)..count {
                r.cells.push(Cell {
                    style: None,
                    col_span: None,
                    children: vec![],
                });
            }
        }
        let table_style = Style {
            width: None,
            ..style
        };
        Node::Table(Table {
            id: None,
            style: some_style(table_style),
            columns,
            header: (!header.is_empty()).then_some(header),
            rows,
            repeat_header: None,
            header_style: None,
            cell_style: Some(Style {
                padding: Some(Edges::Sides(EdgeSides {
                    top: Some(2.0),
                    right: Some(3.0),
                    bottom: Some(2.0),
                    left: Some(3.0),
                })),
                ..Default::default()
            }),
            alt_row_style: None,
            keep_together: None,
        })
    }

    pub(super) fn component_table(&mut self, id: Id) -> Vec<Node> {
        let mut p = self.props(id);
        let mut header = Vec::new();
        let mut rows = Vec::new();
        let row_ids: Vec<Id> = self
            .dom
            .element_children(id)
            .filter(|&r| self.tag(r) == "turbopdf-row")
            .collect();
        for r in row_ids {
            let mut rp = self.props(r);
            let is_header = rp.remove("header").is_some_and(|v| v == Value::Bool(true));
            let cell_ids: Vec<Id> = self
                .dom
                .element_children(r)
                .filter(|&c| self.tag(c) == "turbopdf-cell")
                .collect();
            let mut cells = Vec::new();
            for c in cell_ids {
                let mut cp = self.props(c);
                let kids = self.blocks(self.dom.children(c), false);
                cp.insert(
                    "children".into(),
                    serde_json::to_value(kids).expect("nodes serialize"),
                );
                cells.push(Value::Object(cp));
            }
            rp.insert("cells".into(), Value::Array(cells));
            if is_header {
                header.push(Value::Object(rp));
            } else {
                rows.push(Value::Object(rp));
            }
        }
        p.entry("columns").or_insert_with(|| json!([]));
        let mut extra = json!({ "rows": rows });
        if !header.is_empty() {
            extra["header"] = Value::Array(header);
        }
        self.component(p, "table", extra)
    }
}
