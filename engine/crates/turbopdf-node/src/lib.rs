//! Node.js binding: `render(treeJson, options)` lays a document out once and returns a `NativeDocument`
//! (PDF, warnings, and the PNG / HTML previews drawn from the same layout).
//!
//! Work runs on the libuv thread pool (`AsyncTask`), so a render never blocks the event loop.
//! Errors are thrown as `Error` whose message is a JSON object `{ code, message, details? }` that the
//! TypeScript SDK turns into a `TurboPdfError`.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use turbopdf_engine::turbopdf_fonts::ResourceLoader;
use turbopdf_engine::turbopdf_tree::Document;
use turbopdf_engine::{Engine, EngineError, Prepared};

/// Reads `{ "file": … }` sources, only inside `base` (no `..` escape, no absolute path elsewhere).
/// Files inside `baseDir` (never outside it), and the buffers passed next to the tree.
struct DirLoader {
    base: Option<PathBuf>,
    buffers: Vec<Vec<u8>>,
}

impl ResourceLoader for DirLoader {
    fn buffer(&self, index: u32) -> std::result::Result<Vec<u8>, String> {
        self.buffers
            .get(index as usize)
            .cloned()
            .ok_or_else(|| format!("no buffer {index} was passed with the document"))
    }

    fn load(&self, path: &str) -> std::result::Result<Vec<u8>, String> {
        let Some(base) = &self.base else {
            return Err(format!(
                "{path:?}: no baseDir was given, so files are not read"
            ));
        };
        if path.contains("://") && !path.starts_with("file://") {
            return Err(format!(
                "{path} is remote: TurboPDF never uses the network. Allow its origin with `allowRemote` (images), download it, or pass a data: URL"
            ));
        }
        let requested = Path::new(path.trim_start_matches("file://"));
        // lexical check first: `../x` is refused even when it does not exist
        let mut depth = 0i32;
        for c in requested.components() {
            match c {
                std::path::Component::ParentDir => depth -= 1,
                std::path::Component::Normal(_) => depth += 1,
                _ => {}
            }
            if depth < 0 {
                break;
            }
        }
        let outside = || format!("{path:?} is outside baseDir ({})", base.display());
        let full = base.join(requested);
        // (absolute paths are only checked once resolved: `/var/…` may be `/private/var/…`)
        if depth < 0 && !requested.is_absolute() {
            return Err(outside());
        }
        let real = full
            .canonicalize()
            .map_err(|_| format!("{path:?} was not found in baseDir ({})", base.display()))?;
        // symbolic links are followed: the file itself must be inside too
        if !real.starts_with(base) {
            return Err(outside());
        }
        std::fs::read(&real).map_err(|e| format!("cannot read {path:?}: {e}"))
    }
}

