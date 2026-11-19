//! Structural validation. The engine calls [`validate`] before rendering and refuses invalid trees,
//! with errors that say **where** (JSON Pointer) and **what to do**.

use crate::TREE_VERSION;
use crate::node::*;
use crate::style::{Style, TextStyle};
use crate::value::{Dimension, Edges};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {message} [{code}]")]
pub struct ValidationError {
    /// JSON Pointer to the offending value.
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

struct Ctx {
    errors: Vec<ValidationError>,
    ids: BTreeSet<String>,
}

impl Ctx {
    fn err(&mut self, path: &str, code: &'static str, message: impl Into<String>) {
        self.errors.push(ValidationError {
            path: path.to_string(),
            code,
            message: message.into(),
        });
    }

    fn id(&mut self, path: &str, id: &Option<String>) {
        if let Some(id) = id {
            if id.is_empty() {
                self.err(path, "empty_id", "`id` must not be empty");
            } else if !self.ids.insert(id.clone()) {
                self.err(
                    path,
                    "duplicate_id",
                    format!("`id` {id:?} is used more than once; ids must be unique"),
                );
            }
        }
    }

    fn non_negative(&mut self, path: &str, what: &str, v: f32) {
        if !v.is_finite() || v < 0.0 {
            self.err(
                path,
                "invalid_number",
                format!("{what} must be a finite number >= 0 (got {v})"),
            );
        }
    }

    fn positive(&mut self, path: &str, what: &str, v: f32) {
        if !v.is_finite() || v <= 0.0 {
            self.err(
                path,
                "invalid_number",
                format!("{what} must be a finite number > 0 (got {v})"),
            );
        }
    }

    fn dimension(&mut self, path: &str, what: &str, d: &Option<Dimension>) {
        match d {
            Some(Dimension::Points(v)) | Some(Dimension::Percent(v)) => {
                self.non_negative(path, what, *v)
            }
            _ => {}
        }
    }

    fn edges(&mut self, path: &str, what: &str, e: &Option<Edges>, allow_negative: bool) {
        if let Some(e) = e {
            for v in e.resolve() {
                if !v.is_finite() || (!allow_negative && v < 0.0) {
                    self.err(
                        path,
                        "invalid_number",
                        format!(
                            "{what} must be finite{} (got {v})",
                            if allow_negative { "" } else { " and >= 0" }
                        ),
                    );
                }
            }
        }
    }

    fn font(
        &mut self,
        path: &str,
        size: Option<f32>,
        weight: Option<u16>,
        family: &Option<String>,
    ) {
        if let Some(s) = size {
            self.positive(&format!("{path}/fontSize"), "fontSize", s);
        }
        if let Some(w) = weight.filter(|w| !(100..=900).contains(w)) {
            self.err(
                &format!("{path}/fontWeight"),
                "invalid_font_weight",
                format!("fontWeight must be between 100 and 900 (got {w})"),
            );
        }
        if family.as_ref().is_some_and(|f| f.trim().is_empty()) {
            self.err(
                &format!("{path}/fontFamily"),
                "empty_font_family",
                "fontFamily must not be empty",
            );
        }
    }

