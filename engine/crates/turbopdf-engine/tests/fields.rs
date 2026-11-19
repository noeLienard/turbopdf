//! Form fields: HTML controls and `field` boxes are laid out and reported with their place.

use serde_json::{Value, json};
use turbopdf_engine::Engine;
use turbopdf_engine::turbopdf_tree::{Document, FieldBox, FieldKind};

fn render(children: Value) -> Vec<FieldBox> {
    let doc: Document = serde_json::from_value(
        json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": children }] }),
    )
    .expect("valid tree JSON");
    let prepared = Engine::new().prepare(&doc).expect("prepare");
    prepared.pdf().expect("pdf");
    prepared.fields()
}

fn html(source: &str) -> Vec<FieldBox> {
    let nodes = turbopdf_markup::fragment(source).expect("html");
    render(serde_json::to_value(nodes).unwrap())
}

#[test]
fn html_controls_become_fields() {
    let f = html(
        r#"<p><label for="nom">Nom :</label></p>
        <input id="nom" name="nom" placeholder="Jeanne Dupont" required maxlength="40">
        <textarea name="note" rows="3">Livrer le matin</textarea>
        <label><input type="checkbox" name="cgv" checked> J'accepte les CGV</label>
        <label><input type="radio" name="livraison" value="poste"> Poste</label>
        <label><input type="radio" name="livraison" value="retrait" checked> Retrait</label>
        <select name="pays" aria-label="Pays"><option>France</option><option selected value="BE">Belgique</option></select>
        <input type="hidden" name="secret" value="x">
        <input placeholder="sans nom">
        <input type="submit" value="Envoyer">"#,
    );
    let names: Vec<_> = f.iter().map(|b| b.field.name.as_str()).collect();
    assert_eq!(
        names,
        ["nom", "note", "cgv", "livraison", "livraison", "pays"]
    );

    let nom = &f[0];
    assert_eq!(nom.field.kind, FieldKind::Text);
    assert_eq!(nom.field.label.as_deref(), Some("Nom"));
    assert_eq!(
        (
            nom.field.required,
            nom.field.max_length,
            nom.field.value.as_deref()
        ),
        (Some(true), Some(40), None)
    );
    assert_eq!(nom.page, 1);
    assert!((nom.width - 160.0).abs() < 0.01, "{}", nom.width);
    assert!(nom.height > 10.0 && nom.height < 25.0, "{}", nom.height);
    assert!((nom.page_width - 595.28).abs() < 0.1);

    let note = &f[1];
    assert_eq!(
        (note.field.multiline, note.field.value.as_deref()),
        (Some(true), Some("Livrer le matin"))
    );
    assert!(
        note.height > nom.height * 1.8,
        "{} vs {}",
        note.height,
        nom.height
    );
    assert!(
        note.y >= nom.y + nom.height - 0.01,
        "{} {} {}",
        note.y,
        nom.y,
        nom.height
    );

    let cgv = &f[2];
    assert_eq!(
        (
            cgv.field.kind,
            cgv.field.checked,
            cgv.field.label.as_deref()
        ),
        (FieldKind::Checkbox, Some(true), Some("J'accepte les CGV"))
    );
    assert!((cgv.width - 10.0).abs() < 0.01 && (cgv.height - 10.0).abs() < 0.01);

    assert_eq!(
        (f[3].field.checked, f[3].field.value.as_deref()),
        (Some(false), Some("poste"))
    );
    assert_eq!(
        (f[4].field.checked, f[4].field.value.as_deref()),
        (Some(true), Some("retrait"))
    );

    let pays = &f[5];
    assert_eq!(pays.field.kind, FieldKind::Select);
    assert_eq!(
        pays.field.options.as_deref(),
        Some(&["France".to_string(), "BE".to_string()][..])
    );
    assert_eq!(
        (pays.field.value.as_deref(), pays.field.label.as_deref()),
        (Some("BE"), Some("Pays"))
    );
    assert_eq!(pays.border_width, 0.75);
    assert!(nom.y + nom.height < note.y, "a gap between controls");
    assert_eq!(pays.color, "#111111");
}

#[test]
fn a_field_box_in_the_tree_and_on_a_later_page() {
    let f = render(json!([
        { "type": "view", "style": { "height": 900 }, "children": [] },
        { "type": "view", "style": { "width": 200, "height": 60, "backgroundColor": "#fef3c7", "fontSize": 9 },
          "field": { "name": "sig", "kind": "signature", "label": "Signature du client" }, "children": [] }
    ]));
    assert_eq!(f.len(), 1);
    let s = &f[0];
    assert_eq!((s.page, s.field.kind), (2, FieldKind::Signature));
    assert_eq!((s.width, s.height, s.font_size), (200.0, 60.0, 9.0));
    assert_eq!(s.background.as_deref(), Some("#fef3c7"));
    assert!(s.y < 100.0, "top of page 2: {}", s.y);
}

#[test]
fn invalid_fields_are_refused() {
    let doc: Document = serde_json::from_value(json!({ "type": "document", "version": 1, "children": [{ "type": "page", "children": [
        { "type": "view", "field": { "name": "p", "kind": "select", "options": ["a"], "value": "b" }, "children": [] },
        { "type": "view", "field": { "name": "", "kind": "text" }, "children": [] },
        { "type": "view", "field": { "name": "r", "kind": "radio" }, "children": [] }
    ] }] }))
    .unwrap();
    let e = Engine::new()
        .prepare(&doc)
        .err()
        .expect("invalid")
        .to_string();
    for code in ["field_value", "field_name"] {
        assert!(
            e.contains(code) || e.contains("not one of the options"),
            "{code}: {e}"
        );
    }
}
