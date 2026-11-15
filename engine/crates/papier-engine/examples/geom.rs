//! Debug helper: prints per-page text baselines.
use papier_engine::{
    papier_fonts::{FontDb, NoLoader},
    papier_tree::Document,
};
use papier_layout::DrawOp;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let doc: Document = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let db = FontDb::with_embedded();
    let l = papier_layout::layout_document(&doc, &db, &NoLoader).unwrap();
    for (i, p) in l.pages.iter().enumerate() {
        let mut ys: Vec<f32> = p
            .ops
            .iter()
            .filter_map(|o| {
                if let DrawOp::Text { baseline, .. } = o {
                    Some(*baseline)
                } else {
                    None
                }
            })
            .collect();
        ys.sort_by(f32::total_cmp);
        ys.dedup();
        let d: Vec<f32> = ys.windows(2).map(|w| w[1] - w[0]).collect();
        println!(
            "page {}: {} baselines, first {:?}, last {:?}, steps {:?}",
            i + 1,
            ys.len(),
            ys.first(),
            ys.last(),
            &d[..d.len().min(6)]
        );
    }
}
