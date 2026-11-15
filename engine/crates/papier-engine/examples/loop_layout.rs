//! Profiling helper: runs layout in a loop.
use papier_engine::{
    papier_fonts::{FontDb, NoLoader},
    papier_tree::Document,
};
use std::fs;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let doc: Document = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let db = FontDb::with_embedded();
    for _ in 0..600 {
        std::hint::black_box(papier_layout::layout_document(&doc, &db, &NoLoader).unwrap());
    }
}
