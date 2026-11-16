//! CSS floats, for a layout engine built on flexbox and grid. A run of floated siblings becomes a row:
//! left floats at the start, right floats at the end (the first one rightmost, as in CSS), and the
//! content that follows them, up to the next `clear`, in a column beside them. What differs from a
//! browser: text flows beside a float but does not wrap under it once it is past its bottom.

use super::{Converter, view};
use crate::dom::{Data, Id};
use turbopdf_tree::{AlignItems, Dimension, FlexDirection, FlexWrap, LayoutMode, Node, Style};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Float {
    Left,
    Right,
}

impl Converter<'_> {
    pub(super) fn float_of(&self, id: Id) -> Option<Float> {
        self.el(id)?;
        match self.computed.prop(id, &["float"]).as_deref() {
            Some("left" | "inline-start") => Some(Float::Left),
            Some("right" | "inline-end") => Some(Float::Right),
            _ => None,
        }
    }

    fn clears(&self, id: Id) -> bool {
        self.el(id).is_some()
            && matches!(
                self.computed.prop(id, &["clear"]).as_deref(),
                Some("left" | "right" | "both" | "inline-start" | "inline-end")
            )
    }

    fn is_blank_text(&self, id: Id) -> bool {
        matches!(&self.dom.nodes[id].data, Data::Text(t) if t.trim().is_empty())
    }

    /// `children[at]` is a float: convert it, its floated siblings and the content beside them into one
    /// row. Returns the row and the index of the first child after it.
    pub(super) fn float_row(&mut self, children: &[Id], at: usize) -> (Node, usize) {
        let mut i = at;
        let (mut left, mut right) = (Vec::new(), Vec::new());
        // the run of floats (blank text between them does not end it)
        while i < children.len() {
            let c = children[i];
            if self.is_blank_text(c) || !self.is_content(c) {
                i += 1;
                continue;
            }
            match self.float_of(c) {
                Some(side) if i == at || !self.clears(c) => {
                    let mut nodes = self.block(c);
                    let node = if nodes.len() == 1 {
                        nodes.pop().unwrap()
                    } else {
                        view(nodes, None, None)
                    };
                    if side == Float::Left {
                        left.push(node);
                    } else {
                        // the first right float is the rightmost
                        right.insert(0, node);
                    }
                    i += 1;
                }
                _ => break,
            }
        }
        // the content beside the floats: up to a `clear` or the next float
        let start = i;
        while i < children.len()
            && !self.clears(children[i])
            && self.float_of(children[i]).is_none()
        {
            i += 1;
        }
        let beside = self.blocks(&children[start..i], false);

        let mut items = left;
        let wrap = beside.is_empty();
        if !beside.is_empty() {
            let column = Style {
                flex_grow: Some(1.0),
                flex_shrink: Some(1.0),
                flex_basis: Some(Dimension::Points(0.0)),
                min_width: Some(Dimension::Points(0.0)),
                ..Style::default()
            };
            items.push(view(beside, Some(column), None));
        } else if !right.is_empty() {
            // nothing beside: push the right floats to the end
            let spacer = Style {
                flex_grow: Some(1.0),
                ..Style::default()
            };
            items.push(view(Vec::new(), Some(spacer), None));
        }
        items.extend(right);
        let row = Style {
            display: Some(LayoutMode::Flex),
            flex_direction: Some(FlexDirection::Row),
            flex_wrap: wrap.then_some(FlexWrap::Wrap),
            align_items: Some(AlignItems::FlexStart),
            ..Style::default()
        };
        (view(items, Some(row), None), i)
    }
}