    fn style(&mut self, path: &str, s: &Option<Style>) {
        let Some(s) = s else { return };
        let p = |f: &str| format!("{path}/{f}");
        if let Some(v) = s.gap {
            self.non_negative(&p("gap"), "gap", v)
        }
        if let Some(v) = s.row_gap {
            self.non_negative(&p("rowGap"), "rowGap", v)
        }
        if let Some(v) = s.column_gap {
            self.non_negative(&p("columnGap"), "columnGap", v)
        }
        for (name, tracks) in [
            ("gridTemplateColumns", &s.grid_template_columns),
            ("gridTemplateRows", &s.grid_template_rows),
        ] {
            if tracks.as_ref().is_some_and(Vec::is_empty) {
                self.err(
                    &p(name),
                    "empty_grid_template",
                    format!("{name} must contain at least one track"),
                );
            }
        }
        if let Some(v) = s.flex_grow {
            self.non_negative(&p("flexGrow"), "flexGrow", v)
        }
        if let Some(v) = s.flex_shrink {
            self.non_negative(&p("flexShrink"), "flexShrink", v)
        }
        self.dimension(&p("flexBasis"), "flexBasis", &s.flex_basis);
        self.dimension(&p("width"), "width", &s.width);
        self.dimension(&p("height"), "height", &s.height);
        self.dimension(&p("minWidth"), "minWidth", &s.min_width);
        self.dimension(&p("maxWidth"), "maxWidth", &s.max_width);
        self.dimension(&p("minHeight"), "minHeight", &s.min_height);
        self.dimension(&p("maxHeight"), "maxHeight", &s.max_height);
        if let Some(v) = s.aspect_ratio {
            self.positive(&p("aspectRatio"), "aspectRatio", v)
        }
        self.edges(&p("padding"), "padding", &s.padding, false);
        self.edges(&p("margin"), "margin", &s.margin, true);
        for (key, b) in [
            ("border", &s.border),
            ("borderTop", &s.border_top),
            ("borderRight", &s.border_right),
            ("borderBottom", &s.border_bottom),
            ("borderLeft", &s.border_left),
        ] {
            if let Some(b) = b {
                self.non_negative(&p(&format!("{key}/width")), "border width", b.width)
            }
        }
        if let Some(v) = s.border_radius {
            self.non_negative(&p("borderRadius"), "borderRadius", v)
        }
        if let Some(v) = s.line_height {
            self.positive(&p("lineHeight"), "lineHeight", v)
        }
        self.font(path, s.font_size, s.font_weight, &s.font_family);
    }

    fn link(&mut self, path: &str, link: &Option<String>) {
        if let Some(l) = link
            && (l.trim().is_empty() || l.trim() == "#")
        {
            self.err(
                path,
                "empty_link",
                "`link` must be a URL (https:, mailto:…) or `#id` of a node",
            );
        }
    }

    fn text_style(&mut self, path: &str, s: &Option<TextStyle>) {
        if let Some(s) = s {
            self.font(path, s.font_size, s.font_weight, &s.font_family);
        }
    }

    fn content(&mut self, path: &str, c: &TextContent) {
        if let TextContent::Spans(spans) = c {
            for (i, span) in spans.iter().enumerate() {
                self.text_style(&format!("{path}/content/{i}/style"), &span.style);
                self.link(&format!("{path}/content/{i}/link"), &span.link);
            }
        }
    }

    fn nodes(&mut self, path: &str, nodes: &[Node]) {
        for (i, n) in nodes.iter().enumerate() {
            self.node(&format!("{path}/{i}"), n);
        }
    }

