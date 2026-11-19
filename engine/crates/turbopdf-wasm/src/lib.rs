//! WebAssembly entry points, with a plain C ABI so that no binding generator is needed. They mirror the
//! Node binding (`render` → a prepared document, `convertHtml`, `htmlNodes`), so the SDK can use either.
//! `src/rust/wasm-core.ts` (SDK: `turbopdf` in a worker, `turbopdf/edge` on the calling thread) drives them.
//!
//! Inputs: the host allocates with `turbopdf_alloc`, writes, passes `ptr, len` and frees afterwards
//! (except `turbopdf_push_buffer`, which takes ownership). Every call returns a status (0 = ok,
//! 1 = error) and leaves its result in the output buffer (`turbopdf_output_ptr` / `turbopdf_output_len`).
//! On error the output is JSON `{ code, message, details? }`, the same shape as the Node binding.
//! Sizes and integers in outputs are little-endian `u32`; "bytes" means `u32 n` then `n` bytes.
//!
//! Files: `{ "file": … }` sources and linked stylesheets are asked to the host through the import
//! `turbopdf.host_read`, after the lexical checks below. The host resolves them inside its `baseDir`.

use serde_json::json;
use std::cell::RefCell;
use std::sync::Arc;
use turbopdf_engine::turbopdf_fonts::ResourceLoader;
use turbopdf_engine::turbopdf_tree::Document;
use turbopdf_engine::{Engine, EngineError, Prepared};

thread_local! {
    static ENGINE: Engine = Engine::new();
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static BUFFERS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static DOCS: RefCell<Vec<Option<Prepared>>> = const { RefCell::new(Vec::new()) };
}

// ------------------------------------------------------------------ errors

fn error_json(code: &str, message: String, details: Option<serde_json::Value>) -> Vec<u8> {
    let mut v = json!({ "code": code, "message": message });
    if let Some(d) = details {
        v["details"] = d;
    }
    v.to_string().into_bytes()
}

fn engine_error(e: EngineError) -> Vec<u8> {
    match e {
        EngineError::Invalid(errors) => {
            let details = errors
                .iter()
                .map(|e| json!({ "path": e.path, "code": e.code, "message": e.message }))
                .collect();
            error_json(
                "invalid_tree",
                EngineError::Invalid(errors).to_string(),
                Some(serde_json::Value::Array(details)),
            )
        }
        e @ EngineError::Font { .. } => error_json("font_failed", e.to_string(), None),
        EngineError::Pdf(turbopdf_engine::turbopdf_pdf::PdfError::Accessibility(problems)) => {
            let message = format!(
                "The PDF cannot be PDF/UA conformant: {}. Fix these, or drop the `pdfua` option.",
                problems.join("; ")
            );
            error_json("not_accessible", message, Some(json!(problems)))
        }
        EngineError::Pdf(turbopdf_engine::turbopdf_pdf::PdfError::Archival(standard, problems)) => {
            let message = format!(
                "The PDF cannot be {standard} conformant: {}. Fix these, or drop the `pdfa` option.",
                problems.join("; ")
            );
            error_json("not_pdfa", message, Some(json!(problems)))
        }
        e => error_json("render_failed", e.to_string(), None),
    }
}

fn parse(tree: &[u8]) -> Result<Document, Vec<u8>> {
    serde_json::from_slice(tree).map_err(|e| {
        error_json(
            "invalid_tree",
            format!("the document tree is not valid JSON for this engine: {e}"),
            None,
        )
    })
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn finish(result: Result<Vec<u8>, Vec<u8>>) -> u32 {
    let (status, out) = match result {
        Ok(out) => (0, out),
        Err(out) => (1, out),
    };
    OUTPUT.with(|o| *o.borrow_mut() = out);
    status
}

// ------------------------------------------------------------------ files

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "turbopdf")]
unsafe extern "C" {
    /// Read `path` inside the host's baseDir. The host allocates the result with `turbopdf_alloc`,
    /// writes `ptr, len` at `out` and returns 0 (bytes are the file) or 1 (bytes are a UTF-8 message).
    fn host_read(path: *const u8, path_len: usize, out: *mut u32) -> u32;
}

