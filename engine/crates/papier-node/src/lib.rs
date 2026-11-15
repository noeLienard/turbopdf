//! Node.js binding: `render(treeJson, options)` and `preview(treeJson, options)`.
//!
//! Work runs on the libuv thread pool (`AsyncTask`), so a render never blocks the event loop.
//! Errors are thrown as `Error` whose message is a JSON object `{ code, message, details? }` that the
//! TypeScript SDK turns into a `PapierError`.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use papier_engine::papier_fonts::ResourceLoader;
use papier_engine::papier_tree::Document;
use papier_engine::{Engine, EngineError};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Reads `{ "file": … }` sources, only inside `base` (no `..` escape, no absolute path elsewhere).
struct DirLoader {
    base: PathBuf,
}

impl ResourceLoader for DirLoader {
    fn load(&self, path: &str) -> std::result::Result<Vec<u8>, String> {
        if path.contains("://") && !path.starts_with("file://") {
            return Err(format!(
                "{path} is remote: Papier never uses the network. Download it and pass a file path or a data: URL"
            ));
        }
        let full = self.base.join(path.trim_start_matches("file://"));
        let real = full
            .canonicalize()
            .map_err(|e| format!("cannot read {path:?}: {e}"))?;
        if !real.starts_with(&self.base) {
            return Err(format!(
                "{path:?} is outside the allowed directory {:?}",
                self.base
            ));
        }
        std::fs::read(&real).map_err(|e| format!("cannot read {path:?}: {e}"))
    }
}

/// Fonts are parsed once per process for the common case (no file access).
fn engine(base_dir: Option<&str>) -> std::result::Result<Arc<Engine>, String> {
    static DEFAULT: OnceLock<Arc<Engine>> = OnceLock::new();
    match base_dir {
        None => Ok(DEFAULT.get_or_init(|| Arc::new(Engine::new())).clone()),
        Some(dir) => {
            let base = Path::new(dir)
                .canonicalize()
                .map_err(|e| format!("baseDir {dir:?}: {e}"))?;
            Ok(Arc::new(Engine::with_loader(Arc::new(DirLoader { base }))))
        }
    }
}

fn fail(code: &str, message: String, details: Option<serde_json::Value>) -> Error {
    let mut v = json!({ "code": code, "message": message });
    if let Some(d) = details {
        v["details"] = d;
    }
    Error::new(Status::GenericFailure, v.to_string())
}

fn engine_error(e: EngineError) -> Error {
    match e {
        EngineError::Invalid(errors) => {
            let details = errors
                .iter()
                .map(|e| json!({ "path": e.path, "code": e.code, "message": e.message }))
                .collect();
            fail(
                "invalid_tree",
                EngineError::Invalid(errors).to_string(),
                Some(serde_json::Value::Array(details)),
            )
        }
        e @ EngineError::Font { .. } => fail("font_failed", e.to_string(), None),
        EngineError::Pdf(papier_engine::papier_pdf::PdfError::Accessibility(problems)) => {
            let message = format!(
                "The PDF cannot be PDF/UA conformant: {}. Fix these, or drop the `pdfua` option.",
                problems.join("; ")
            );
            fail("not_accessible", message, Some(json!(problems)))
        }
        e => fail("render_failed", e.to_string(), None),
    }
}

fn parse(tree: &str) -> Result<Document> {
    serde_json::from_str(tree).map_err(|e| {
        fail(
            "invalid_tree",
            format!("the document tree is not valid JSON for this engine: {e}"),
            None,
        )
    })
}

#[napi(object)]
pub struct RenderOptions {
    /// Directory that `{ "file": … }` sources are read from. Without it, file sources are refused.
    pub base_dir: Option<String>,
}

#[napi(object)]
pub struct PreviewOptions {
    pub base_dir: Option<String>,
    /// Pixels per point (1 = 72 dpi). Default 2.
    pub scale: Option<f64>,
}

#[napi(object)]
pub struct RenderResult {
    pub pdf: Buffer,
    pub pages: u32,
    /// JSON array of warnings (same shape as `Warning` in tree.ts).
    pub warnings: String,
}

