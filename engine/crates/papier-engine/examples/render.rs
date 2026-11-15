//! `cargo run --release -p papier-engine --example render -- fixtures/invoice.json out/`
//! Renders a document tree to `<out>/<name>.pdf` and `<out>/<name>-<n>.png`, and prints the warnings.
use papier_engine::Engine;
use std::{fs, path::Path, time::Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(input), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("usage: render <tree.json> <out-dir> [scale]");
        std::process::exit(2);
    };
    let scale: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.5);
    let doc =
        serde_json::from_str(&fs::read_to_string(input).expect("read tree")).expect("parse tree");
    let name = Path::new(input)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    fs::create_dir_all(out).unwrap();

    let engine = Engine::new();
    let t = Instant::now();
    let (res, previews) = match engine.render_with_previews(&doc, scale) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let elapsed = t.elapsed();
    fs::write(Path::new(out).join(format!("{name}.pdf")), &res.pdf).unwrap();
    for (i, png) in previews.iter().enumerate() {
        fs::write(Path::new(out).join(format!("{name}-{}.png", i + 1)), png).unwrap();
    }
    println!(
        "{name}: {} page(s), {} bytes, {:.1} ms (pdf + png)",
        res.pages,
        res.pdf.len(),
        elapsed.as_secs_f64() * 1000.0
    );
    for w in &res.warnings {
        println!("  warning: {}", serde_json::to_string(w).unwrap());
    }
}
