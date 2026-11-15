//! Profiling helper: runs layout in a loop.
use std::fs;
use turbopdf_engine::{
    turbopdf_fonts::{FontDb, NoLoader},
    turbopdf_tree::Document,
};

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let doc: Document = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let db = FontDb::with_embedded();
    for _ in 0..600 {
        std::hint::black_box(turbopdf_layout::layout_document(&doc, &db, &NoLoader).unwrap());
    }
}
