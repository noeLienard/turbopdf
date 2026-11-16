use super::*;

fn style(db: &FontDb, size: f32) -> TextStyle {
    TextStyle {
        face: db.select("Inter", 400, false),
        fallback: None,
        size,
        color: [0, 0, 0, 255],
        line_height: 1.4,
        link: None,
        effects: Effects::default(),
    }
}
fn span(db: &FontDb, text: &str, size: f32) -> Span {
    Span {
        text: text.to_string(),
        style: style(db, size),
    }
}
fn line_text(l: &Line) -> String {
    l.runs.iter().map(|r| r.text.as_str()).collect()
}

#[test]
fn shapes_a_single_line() {
    let db = FontDb::default();
    let p = layout(
        &db,
        &[span(&db, "Hello world", 12.0)],
        &ParagraphOptions::new(500.0),
    );
    assert_eq!(p.lines.len(), 1);
    assert_eq!(line_text(&p.lines[0]), "Hello world");
    assert!(p.width > 40.0 && p.width < 100.0, "width {}", p.width);
    assert!((p.height - 12.0 * 1.4).abs() < 0.5, "height {}", p.height);
    assert!(p.missing.is_empty());
}

#[test]
fn wraps_greedily_and_never_exceeds_width() {
    let db = FontDb::default();
    let text = "The quick brown fox jumps over the lazy dog and keeps running far away";
    let p = layout(&db, &[span(&db, text, 12.0)], &ParagraphOptions::new(120.0));
    assert!(p.lines.len() >= 4, "{} lines", p.lines.len());
    for l in &p.lines {
        assert!(
            l.width <= 120.01,
            "line too wide: {} {:?}",
            l.width,
            line_text(l)
        );
    }
    let joined = p.lines.iter().map(line_text).collect::<Vec<_>>().join(" ");
    assert_eq!(joined, text);
}

#[test]
fn hard_newlines_break_lines() {
    let db = FontDb::default();
    let p = layout(
        &db,
        &[span(&db, "one\ntwo\n\nfour", 12.0)],
        &ParagraphOptions::new(500.0),
    );
    assert_eq!(p.lines.len(), 4);
    assert_eq!(line_text(&p.lines[2]), ""); // blank line keeps its height
    assert!(p.lines[2].height > 10.0);
}

#[test]
fn alignment_moves_the_runs() {
    let db = FontDb::default();
    let spans = [span(&db, "right", 12.0)];
    let mut o = ParagraphOptions::new(200.0);
    o.align = Align::Right;
    let r = layout(&db, &spans, &o);
    assert!((r.lines[0].runs[0].x + r.lines[0].width - 200.0).abs() < 0.01);
    o.align = Align::Center;
    let c = layout(&db, &spans, &o);
    assert!((c.lines[0].runs[0].x - (200.0 - c.lines[0].width) / 2.0).abs() < 0.01);
}

#[test]
fn justify_fills_every_line_but_the_last() {
    let db = FontDb::default();
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    let mut o = ParagraphOptions::new(150.0);
    o.align = Align::Justify;
    let p = layout(&db, &[span(&db, text, 11.0)], &o);
    assert!(p.lines.len() > 2);
    for l in &p.lines[..p.lines.len() - 1] {
        assert!((l.width - 150.0).abs() < 0.05, "not justified: {}", l.width);
    }
    assert!(p.lines.last().unwrap().width < 150.0);
}

#[test]
fn styled_spans_share_a_line() {
    let db = FontDb::default();
    let bold = TextStyle {
        face: db.select("Inter", 700, false),
        ..style(&db, 12.0)
    };
    let spans = [
        span(&db, "plain ", 12.0),
        Span {
            text: "bold".into(),
            style: bold,
        },
        span(&db, " plain", 12.0),
    ];
    let p = layout(&db, &spans, &ParagraphOptions::new(500.0));
    assert_eq!(p.lines.len(), 1);
    assert!(p.lines[0].runs.len() >= 3, "styles must stay separate runs");
    assert_eq!(line_text(&p.lines[0]), "plain bold plain");
}

#[test]
fn an_unbreakable_word_overflows_instead_of_looping() {
    let db = FontDb::default();
    let p = layout(
        &db,
        &[span(&db, "Supercalifragilisticexpialidocious", 14.0)],
        &ParagraphOptions::new(40.0),
    );
    assert_eq!(p.lines.len(), 1);
    assert!(p.lines[0].width > 40.0);
}

#[test]
fn hyphenation_splits_long_words_when_enabled() {
    let db = FontDb::default();
    let spans = [span(&db, "internationalization international", 12.0)];
    let mut o = ParagraphOptions::new(70.0);
    o.lang = Some("en".into());
    o.hyphenate = true;
    let on = layout(&db, &spans, &o);
    assert!(
        on.lines.iter().any(|l| line_text(l).ends_with('-')),
        "{:?}",
        on.lines.iter().map(line_text).collect::<Vec<_>>()
    );
    for l in &on.lines {
        assert!(l.width <= 70.01);
    }
    o.hyphenate = false;
    let off = layout(&db, &spans, &o);
    assert!(!off.lines.iter().any(|l| line_text(l).ends_with('-')));
}

#[test]
fn reports_missing_glyphs_once() {
    let db = FontDb::default();
    let p = layout(
        &db,
        &[span(&db, "ok \u{10FFFD} \u{10FFFD}", 12.0)],
        &ParagraphOptions::new(500.0),
    );
    assert_eq!(p.missing.len(), 1);
    assert_eq!(p.missing[0].character, '\u{10FFFD}');
}

#[test]
fn accents_and_symbols_are_covered_by_inter() {
    let db = FontDb::default();
    let p = layout(
        &db,
        &[span(&db, "Écrit à Lyon — 12 000 € ✓ œuvre", 12.0)],
        &ParagraphOptions::new(500.0),
    );
    assert!(
        p.missing.iter().all(|m| m.character == '✓'),
        "{:?}",
        p.missing
    ); // only the checkmark may be absent
}

#[test]
fn intrinsic_widths_min_and_max() {
    let db = FontDb::default();
    let (min, max) = intrinsic_widths(&db, &[span(&db, "short extraordinarily", 12.0)]);
    let one = layout(
        &db,
        &[span(&db, "short extraordinarily", 12.0)],
        &ParagraphOptions::new(1000.0),
    );
    assert!((max - one.width).abs() < 0.01);
    assert!(min < max && min > 40.0);
}

#[test]
fn layout_is_deterministic() {
    let db = FontDb::default();
    let spans = [span(
        &db,
        "Determinism matters: same text, same glyphs, same positions.",
        11.0,
    )];
    let a = layout(&db, &spans, &ParagraphOptions::new(180.0));
    let b = layout(&db, &spans, &ParagraphOptions::new(180.0));
    assert_eq!(a, b);
}