    fn node(&mut self, path: &str, node: &Node) {
        match node {
            Node::View(v) => {
                self.id(path, &v.id);
                self.style(&format!("{path}/style"), &v.style);
                self.link(&format!("{path}/link"), &v.link);
                self.nodes(&format!("{path}/children"), &v.children);
            }
            Node::Text(t) => {
                self.id(path, &t.id);
                self.style(&format!("{path}/style"), &t.style);
                self.content(path, &t.content);
            }
            Node::Heading(h) => {
                self.id(path, &h.id);
                if !(1..=6).contains(&h.level) {
                    self.err(
                        &format!("{path}/level"),
                        "invalid_heading_level",
                        format!("heading level must be between 1 and 6 (got {})", h.level),
                    );
                }
                self.style(&format!("{path}/style"), &h.style);
                self.content(path, &h.content);
            }
            Node::Image(img) => {
                self.id(path, &img.id);
                match &img.src {
                    ImageSource::Data { data } if data.is_empty() => self.err(
                        &format!("{path}/src/data"),
                        "empty_source",
                        "image data must not be empty (expected base64)",
                    ),
                    ImageSource::File { file } if file.is_empty() => self.err(
                        &format!("{path}/src/file"),
                        "empty_source",
                        "image file path must not be empty",
                    ),
                    ImageSource::Qr(q) if q.qr.is_empty() => self.err(&format!("{path}/src/qr"), "empty_source", "QR code text must not be empty"),
                    ImageSource::Qr(q) if q.qr.len() > 2953 => self.err(
                        &format!("{path}/src/qr"),
                        "qr_too_long",
                        "QR code text is too long (a QR code holds at most 2953 bytes; use a shorter URL)",
                    ),
                    _ => {}
                }
                self.style(&format!("{path}/style"), &img.style);
                self.link(&format!("{path}/link"), &img.link);
            }
            Node::Table(t) => {
                self.id(path, &t.id);
                self.style(&format!("{path}/style"), &t.style);
                self.style(&format!("{path}/headerStyle"), &t.header_style);
                self.style(&format!("{path}/cellStyle"), &t.cell_style);
                self.style(&format!("{path}/altRowStyle"), &t.alt_row_style);
                if t.columns.is_empty() {
                    self.err(
                        &format!("{path}/columns"),
                        "no_columns",
                        "a table needs at least one column",
                    );
                }
                for (i, c) in t.columns.iter().enumerate() {
                    self.dimension(
                        &format!("{path}/columns/{i}/width"),
                        "column width",
                        &c.width,
                    );
                }
                for (i, r) in t.header.as_deref().unwrap_or_default().iter().enumerate() {
                    self.row(&format!("{path}/header/{i}"), r, t.columns.len());
                }
                for (i, r) in t.rows.iter().enumerate() {
                    self.row(&format!("{path}/rows/{i}"), r, t.columns.len());
                }
            }
            Node::PageBreak(b) => self.id(path, &b.id),
            Node::PageNumber(n) => {
                self.id(path, &n.id);
                self.style(&format!("{path}/style"), &n.style);
                if n.target.as_ref().is_some_and(|t| t.trim().is_empty()) {
                    self.err(
                        &format!("{path}/target"),
                        "empty_target",
                        "`target` must be the `id` of a node",
                    );
                }
            }
            Node::TableOfContents(t) => {
                self.id(path, &t.id);
                self.style(&format!("{path}/style"), &t.style);
                self.style(&format!("{path}/entryStyle"), &t.entry_style);
                if let Some(l) = t.levels.filter(|l| !(1..=6).contains(l)) {
                    self.err(
                        &format!("{path}/levels"),
                        "invalid_levels",
                        format!("levels must be between 1 and 6 (got {l})"),
                    );
                }
            }
        }
    }

    fn row(&mut self, path: &str, row: &Row, columns: usize) {
        self.id(path, &row.id);
        self.style(&format!("{path}/style"), &row.style);
        let mut span_sum = 0usize;
        for (i, cell) in row.cells.iter().enumerate() {
            let cp = format!("{path}/cells/{i}");
            let span = cell.col_span.unwrap_or(1);
            if span == 0 {
                self.err(
                    &format!("{cp}/colSpan"),
                    "invalid_col_span",
                    "colSpan must be >= 1",
                );
            }
            span_sum += span.max(1) as usize;
            self.style(&format!("{cp}/style"), &cell.style);
            self.nodes(&format!("{cp}/children"), &cell.children);
        }
        if span_sum != columns {
            self.err(
                &format!("{path}/cells"),
                "column_mismatch",
                format!("the cells of this row cover {span_sum} column(s) but the table has {columns}; add cells or adjust colSpan"),
            );
        }
    }

