//! Tables: column widths, rows (never split), the fast path for plain text cells.

use super::*;

pub(super) fn column_widths(cols: &[t::Column], total: f32) -> Vec<f32> {
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
pub(super) fn layout_rows(
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
pub(super) fn fast_text_cell(
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
