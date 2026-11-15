//! Time of each phase: `cargo run --release -p papier-engine --example phases -- fixtures/invoice_200.json`
use papier_engine::{
    papier_fonts::{FontDb, NoLoader},
    papier_tree::{Document, validate},
};
use std::{fs, time::Instant};

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
        let layout = papier_layout::layout_document(&doc, &db, &NoLoader).unwrap();
        let t_layout = t.elapsed();
        let t = Instant::now();
        let pdf = papier_pdf::write(
            &layout,
            &db,
            doc.meta.as_ref(),
            papier_pdf::PdfOptions {
                tagged: true,
                pdfua: false,
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
