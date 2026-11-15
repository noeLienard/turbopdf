//! # turbopdf-engine
//!
//! Public facade of the Rust engine: **document tree in → PDF, PNG previews and warnings out**.
//! No browser, no network, no JavaScript. Bindings (`turbopdf-node`, `turbopdf-wasm`) only wrap this.
//!
//! ```no_run
//! # use turbopdf_engine::Engine;
//! # let doc: turbopdf_tree::Document = serde_json::from_str("{}").unwrap();
//! let engine = Engine::new();
//! let out = engine.render(&doc)?;            // PDF + page count + warnings
//! let previews = engine.preview(&doc, 2.0)?; // one PNG per page
//! # Ok::<(), turbopdf_engine::EngineError>(())
//! ```

use base64::Engine as _;
use std::sync::Arc;
use turbopdf_fonts::{FontDb, FontError, NoLoader, ResourceLoader};
use turbopdf_layout::{Layout, LayoutError, layout_document};
use turbopdf_pdf::PdfError;
use turbopdf_raster::RasterError;
use turbopdf_tree::{BinarySource, Document, FontStyle, ValidationError, Warning, validate};

pub use turbopdf_fonts;
pub use turbopdf_html;
pub use turbopdf_layout;
pub use turbopdf_pdf;
pub use turbopdf_tree;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid document tree:\n{}", .0.iter().map(|e| format!("  - {e}")).collect::<Vec<_>>().join("\n"))]
    Invalid(Vec<ValidationError>),
    #[error("font {family:?}: {reason}")]
    Font { family: String, reason: String },
    #[error(transparent)]
    Layout(#[from] LayoutError),
    #[error(transparent)]
    Pdf(#[from] PdfError),
    #[error(transparent)]
    Raster(#[from] RasterError),
}

fn pdf_options(doc: &Document) -> turbopdf_pdf::PdfOptions {
    turbopdf_pdf::PdfOptions {
        tagged: doc.tagged.unwrap_or(true),
        pdfua: doc.pdfua == Some(true),
    }
}

/// A rendered document.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOutput {
    pub pdf: Vec<u8>,
    pub pages: u32,
    pub warnings: Vec<Warning>,
}

pub struct Engine {
    loader: Arc<dyn ResourceLoader>,
    /// Embedded fonts, parsed once for the lifetime of the engine.
    base_fonts: FontDb,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// No filesystem, no network: `file` sources fail (warning for images, error for fonts).
    pub fn new() -> Self {
        Self {
            loader: Arc::new(NoLoader),
            base_fonts: FontDb::with_embedded(),
        }
    }

    /// Let the host resolve `{ "file": … }` sources (used by the Node binding).
    pub fn with_loader(loader: Arc<dyn ResourceLoader>) -> Self {
        Self {
            loader,
            base_fonts: FontDb::with_embedded(),
        }
    }

    fn fonts(&self, doc: &Document) -> Result<FontDb, EngineError> {
        let mut db = self.base_fonts.clone();
        for f in doc.fonts.as_deref().unwrap_or_default() {
            let bytes = match &f.source {
                BinarySource::Data { data } => base64::engine::general_purpose::STANDARD
                    .decode(data.trim())
                    .map_err(|e| EngineError::Font {
                        family: f.family.clone(),
                        reason: format!("invalid base64: {e}"),
                    })?,
                BinarySource::File { file } => {
                    self.loader.load(file).map_err(|reason| EngineError::Font {
                        family: f.family.clone(),
                        reason,
                    })?
                }
            };
            db.add(
                &f.family,
                f.weight,
                f.style == Some(FontStyle::Italic),
                bytes,
            )
            .map_err(|e| match e {
                FontError::Invalid { family } => EngineError::Font {
                    family,
                    reason: "not a valid TrueType/OpenType font".into(),
                },
                FontError::Load { family, reason } => EngineError::Font { family, reason },
            })?;
        }
        Ok(db)
    }

    /// Validate, load fonts and lay out, without writing the PDF: the positioned drawing operations of
    /// every page. Used by tests and debugging tools.
    pub fn layout(&self, doc: &Document) -> Result<(Layout, FontDb), EngineError> {
        validate(doc).map_err(EngineError::Invalid)?;
        let db = self.fonts(doc)?;
        let layout = layout_document(doc, &db, self.loader.as_ref())?;
        Ok((layout, db))
    }

    /// Tree → PDF bytes (+ page count + warnings).
    pub fn render(&self, doc: &Document) -> Result<RenderOutput, EngineError> {
        let (layout, db) = self.layout(doc)?;
        let pdf = turbopdf_pdf::write(&layout, &db, doc.meta.as_ref(), pdf_options(doc))?;
        Ok(RenderOutput {
            pdf,
            pages: layout.pages.len() as u32,
            warnings: layout.warnings,
        })
    }

    /// Tree → one PNG per page.
    pub fn preview(&self, doc: &Document, scale: f32) -> Result<Vec<Vec<u8>>, EngineError> {
        let (layout, db) = self.layout(doc)?;
        Ok(turbopdf_raster::render(&layout, &db, scale)?)
    }

    /// Tree → HTML/CSS preview: real text and fonts, working links, same layout as the PDF.
    pub fn preview_html(&self, doc: &Document) -> Result<turbopdf_html::HtmlPreview, EngineError> {
        let (layout, db) = self.layout(doc)?;
        Ok(turbopdf_html::render(&layout, &db))
    }

    /// PDF and previews from a single layout pass.
    pub fn render_with_previews(
        &self,
        doc: &Document,
        scale: f32,
    ) -> Result<(RenderOutput, Vec<Vec<u8>>), EngineError> {
        let (layout, db) = self.layout(doc)?;
        let pdf = turbopdf_pdf::write(&layout, &db, doc.meta.as_ref(), pdf_options(doc))?;
        let previews = turbopdf_raster::render(&layout, &db, scale)?;
        Ok((
            RenderOutput {
                pdf,
                pages: layout.pages.len() as u32,
                warnings: layout.warnings,
            },
            previews,
        ))
    }
}