pub struct RenderTask {
    tree: String,
    base_dir: Option<String>,
}

impl Task for RenderTask {
    type Output = (Vec<u8>, u32, String);
    type JsValue = RenderResult;

    fn compute(&mut self) -> Result<Self::Output> {
        let doc = parse(&self.tree)?;
        let engine =
            engine(self.base_dir.as_deref()).map_err(|m| fail("invalid_option", m, None))?;
        let out = engine.render(&doc).map_err(engine_error)?;
        let warnings = serde_json::to_string(&out.warnings).expect("warnings serialize");
        Ok((out.pdf, out.pages, warnings))
    }

    fn resolve(
        &mut self,
        _env: Env,
        (pdf, pages, warnings): Self::Output,
    ) -> Result<Self::JsValue> {
        Ok(RenderResult {
            pdf: pdf.into(),
            pages,
            warnings,
        })
    }
}

pub struct PreviewTask {
    tree: String,
    base_dir: Option<String>,
    scale: f32,
}

impl Task for PreviewTask {
    type Output = Vec<Vec<u8>>;
    type JsValue = Vec<Buffer>;

    fn compute(&mut self) -> Result<Self::Output> {
        let doc = parse(&self.tree)?;
        let engine =
            engine(self.base_dir.as_deref()).map_err(|m| fail("invalid_option", m, None))?;
        engine.preview(&doc, self.scale).map_err(engine_error)
    }

    fn resolve(&mut self, _env: Env, pages: Self::Output) -> Result<Self::JsValue> {
        Ok(pages.into_iter().map(Buffer::from).collect())
    }
}

/// Document tree (JSON string) → PDF bytes, page count and warnings.
#[napi]
pub fn render(tree: String, options: Option<RenderOptions>) -> AsyncTask<RenderTask> {
    AsyncTask::new(RenderTask {
        tree,
        base_dir: options.and_then(|o| o.base_dir),
    })
}

/// Document tree (JSON string) → one PNG per page.
#[napi]
pub fn preview(tree: String, options: Option<PreviewOptions>) -> AsyncTask<PreviewTask> {
    let (base_dir, scale) = options.map_or((None, None), |o| (o.base_dir, o.scale));
    AsyncTask::new(PreviewTask {
        tree,
        base_dir,
        scale: scale.unwrap_or(2.0) as f32,
    })
}

#[napi(object)]
pub struct HtmlPreviewPage {
    pub width: f64,
    pub height: f64,
    pub html: String,
}

#[napi(object)]
pub struct HtmlPreviewResult {
    pub style: String,
    pub pages: Vec<HtmlPreviewPage>,
}

pub struct HtmlPreviewTask {
    tree: String,
    base_dir: Option<String>,
}

impl Task for HtmlPreviewTask {
    type Output = papier_engine::papier_html::HtmlPreview;
    type JsValue = HtmlPreviewResult;

    fn compute(&mut self) -> Result<Self::Output> {
        let doc = parse(&self.tree)?;
        let engine =
            engine(self.base_dir.as_deref()).map_err(|m| fail("invalid_option", m, None))?;
        engine.preview_html(&doc).map_err(engine_error)
    }

    fn resolve(&mut self, _env: Env, p: Self::Output) -> Result<Self::JsValue> {
        Ok(HtmlPreviewResult {
            style: p.style,
            pages: p
                .pages
                .into_iter()
                .map(|p| HtmlPreviewPage {
                    width: round2(p.width),
                    height: round2(p.height),
                    html: p.html,
                })
                .collect(),
        })
    }
}

fn round2(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

/// Document tree (JSON string) → HTML/CSS preview (shared style + one HTML fragment per page).
#[napi]
pub fn preview_html(tree: String, options: Option<RenderOptions>) -> AsyncTask<HtmlPreviewTask> {
    AsyncTask::new(HtmlPreviewTask {
        tree,
        base_dir: options.and_then(|o| o.base_dir),
    })
}

/// Version of the engine and of the tree format it reads.
#[napi]
pub fn version() -> String {
    format!(
        "{} (tree v{})",
        env!("CARGO_PKG_VERSION"),
        papier_engine::papier_tree::TREE_VERSION
    )
}
