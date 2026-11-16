//! Which CSS properties the converter reads, which only matter on a screen (ignored silently), and the rest
//! (ignored, with an `unsupported_css` warning so the author knows why the PDF differs from a browser).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Yes,
    /// Screen-only or with no effect on paper: ignored without a warning.
    Irrelevant,
    No,
}

#[rustfmt::skip]
const SUPPORTED: &[&str] = &[
    "display", "visibility", "float", "clear", "position", "top", "right", "bottom", "left", "inset",
    "width", "height", "min-width", "max-width", "min-height", "max-height", "aspect-ratio",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "border", "border-width", "border-style", "border-color", "border-radius",
    "border-top", "border-right", "border-bottom", "border-left",
    "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
    "border-top-style", "border-right-style", "border-bottom-style", "border-left-style",
    "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
    "background", "background-color", "color",
    "font-family", "font-size", "font-weight", "font-style", "line-height", "text-align",
    "flex", "flex-grow", "flex-shrink", "flex-basis", "flex-direction", "flex-wrap",
    "align-items", "align-self", "align-content", "justify-content", "gap", "row-gap", "column-gap",
    "grid-template-columns", "grid-template-rows",
    "break-before", "break-after", "break-inside",
    "page-break-before", "page-break-after", "page-break-inside",
    "content", "text-decoration", "text-decoration-line", "letter-spacing", "vertical-align",
    "font-variant-numeric", "text-indent", "white-space", "text-transform",
];

/// Interaction, animation, rendering hints: nothing to draw on paper.
#[rustfmt::skip]
const IRRELEVANT: &[&str] = &[
    "cursor", "pointer-events", "user-select", "touch-action", "resize", "caret-color", "accent-color",
    "appearance", "will-change", "contain", "content-visibility", "isolation", "backface-visibility",
    "color-scheme", "print-color-adjust", "color-adjust", "forced-color-adjust", "font-display",
    "font-smooth", "text-rendering", "image-rendering", "speak", "box-sizing", "tab-size",
    "outline", "outline-width", "outline-style", "outline-color", "outline-offset",
    "text-size-adjust",
];

#[rustfmt::skip]
const IRRELEVANT_PREFIXES: &[&str] = &[
    "-webkit-", "-moz-", "-ms-", "-o-", "transition", "animation", "scroll-", "overscroll-",
];

/// Is `property` (lowercase) read by the converter?
pub fn support(property: &str) -> Support {
    if property.starts_with("--") || SUPPORTED.contains(&property) {
        Support::Yes
    } else if IRRELEVANT.contains(&property)
        || IRRELEVANT_PREFIXES.iter().any(|p| property.starts_with(p))
    {
        Support::Irrelevant
    } else {
        Support::No
    }
}