#[cfg(target_arch = "wasm32")]
fn read_host(path: &str) -> Result<Vec<u8>, String> {
    let mut out = [0u32; 2];
    let status = unsafe { host_read(path.as_ptr(), path.len(), out.as_mut_ptr()) };
    let (ptr, len) = (out[0] as usize as *mut u8, out[1] as usize);
    let bytes = if len == 0 {
        Vec::new()
    } else {
        // allocated by `turbopdf_alloc(len)`: ownership comes back here
        unsafe { Vec::from_raw_parts(ptr, len, len) }
    };
    match status {
        0 => Ok(bytes),
        _ => Err(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_host(path: &str) -> Result<Vec<u8>, String> {
    Err(format!(
        "{path:?}: files are only read in the WebAssembly build"
    ))
}

/// Files through the host (when it has a baseDir), and the buffers pushed before the call.
struct HostLoader {
    files: bool,
    buffers: Vec<Vec<u8>>,
}

impl ResourceLoader for HostLoader {
    fn buffer(&self, index: u32) -> Result<Vec<u8>, String> {
        self.buffers
            .get(index as usize)
            .cloned()
            .ok_or_else(|| format!("no buffer {index} was passed with the document"))
    }

    fn load(&self, path: &str) -> Result<Vec<u8>, String> {
        if !self.files {
            return Err(format!(
                "{path:?}: no baseDir was given, so files are not read"
            ));
        }
        if path.contains("://") && !path.starts_with("file://") {
            return Err(format!(
                "{path} is remote: TurboPDF never uses the network. Allow its origin with `allowRemote` (images), download it, or pass a data: URL"
            ));
        }
        read_host(path.trim_start_matches("file://"))
    }
}

fn loader(files: bool) -> HostLoader {
    HostLoader {
        files,
        buffers: BUFFERS.with(|b| std::mem::take(&mut *b.borrow_mut())),
    }
}

// ------------------------------------------------------------------ memory

/// Allocate `len` bytes the host can write into.
#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Free a buffer from `turbopdf_alloc`.
///
/// # Safety
/// `ptr` / `len` must come from one `turbopdf_alloc` call, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn turbopdf_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_output_ptr() -> *const u8 {
    OUTPUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_output_len() -> usize {
    OUTPUT.with(|o| o.borrow().len())
}

/// Hand over a buffer (from `turbopdf_alloc`, fully written) for the next `turbopdf_prepare`: the
/// `n`-th pushed buffer is `{ "buffer": n }`. The engine takes ownership.
///
/// # Safety
/// `ptr` / `len` must come from one `turbopdf_alloc(len)` call, not used by the host afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn turbopdf_push_buffer(ptr: *mut u8, len: usize) {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    BUFFERS.with(|b| b.borrow_mut().push(bytes));
}

// ------------------------------------------------------------------ documents

/// Lay a tree out once. `files`: 1 when the host reads files (it has a baseDir).
/// Output: `u32 handle | u32 pages | bytes warnings JSON | bytes PDF`.
///
/// # Safety
/// `ptr..ptr+len` must be readable memory of this instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn turbopdf_prepare(ptr: *const u8, len: usize, files: u32) -> u32 {
    let tree = unsafe { std::slice::from_raw_parts(ptr, len) };
    let loader = loader(files != 0);
    finish((|| {
        let doc = parse(tree)?;
        let prepared = if files == 0 && loader.buffers.is_empty() {
            ENGINE.with(|e| e.prepare(&doc))
        } else {
            Engine::with_loader(Arc::new(loader)).prepare(&doc)
        }
        .map_err(engine_error)?;
        let pdf = prepared.pdf().map_err(engine_error)?;
        let warnings = serde_json::to_string(prepared.warnings()).expect("warnings serialize");
        let pages = prepared.pages();
        let handle = DOCS.with(|d| {
            let mut docs = d.borrow_mut();
            match docs.iter().position(Option::is_none) {
                Some(i) => {
                    docs[i] = Some(prepared);
                    i
                }
                None => {
                    docs.push(Some(prepared));
                    docs.len() - 1
                }
            }
        }) as u32;
        let mut out = Vec::with_capacity(pdf.len() + warnings.len() + 16);
        out.extend_from_slice(&handle.to_le_bytes());
        out.extend_from_slice(&pages.to_le_bytes());
        put(&mut out, warnings.as_bytes());
        put(&mut out, &pdf);
        Ok(out)
    })())
}

fn with_doc(handle: u32, f: impl FnOnce(&Prepared) -> Result<Vec<u8>, Vec<u8>>) -> u32 {
    finish(DOCS.with(|d| match d.borrow().get(handle as usize) {
        Some(Some(doc)) => f(doc),
        _ => Err(error_json(
            "render_failed",
            "this document was released".into(),
            None,
        )),
    }))
}

/// One PNG per page. Output: `u32 count | bytes*`.
#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_png(handle: u32, scale: f32) -> u32 {
    with_doc(handle, |doc| {
        let pages = doc.png(scale).map_err(engine_error)?;
        let mut out = Vec::new();
        out.extend_from_slice(&(pages.len() as u32).to_le_bytes());
        for png in &pages {
            put(&mut out, png);
        }
        Ok(out)
    })
}

