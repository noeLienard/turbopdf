//! Leaf measurement for taffy: text paragraphs (cached per width), intrinsic sizes, tables.

use super::*;

impl State<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn text_leaf(
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
            indent: inh2.indent,
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

    pub(super) fn paragraph(&mut self, idx: usize, width: f32) -> &text::Paragraph {
        let key = (idx, width.max(0.0).to_bits());
        if !self.paragraphs.contains_key(&key) {
            let Item::Text(item) = &self.items[idx] else {
                unreachable!()
            };
            let mut opts = text::ParagraphOptions::new(width.max(0.0));
            opts.align = item.align;
            opts.indent = item.indent;
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

    pub(super) fn intrinsic(&self, idx: usize) -> (f32, f32) {
        let Item::Text(item) = &self.items[idx] else {
            unreachable!()
        };
        item.prepared
            .as_ref()
            .map_or((0.0, 0.0), |p| p.intrinsic_widths())
    }

    pub(super) fn table(&mut self, idx: usize, width: f32) -> &(LTable, f32) {
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
    pub(super) fn measure(
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
}
