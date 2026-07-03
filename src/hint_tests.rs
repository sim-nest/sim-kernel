use crate::{CapabilityName, Diagnostic, HintMetadata, Symbol};

use crate::testing::bare_cx as cx;

#[test]
fn hint_metadata_round_trips_through_related_diagnostic() {
    let hint = HintMetadata::new(Symbol::qualified("shape-hint", "expected"), "shape input")
        .with_detail("expected number expression, found string expression")
        .with_tag(Symbol::qualified("shape", "expected"))
        .with_argument(Symbol::new("value"))
        .with_capability(CapabilityName::new("browse.read"))
        .with_codec_form(Symbol::qualified("codec", "lisp"))
        .with_example("(shape/check Number \"x\")");

    let diagnostic = hint.clone().attach_to(
        Diagnostic::error("shape mismatch").with_code(Symbol::qualified("shape", "expected")),
    );

    let collected = HintMetadata::collect_from_diagnostic(&diagnostic);
    assert_eq!(collected, vec![hint]);
    assert!(collected[0].radar_text().contains("shape/expected"));
    assert!(collected[0].radar_text().contains("browse.read"));
}

#[test]
fn hint_metadata_projects_to_runtime_table() {
    let mut cx = cx();
    let hint = HintMetadata::new(Symbol::qualified("runtime-hint", "argument"), "argument")
        .with_argument(Symbol::new("input"))
        .with_example("(dispatch input)");

    let value = hint.as_value(&mut cx).unwrap();
    let table = value.object().as_table_impl().unwrap();
    let text = table
        .get(&mut cx, Symbol::new("radar-text"))
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();

    assert_eq!(
        text,
        crate::Expr::String("runtime-hint/argument argument input (dispatch input)".to_owned())
    );
}
