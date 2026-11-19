//! Time of each phase: `cargo run --release -p turbopdf-engine --example phases -- fixtures/invoice_200.json`
use std::{fs, time::Instant};
use turbopdf_engine::{
    turbopdf_fonts::{FontDb, NoLoader},
    turbopdf_tree::{Document, validate},
};

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let doc: Document = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    for round in 0..3 {
        let t = Instant::now();
        validate(&doc).unwrap();
        let t_validate = t.elapsed();
        let t = Instant::now();
        let db = FontDb::with_embedded();
        let t_fonts = t.elapsed();
        let t = Instant::now();
        let layout = turbopdf_layout::layout_document(&doc, &db, &NoLoader).unwrap();
        let t_layout = t.elapsed();
        let t = Instant::now();
        let pdf = turbopdf_pdf::write(
            &layout,
            &db,
            doc.meta.as_ref(),
            turbopdf_pdf::PdfOptions {
                tagged: true,
                pdfua: false,
                pdfa: None,
            },
        )
        .unwrap();
        let t_pdf = t.elapsed();
        println!(
            "round {round}: validate {:.2} ms | fonts {:.2} ms | layout {:.2} ms | pdf {:.2} ms | {} pages, {} bytes",
            t_validate.as_secs_f64() * 1e3,
            t_fonts.as_secs_f64() * 1e3,
            t_layout.as_secs_f64() * 1e3,
            t_pdf.as_secs_f64() * 1e3,
            layout.pages.len(),
            pdf.len()
        );
    }
}