    fn band(&mut self, path: &str, nodes: &[Node]) {
        self.nodes(path, nodes);
    }
}

/// Validate a whole tree. Returns **all** problems, not just the first.
pub fn validate(doc: &Document) -> Result<(), Vec<ValidationError>> {
    let mut c = Ctx {
        errors: Vec::new(),
        ids: BTreeSet::new(),
    };

    if doc.version != TREE_VERSION {
        c.err(
            "/version",
            "unsupported_version",
            format!(
                "tree version {} is not supported; this engine reads version {TREE_VERSION}",
                doc.version
            ),
        );
    }
    if doc.pdfua == Some(true) {
        let meta = doc.meta.as_ref();
        if meta
            .and_then(|m| m.title.as_deref())
            .is_none_or(|t| t.trim().is_empty())
        {
            c.err(
                "/meta/title",
                "pdfua_title",
                "PDF/UA needs a document title: set meta.title (HTML: <title>)",
            );
        }
        if meta
            .and_then(|m| m.lang.as_deref())
            .is_none_or(|t| t.trim().is_empty())
        {
            c.err(
                "/meta/lang",
                "pdfua_lang",
                "PDF/UA needs the document language: set meta.lang (HTML: <html lang=\"fr\">)",
            );
        }
        if doc.tagged == Some(false) {
            c.err(
                "/tagged",
                "pdfua_untagged",
                "PDF/UA documents are always tagged: remove tagged: false",
            );
        }
    }
    if let Some(a) = doc.pdfa {
        let date = doc.meta.as_ref().and_then(|m| m.creation_date.as_deref());
        if date.is_none_or(|d| d.trim().is_empty()) {
            c.err(
                "/meta/creationDate",
                "pdfa_date",
                format!(
                    "{} needs a creation date: set meta.creationDate (ISO 8601)",
                    a.name()
                ),
            );
        }
        if a.needs_tagging() && doc.tagged == Some(false) {
            c.err(
                "/tagged",
                "pdfa_untagged",
                format!("{} documents are tagged: remove tagged: false", a.name()),
            );
        }
        if matches!(a, PdfA::A1a | PdfA::A1b) && doc.pdfua == Some(true) {
            c.err(
                "/pdfa",
                "pdfa_pdfua",
                "PDF/A-1 (PDF 1.4) cannot be combined with PDF/UA-1 (PDF 1.7): use PDF/A-2 or later",
            );
        }
    }
    c.style("/defaultStyle", &doc.default_style);
    for (i, f) in doc.fonts.as_deref().unwrap_or_default().iter().enumerate() {
        c.font(
            &format!("/fonts/{i}"),
            None,
            Some(f.weight),
            &Some(f.family.clone()),
        );
    }
    if doc.children.is_empty() {
        c.err(
            "/children",
            "no_pages",
            "a document needs at least one page",
        );
    }
    for (i, page) in doc.children.iter().enumerate() {
        let p = format!("/children/{i}");
        c.id(&p, &page.id);
        c.style(&format!("{p}/style"), &page.style);
        if let Some(PageSize::Custom { width, height }) = page.size {
            c.positive(&format!("{p}/size/width"), "page width", width);
            c.positive(&format!("{p}/size/height"), "page height", height);
        }
        c.edges(&format!("{p}/margin"), "margin", &page.margin, false);
        if let Some(w) = &page.watermark {
            let wp = format!("{p}/watermark");
            if w.text.trim().is_empty() {
                c.err(
                    &format!("{wp}/text"),
                    "empty_watermark",
                    "watermark text must not be empty",
                );
            }
            c.font(&wp, w.font_size, w.font_weight, &w.font_family);
            if let Some(a) = w.angle.filter(|a| !a.is_finite()) {
                c.err(
                    &format!("{wp}/angle"),
                    "invalid_number",
                    format!("angle must be a finite number of degrees (got {a})"),
                );
            }
        }
        c.band(
            &format!("{p}/header"),
            page.header.as_deref().unwrap_or_default(),
        );
        c.band(
            &format!("{p}/footer"),
            page.footer.as_deref().unwrap_or_default(),
        );
        c.nodes(&format!("{p}/children"), &page.children);
    }

    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors)
    }
}
