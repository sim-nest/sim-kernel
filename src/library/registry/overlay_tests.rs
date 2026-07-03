use crate::{
    Cx, Error, Symbol, Value,
    library::{ExportKind, Registry},
};

fn bool_value(cx: &mut Cx, value: bool) -> Value {
    cx.factory().bool(value).unwrap()
}

fn sequence_next(registry: &Registry, kind: &str) -> Option<u64> {
    registry.catalog_sequence_next(&Symbol::new(kind))
}

#[test]
fn registry_overlay_success_commits_registered_rows() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let symbol = Symbol::new("overlay-value");

    let result = registry
        .with_catalog_overlay(|registry| {
            registry.register_value(symbol.clone(), bool_value(&mut cx, true))?;
            assert!(registry.value_by_symbol(&symbol).is_some());
            Ok(42)
        })
        .unwrap();

    assert_eq!(result, 42);
    assert!(registry.value_by_symbol(&symbol).is_some());
    assert_eq!(registry.catalog_export_row_count_for_tests(), 1);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), 1);
    registry.assert_catalog_projection_caches_for_tests();
}

#[test]
fn registry_overlay_error_rolls_back_registered_rows() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let symbol = Symbol::new("rolled-back-value");

    let err = registry
        .with_catalog_overlay(|registry| {
            registry.register_value(symbol.clone(), bool_value(&mut cx, true))?;
            assert!(registry.value_by_symbol(&symbol).is_some());
            Err::<(), Error>(Error::Eval("rollback registry overlay".to_owned()))
        })
        .unwrap_err();

    assert!(matches!(
        err,
        Error::Eval(message) if message == "rollback registry overlay"
    ));
    assert!(registry.value_by_symbol(&symbol).is_none());
    assert!(registry.export_symbols().is_empty());
    assert_eq!(registry.catalog_export_row_count_for_tests(), 0);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), 0);
    registry.assert_catalog_projection_caches_for_tests();
}

#[test]
fn duplicate_export_in_overlay_fails_without_changing_parent() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let existing = Symbol::new("existing-value");
    let temporary = Symbol::new("temporary-value");
    let existing_value = bool_value(&mut cx, true);
    registry
        .register_value(existing.clone(), existing_value.clone())
        .unwrap();

    let err = registry
        .with_catalog_overlay(|registry| {
            registry.register_value(temporary.clone(), bool_value(&mut cx, false))?;
            registry.register_value(existing.clone(), bool_value(&mut cx, false))?;
            Ok(())
        })
        .unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateExport { kind: "value", symbol } if symbol == existing
    ));
    assert_eq!(registry.value_by_symbol(&existing), Some(&existing_value));
    assert!(registry.value_by_symbol(&temporary).is_none());
    assert_eq!(registry.catalog_export_row_count_for_tests(), 1);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), 1);
    registry.assert_catalog_projection_caches_for_tests();
}

#[test]
fn sequence_reservations_roll_back_on_overlay_failure() {
    let mut registry = Registry::default();
    let before = sequence_next(&registry, "class");

    let err = registry
        .with_catalog_overlay(|registry| {
            let id = registry.fresh_class_id();
            assert_eq!(id.0, 1);
            assert_eq!(sequence_next(registry, "class"), Some(2));
            Err::<(), Error>(Error::Eval("rollback sequence".to_owned()))
        })
        .unwrap_err();

    assert!(matches!(
        err,
        Error::Eval(message) if message == "rollback sequence"
    ));
    assert_eq!(sequence_next(&registry, "class"), before);
    assert_eq!(registry.fresh_class_id().0, 1);
    assert_eq!(
        registry
            .export_symbols()
            .get(&ExportKind::named(ExportKind::CLASS)),
        None
    );
}
