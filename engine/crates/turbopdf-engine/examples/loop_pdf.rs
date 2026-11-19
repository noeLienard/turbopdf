//! Profiling helper: runs PDF serialization in a loop.
use turbopdf_engine::{
    turbopdf_fonts::{FontDb, NoLoader},
    turbopdf_tree::Document,
};
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let doc: Document = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let db = FontDb::with_embedded();
    let layout = turbopdf_layout::layout_document(&doc, &db, &NoLoader).unwrap();
    for _ in 0..800 {
        std::hint::black_box(
            turbopdf_pdf::write(
                &layout,
                &db,
                doc.meta.as_ref(),
                turbopdf_pdf::PdfOptions {
                    tagged: true,
                    pdfua: false,
                    pdfa: None,
                },
            )
            .unwrap(),
        );
    }
}
