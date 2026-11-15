//! Taffy layout → positioned `LNode`s: boxes, text lines, tables, absolute children.

use super::*;

impl State<'_, '_> {
    pub(super) fn finish(&mut self, tree: &TaffyTree<usize>, sk: &Skel, parent_w: f32) -> LNode {
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
