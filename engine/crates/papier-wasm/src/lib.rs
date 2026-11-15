//! WebAssembly entry points, with a plain C ABI so that no binding generator is needed.
//! `engine/wasm/papier.mjs` wraps them for Node and browsers.
//!
//! Protocol: the host allocates the tree JSON with `papier_alloc`, calls `papier_render`, then reads the
//! output buffer (`papier_output_ptr` / `papier_output_len`):
//!
//! * status 0: `u32 pages | u32 n | n bytes warnings JSON | u32 n | n bytes PDF | u32 count | (u32 n | n bytes PNG)*`
//! * status 1: an error as JSON `{ code, message, details? }` (same shape as the Node binding)
//!
//! All integers are little-endian.

use papier_engine::papier_tree::Document;
use papier_engine::{Engine, EngineError};
use serde_json::json;
use std::cell::RefCell;

thread_local! {
    static ENGINE: Engine = Engine::new();
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

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
        EngineError::Pdf(papier_engine::papier_pdf::PdfError::Accessibility(problems)) => {
            let message = format!(
                "The PDF cannot be PDF/UA conformant: {}. Fix these, or drop the `pdfua` option.",
                problems.join("; ")
            );
            error_json("not_accessible", message, Some(json!(problems)))
        }
        e => error_json("render_failed", e.to_string(), None),
    }
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Render a tree. `scale > 0` also renders PNG previews at that scale. Returns 0 on success, 1 on error.
pub fn render(tree: &[u8], scale: f32) -> (u32, Vec<u8>) {
    let doc: Document = match serde_json::from_slice(tree) {
        Ok(d) => d,
        Err(e) => {
            return (
                1,
                error_json(
                    "invalid_tree",
                    format!("the document tree is not valid JSON for this engine: {e}"),
                    None,
                ),
            );
        }
    };
    ENGINE.with(|engine| {
        let result = if scale > 0.0 {
            engine.render_with_previews(&doc, scale)
        } else {
            engine.render(&doc).map(|o| (o, Vec::new()))
        };
        match result {
            Err(e) => (1, engine_error(e)),
            Ok((out, previews)) => {
                let mut buf = Vec::with_capacity(out.pdf.len() + 1024);
                buf.extend_from_slice(&out.pages.to_le_bytes());
                put(
                    &mut buf,
                    serde_json::to_string(&out.warnings)
                        .expect("warnings serialize")
                        .as_bytes(),
                );
                put(&mut buf, &out.pdf);
                buf.extend_from_slice(&(previews.len() as u32).to_le_bytes());
                for png in &previews {
                    put(&mut buf, png);
                }
                (0, buf)
            }
        }
    })
}

/// Allocate `len` bytes the host can write into.
#[unsafe(no_mangle)]
pub extern "C" fn papier_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Free a buffer from `papier_alloc`.
///
/// # Safety
/// `ptr` / `len` must come from one `papier_alloc` call, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn papier_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Render the tree JSON at `ptr..ptr+len` (the buffer stays owned by the host).
///
/// # Safety
/// `ptr..ptr+len` must be readable memory of this instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn papier_render(ptr: *const u8, len: usize, scale: f32) -> u32 {
    let tree = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (status, out) = render(tree, scale);
    OUTPUT.with(|o| *o.borrow_mut() = out);
    status
}

#[unsafe(no_mangle)]
pub extern "C" fn papier_output_ptr() -> *const u8 {
    OUTPUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn papier_output_len() -> usize {
    OUTPUT.with(|o| o.borrow().len())
}
