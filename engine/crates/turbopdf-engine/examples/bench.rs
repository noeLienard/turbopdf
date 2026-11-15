//! `cargo run --release -p turbopdf-engine --example bench -- fixtures/invoice_200.json`
//! Median time of the full pipeline (validate → layout → PDF), without and with PNG previews.
use std::{fs, time::Instant};
use turbopdf_engine::Engine;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: bench <tree.json>");
    let doc = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let engine = Engine::new();
    let runs = 30;
    let _ = engine.render(&doc).unwrap(); // warm-up
    let mut pdf_ms = Vec::new();
    let mut both_ms = Vec::new();
    let mut pages = 0;
    for _ in 0..runs {
        let t = Instant::now();
        let out = engine.render(&doc).unwrap();
        pdf_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        pages = out.pages;
        let t = Instant::now();
        let _ = engine.render_with_previews(&doc, 2.0).unwrap();
        both_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("{path}: {pages} pages");
    println!(
        "  PDF only          median {:.1} ms (min {:.1})",
        median(pdf_ms.clone()),
        pdf_ms.iter().cloned().fold(f64::MAX, f64::min)
    );
    println!(
        "  PDF + PNG (x2)    median {:.1} ms (min {:.1})",
        median(both_ms.clone()),
        both_ms.iter().cloned().fold(f64::MAX, f64::min)
    );
}
