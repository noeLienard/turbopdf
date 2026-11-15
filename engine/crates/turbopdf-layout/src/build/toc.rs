//! Tables of contents, built as ordinary nodes from the headings found.

use super::*;

/// The rows of a table of contents, as ordinary nodes.
pub(super) fn toc_view(toc: &t::TableOfContents, entries: &[TocEntry]) -> t::Node {
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