/// Fonts are parsed once per process for the common case (no file access, no buffers).
fn engine(
    base_dir: Option<&str>,
    buffers: Vec<Vec<u8>>,
) -> std::result::Result<Arc<Engine>, String> {
    static DEFAULT: OnceLock<Arc<Engine>> = OnceLock::new();
    if base_dir.is_none() && buffers.is_empty() {
        return Ok(DEFAULT.get_or_init(|| Arc::new(Engine::new())).clone());
    }
    let base = base_dir
        .map(|dir| {
            Path::new(dir)
                .canonicalize()
                .map_err(|e| format!("baseDir {dir:?}: {e}"))
        })
        .transpose()?;
    Ok(Arc::new(Engine::with_loader(Arc::new(DirLoader {
        base,
        buffers,
    }))))
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
        EngineError::Pdf(turbopdf_engine::turbopdf_pdf::PdfError::Accessibility(problems)) => {
            let message = format!(
                "The PDF cannot be PDF/UA conformant: {}. Fix these, or drop the `pdfua` option.",
                problems.join("; ")
            );
            fail("not_accessible", message, Some(json!(problems)))
        }
        EngineError::Pdf(turbopdf_engine::turbopdf_pdf::PdfError::Archival(standard, problems)) => {
            let message = format!(
                "The PDF cannot be {standard} conformant: {}. Fix these, or drop the `pdfa` option.",
                problems.join("; ")
            );
            fail("not_pdfa", message, Some(json!(problems)))
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
    /// Bytes referenced by `{ "buffer": n }` sources (fonts, images), passed without base64.
    pub buffers: Option<Vec<Buffer>>,
}

/// A laid-out document. The PDF is written once; the PNGs and the HTML preview are drawn from the same
/// layout, without parsing or laying the document out again.
#[napi]
pub struct NativeDocument {
    prepared: Arc<Prepared>,
    pdf: Option<Vec<u8>>,
    warnings: String,
}

#[napi]
impl NativeDocument {
    #[napi(getter)]
    pub fn pages(&self) -> u32 {
        self.prepared.pages()
    }

    /// JSON array of warnings (same shape as `Warning` in tree.ts).
    #[napi(getter)]
    pub fn warnings(&self) -> String {
        self.warnings.clone()
    }

    /// JSON array of the form fields and where they were laid out (`FieldBox` in tree.ts).
    #[napi(getter)]
    pub fn fields(&self) -> String {
        serde_json::to_string(&self.prepared.fields()).expect("fields serialize")
    }

    /// The PDF bytes. Handed over without a copy, so it can be taken only once.
    #[napi]
    pub fn take_pdf(&mut self) -> Result<Buffer> {
        self.pdf
            .take()
            .map(Buffer::from)
            .ok_or_else(|| fail("render_failed", "the PDF was already taken".into(), None))
    }

    /// One PNG per page. `scale`: pixels per point (1 = 72 dpi). Default 2.
    #[napi]
    pub fn png(&self, scale: Option<f64>) -> AsyncTask<PngTask> {
        AsyncTask::new(PngTask {
            prepared: self.prepared.clone(),
            scale: scale.unwrap_or(2.0) as f32,
        })
    }

    /// HTML/CSS preview (shared style + one HTML fragment per page).
    #[napi]
    pub fn html(&self) -> AsyncTask<HtmlTask> {
        AsyncTask::new(HtmlTask {
            prepared: self.prepared.clone(),
        })
    }
}

pub struct RenderTask {
    tree: String,
    base_dir: Option<String>,
    buffers: Vec<Vec<u8>>,
}

impl Task for RenderTask {
    type Output = (Prepared, Vec<u8>, String);
    type JsValue = NativeDocument;

    fn compute(&mut self) -> Result<Self::Output> {
        let doc = parse(&self.tree)?;
        let buffers = std::mem::take(&mut self.buffers);
        let engine = engine(self.base_dir.as_deref(), buffers)
            .map_err(|m| fail("invalid_option", m, None))?;
        let prepared = engine.prepare(&doc).map_err(engine_error)?;
        let pdf = prepared.pdf().map_err(engine_error)?;
        let warnings = serde_json::to_string(prepared.warnings()).expect("warnings serialize");
        Ok((prepared, pdf, warnings))
    }

    fn resolve(
        &mut self,
        _env: Env,
        (prepared, pdf, warnings): Self::Output,
    ) -> Result<Self::JsValue> {
        Ok(NativeDocument {
            prepared: Arc::new(prepared),
            pdf: Some(pdf),
            warnings,
        })
    }
}

/// Document tree (JSON string) → a laid-out document with its PDF.
#[napi]
pub fn render(tree: String, options: Option<RenderOptions>) -> AsyncTask<RenderTask> {
    let (base_dir, buffers) = options.map_or((None, None), |o| (o.base_dir, o.buffers));
    AsyncTask::new(RenderTask {
        tree,
        base_dir,
        // copied on the JS thread: a napi Buffer must not be dropped on a worker thread
        buffers: buffers
            .unwrap_or_default()
            .iter()
            .map(|b| b.to_vec())
            .collect(),
    })
}

pub struct PngTask {
    prepared: Arc<Prepared>,
    scale: f32,
}

impl Task for PngTask {
    type Output = Vec<Vec<u8>>;
    type JsValue = Vec<Buffer>;

    fn compute(&mut self) -> Result<Self::Output> {
        self.prepared.png(self.scale).map_err(engine_error)
    }

    fn resolve(&mut self, _env: Env, pages: Self::Output) -> Result<Self::JsValue> {
        Ok(pages.into_iter().map(Buffer::from).collect())
    }
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

pub struct HtmlTask {
    prepared: Arc<Prepared>,
}

impl Task for HtmlTask {
    type Output = turbopdf_engine::turbopdf_html::HtmlPreview;
    type JsValue = HtmlPreviewResult;

    fn compute(&mut self) -> Result<Self::Output> {
        Ok(self.prepared.html())
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

#[napi(object)]
pub struct ConvertOptions {
    /// Directory that linked stylesheets and `@font-face` files are read from (never outside it).
    pub base_dir: Option<String>,
    pub css: Option<String>,
    pub header: Option<String>,
    pub footer: Option<String>,
}

#[napi(object)]
pub struct ConvertResult {
    /// JSON: `{ meta, pages, header, footer, page, fonts, warnings }`.
    pub parts: String,
    /// Bytes of the `@font-face` fonts, referenced as `{ "buffer": n }`.
    pub buffers: Vec<Buffer>,
}

pub struct ConvertTask {
    html: String,
    options: ConvertOptions,
}

impl Task for ConvertTask {
    type Output = (String, Vec<Vec<u8>>);
    type JsValue = ConvertResult;

    fn compute(&mut self) -> Result<Self::Output> {
        let base = self
            .options
            .base_dir
            .as_deref()
            .map(|dir| {
                Path::new(dir)
                    .canonicalize()
                    .map_err(|e| fail("invalid_option", format!("baseDir {dir:?}: {e}"), None))
            })
            .transpose()?;
        let loader = DirLoader {
            base,
            buffers: Vec::new(),
        };
        let input = turbopdf_markup::Input {
            html: &self.html,
            css: self.options.css.as_deref(),
            header: self.options.header.as_deref(),
            footer: self.options.footer.as_deref(),
        };
        let mut out = turbopdf_markup::convert(&input, &loader)
            .map_err(|e| fail("invalid_tree", e.to_string(), None))?;
        let buffers = std::mem::take(&mut out.buffers);
        Ok((
            serde_json::to_string(&out).expect("parts serialize"),
            buffers,
        ))
    }

    fn resolve(&mut self, _env: Env, (parts, buffers): Self::Output) -> Result<Self::JsValue> {
        Ok(ConvertResult {
            parts,
            buffers: buffers.into_iter().map(Buffer::from).collect(),
        })
    }
}

/// HTML (a page, a fragment, or React's static markup) → the parts of a document tree, with the CSS
/// cascade applied. HTML5 parsing (html5ever); stylesheets and fonts read inside `baseDir`.
#[napi]
pub fn convert_html(html: String, options: Option<ConvertOptions>) -> AsyncTask<ConvertTask> {
    AsyncTask::new(ConvertTask {
        html,
        options: options.unwrap_or(ConvertOptions {
            base_dir: None,
            css: None,
            header: None,
            footer: None,
        }),
    })
}

/// An HTML fragment (raw HTML in Markdown) → JSON array of tree nodes. Inline styles only, no files.
#[napi]
pub fn html_nodes(html: String) -> Result<String> {
    let nodes =
        turbopdf_markup::fragment(&html).map_err(|e| fail("invalid_tree", e.to_string(), None))?;
    Ok(serde_json::to_string(&nodes).expect("nodes serialize"))
}

/// Version of the engine and of the tree format it reads.
#[napi]
pub fn version() -> String {
    format!(
        "{} (tree v{})",
        env!("CARGO_PKG_VERSION"),
        turbopdf_engine::turbopdf_tree::TREE_VERSION
    )
}