/// The HTML preview, as JSON `{ style, pages: [{ width, height, html }] }`.
#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_html(handle: u32) -> u32 {
    with_doc(handle, |doc| {
        let p = doc.html();
        let round2 = |v: f32| (v as f64 * 100.0).round() / 100.0;
        let pages: Vec<_> = p
            .pages
            .iter()
            .map(
                |p| json!({ "width": round2(p.width), "height": round2(p.height), "html": p.html }),
            )
            .collect();
        Ok(json!({ "style": p.style, "pages": pages })
            .to_string()
            .into_bytes())
    })
}

/// Release a document from `turbopdf_prepare`.
#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_release(handle: u32) {
    DOCS.with(|d| {
        if let Some(slot) = d.borrow_mut().get_mut(handle as usize) {
            *slot = None;
        }
    });
}

// ------------------------------------------------------------------ HTML

#[derive(serde::Deserialize)]
struct ConvertRequest {
    html: String,
    css: Option<String>,
    header: Option<String>,
    footer: Option<String>,
    /// The host reads files (it has a baseDir).
    #[serde(default)]
    files: bool,
}

/// HTML → document parts. Input: JSON `{ html, css?, header?, footer?, files? }`.
/// Output: `bytes parts JSON | u32 count | bytes*` (the `@font-face` buffers).
///
/// # Safety
/// `ptr..ptr+len` must be readable memory of this instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn turbopdf_convert(ptr: *const u8, len: usize) -> u32 {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    finish((|| {
        let req: ConvertRequest = serde_json::from_slice(input)
            .map_err(|e| error_json("invalid_option", e.to_string(), None))?;
        let loader = HostLoader {
            files: req.files,
            buffers: Vec::new(),
        };
        let input = turbopdf_markup::Input {
            html: &req.html,
            css: req.css.as_deref(),
            header: req.header.as_deref(),
            footer: req.footer.as_deref(),
        };
        let mut converted = turbopdf_markup::convert(&input, &loader)
            .map_err(|e| error_json("invalid_tree", e.to_string(), None))?;
        let buffers = std::mem::take(&mut converted.buffers);
        let mut out = Vec::new();
        put(
            &mut out,
            serde_json::to_string(&converted)
                .expect("parts serialize")
                .as_bytes(),
        );
        out.extend_from_slice(&(buffers.len() as u32).to_le_bytes());
        for b in &buffers {
            put(&mut out, b);
        }
        Ok(out)
    })())
}

/// An HTML fragment (raw HTML in Markdown) → JSON array of tree nodes.
///
/// # Safety
/// `ptr..ptr+len` must be readable memory of this instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn turbopdf_html_nodes(ptr: *const u8, len: usize) -> u32 {
    let html = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) });
    finish(
        turbopdf_markup::fragment(&html)
            .map(|nodes| {
                serde_json::to_string(&nodes)
                    .expect("nodes serialize")
                    .into_bytes()
            })
            .map_err(|e| error_json("invalid_tree", e.to_string(), None)),
    )
}

/// Version of the engine and of the tree format it reads, as UTF-8 in the output.
#[unsafe(no_mangle)]
pub extern "C" fn turbopdf_version() -> u32 {
    finish(Ok(format!(
        "{} (tree v{}, wasm)",
        env!("CARGO_PKG_VERSION"),
        turbopdf_engine::turbopdf_tree::TREE_VERSION
    )
    .into_bytes()))
}
