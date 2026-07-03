use std::sync::Arc;

use crate::{
    AbiVersion, CORE_EXPR_CLASS_ID, Cx, DefaultFactory, Error, ExportKind, ExportState, Expr,
    Factory, FunctionId, LibId, LibManifest, LibTarget, Registry, Result, RuntimeId, Symbol, Test,
    TestReport, Value,
    catalog::{CatalogStore, CatalogWritePolicy},
};

use super::{
    export_row, exports_table, install_registry_catalog_schema, lib_row, libs_table,
    number_ops_table, plain_value_row, promotion_rules_table, runtime_table, runtime_value_row,
    schema::{
        field, lib_key, plain_value_key, registry_catalog_specs, runtime_key, sequence_key,
        split_key, test_key,
    },
    schema_table, sequence_row, sequences_table, test_row, tests_by_lib_table, tests_table,
    value_promotion_rules_table,
};

fn manifest(id: &str) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: crate::Version("1.2.3".to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target: LibTarget::HostRegistered,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: Vec::new(),
    }
}

fn int(value: &str) -> Expr {
    Expr::Number(crate::NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_owned(),
    })
}

fn bool_value(value: bool) -> Value {
    DefaultFactory.bool(value).unwrap()
}

fn assert_duplicate_export(err: Error, kind: &'static str, symbol: &Symbol) {
    assert!(matches!(
        err,
        Error::DuplicateExport {
            kind: duplicate_kind,
            symbol: duplicate_symbol,
        } if duplicate_kind == kind && duplicate_symbol == symbol.clone()
    ));
}

struct DummyTest;

impl Test for DummyTest {
    fn symbol(&self) -> Symbol {
        Symbol::new("demo-test")
    }

    fn lib(&self) -> Symbol {
        Symbol::new("demo-lib")
    }

    fn describe(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().nil()
    }

    fn run(&self, _cx: &mut Cx) -> Result<TestReport> {
        Ok(TestReport::from_result(
            Symbol::new("demo-test"),
            true,
            None,
        ))
    }
}

#[test]
fn schema_installs_every_registry_catalog_table() {
    let mut store = CatalogStore::new();

    install_registry_catalog_schema(&mut store).unwrap();

    let expected = registry_catalog_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect::<Vec<_>>();
    assert_eq!(store.tables().len(), expected.len());
    for table in expected {
        assert!(store.table(&table).is_some(), "missing table {table}");
    }
}

#[test]
fn schema_uses_expected_write_policies() {
    let mut store = CatalogStore::new();
    install_registry_catalog_schema(&mut store).unwrap();

    let expected = [
        (schema_table(), CatalogWritePolicy::Sealed),
        (sequences_table(), CatalogWritePolicy::Mutable),
        (libs_table(), CatalogWritePolicy::Sealed),
        (exports_table(), CatalogWritePolicy::Sealed),
        (runtime_table(), CatalogWritePolicy::Sealed),
        (tests_table(), CatalogWritePolicy::Sealed),
        (tests_by_lib_table(), CatalogWritePolicy::Derived),
        (number_ops_table(), CatalogWritePolicy::AppendOnly),
        (promotion_rules_table(), CatalogWritePolicy::AppendOnly),
        (
            value_promotion_rules_table(),
            CatalogWritePolicy::AppendOnly,
        ),
    ];

    for (table, policy) in expected {
        assert_eq!(store.table(&table).unwrap().policy, policy);
    }
}

#[test]
fn row_keys_match_documented_forms() {
    let lib = manifest("demo-lib");
    let value = bool_value(true);

    assert_eq!(
        sequence_row(Symbol::new("lib"), 1).key.as_qualified_str(),
        "registry-seq/lib"
    );
    assert_eq!(
        lib_row(LibId(7), &lib, true).key.as_qualified_str(),
        "registry-lib/demo-lib"
    );
    assert_eq!(
        export_row(
            ExportKind::named(ExportKind::FUNCTION),
            Symbol::qualified("core", "add"),
            Symbol::new("demo-lib"),
            ExportState::Resolved {
                id: RuntimeId::Function(FunctionId(3)),
            },
        )
        .key
        .as_qualified_str(),
        "registry-export/function/core%2Fadd"
    );
    assert_eq!(
        runtime_value_row(
            Symbol::new("class"),
            7,
            Symbol::new("Widget"),
            value.clone()
        )
        .key
        .as_qualified_str(),
        "registry-runtime/class/7"
    );
    assert_eq!(
        plain_value_row(Symbol::qualified("core", "pi"), value.clone())
            .key
            .as_qualified_str(),
        "registry-runtime/value/core%2Fpi"
    );
    assert_eq!(
        test_row(
            Symbol::qualified("demo", "smoke"),
            Symbol::new("demo-lib"),
            Arc::new(DummyTest),
            Vec::new(),
        )
        .key
        .as_qualified_str(),
        "registry-test/demo%2Fsmoke"
    );
}

#[test]
fn row_key_helpers_round_trip_components() {
    let qualified = Symbol::qualified("core", "Thing");

    assert_eq!(
        split_key(&sequence_key(&Symbol::new("number-domain"))),
        vec!["registry-seq", "number-domain"]
    );
    assert_eq!(
        split_key(&lib_key(&qualified)),
        vec!["registry-lib", "core/Thing"]
    );
    assert_eq!(
        split_key(&runtime_key(&Symbol::new("shape"), 42)),
        vec!["registry-runtime", "shape", "42"]
    );
    assert_eq!(
        split_key(&plain_value_key(&qualified)),
        vec!["registry-runtime", "value", "core/Thing"]
    );
    assert_eq!(
        split_key(&test_key(&qualified)),
        vec!["registry-test", "core/Thing"]
    );
}

#[test]
fn row_data_fields_are_deterministic() {
    let manifest = manifest("demo-lib");
    let row = lib_row(LibId(7), &manifest, true);

    assert_eq!(row.data.get(&field("id")), Some(&int("7")));
    assert_eq!(
        row.data.get(&field("symbol")),
        Some(&Expr::Symbol(Symbol::new("demo-lib")))
    );
    assert_eq!(
        row.data.get(&field("version")),
        Some(&Expr::String("1.2.3".to_owned()))
    );
    assert_eq!(row.data.get(&field("abi-major")), Some(&int("0")));
    assert_eq!(row.data.get(&field("abi-minor")), Some(&int("1")));
    assert_eq!(
        row.data.get(&field("target")),
        Some(&Expr::Symbol(Symbol::new("host-registered")))
    );
    assert_eq!(row.data.get(&field("trusted")), Some(&Expr::Bool(true)));
}

#[test]
fn export_rows_encode_resolved_and_failure_states() {
    let resolved = export_row(
        ExportKind::named(ExportKind::CLASS),
        Symbol::new("Widget"),
        Symbol::new("demo-lib"),
        ExportState::Resolved {
            id: RuntimeId::Class(CORE_EXPR_CLASS_ID),
        },
    );
    assert_eq!(
        resolved.data.get(&field("state")),
        Some(&Expr::Symbol(Symbol::new("resolved")))
    );
    assert_eq!(
        resolved.data.get(&field("runtime-kind")),
        Some(&Expr::Symbol(Symbol::new("class")))
    );
    assert_eq!(resolved.data.get(&field("runtime-id")), Some(&int("9")));

    let value = export_row(
        ExportKind::named(ExportKind::VALUE),
        Symbol::new("answer"),
        Symbol::new("demo-lib"),
        ExportState::Resolved {
            id: RuntimeId::Value,
        },
    );
    assert_eq!(
        value.data.get(&field("runtime-kind")),
        Some(&Expr::Symbol(Symbol::new("value")))
    );
    assert!(!value.data.contains_key(&field("runtime-id")));

    let unsupported = export_row(
        ExportKind::named(ExportKind::FUNCTION),
        Symbol::new("future"),
        Symbol::new("demo-lib"),
        ExportState::Unsupported {
            reason: "not implemented".to_owned(),
        },
    );
    assert_eq!(
        unsupported.data.get(&field("state")),
        Some(&Expr::Symbol(Symbol::new("unsupported")))
    );
    assert_eq!(
        unsupported.data.get(&field("reason")),
        Some(&Expr::String("not implemented".to_owned()))
    );

    let invalid = export_row(
        ExportKind::named(ExportKind::FUNCTION),
        Symbol::new("bad"),
        Symbol::new("demo-lib"),
        ExportState::Invalid {
            error: "bad export".to_owned(),
        },
    );
    assert_eq!(
        invalid.data.get(&field("state")),
        Some(&Expr::Symbol(Symbol::new("invalid")))
    );
    assert_eq!(
        invalid.data.get(&field("error")),
        Some(&Expr::String("bad export".to_owned()))
    );
}

#[test]
fn runtime_and_test_rows_keep_live_payloads_out_of_data() {
    let field_value = field("value");
    let field_test = field("test");
    let value = bool_value(true);
    let runtime = runtime_value_row(Symbol::new("function"), 3, Symbol::new("f"), value.clone());
    let plain = plain_value_row(Symbol::new("answer"), value.clone());
    let test = test_row(
        Symbol::new("smoke"),
        Symbol::new("demo-lib"),
        Arc::new(DummyTest),
        vec![Symbol::new("answer"), Symbol::qualified("core", "add")],
    );

    assert_eq!(runtime.live_value(&field_value), Some(&value));
    assert_eq!(plain.live_value(&field_value), Some(&value));
    assert!(!runtime.data.contains_key(&field_value));
    assert!(!plain.data.contains_key(&field_value));
    assert!(test.live_test(&field_test).is_some());
    assert!(!test.data.contains_key(&field_test));
    assert_eq!(
        test.data.get(&field("subjects")),
        Some(&Expr::List(vec![
            Expr::Symbol(Symbol::new("answer")),
            Expr::Symbol(Symbol::qualified("core", "add")),
        ]))
    );
}

#[test]
fn direct_duplicate_class_still_returns_duplicate_export() {
    let mut registry = Registry::default();
    let symbol = Symbol::new("direct-dup-class");
    let class_id = registry
        .register_class_value(symbol.clone(), bool_value(true))
        .unwrap();

    assert!(registry.catalog_runtime_projection_matches(
        &ExportKind::named(ExportKind::CLASS),
        &symbol,
        RuntimeId::Class(class_id),
    ));
    let export_rows = registry.catalog_export_row_count_for_tests();
    let runtime_rows = registry.catalog_runtime_row_count_for_tests();
    let err = registry
        .register_class_value(symbol.clone(), bool_value(false))
        .unwrap_err();

    assert_duplicate_export(err, ExportKind::CLASS, &symbol);
    assert_eq!(registry.classes().get(&symbol), Some(&class_id));
    assert_eq!(registry.catalog_export_row_count_for_tests(), export_rows);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), runtime_rows);
}

#[test]
fn direct_duplicate_test_still_returns_duplicate_export() {
    let mut registry = Registry::default();
    let symbol = Symbol::qualified("direct", "smoke");
    let lib = Symbol::new("direct-lib");
    registry
        .register_test(
            symbol.clone(),
            lib.clone(),
            Arc::new(DummyTest),
            vec![Symbol::new("subject")],
        )
        .unwrap();

    assert!(
        registry
            .catalog_test_registration_matches(&symbol, registry.registered_test(&symbol).unwrap())
    );
    let export_rows = registry.catalog_export_row_count_for_tests();
    let test_rows = registry.catalog_test_row_count_for_tests();
    let err = registry
        .register_test(symbol.clone(), lib.clone(), Arc::new(DummyTest), Vec::new())
        .unwrap_err();

    assert_duplicate_export(err, "test", &symbol);
    assert_eq!(registry.tests_for_lib(&lib), Some(&[symbol.clone()][..]));
    assert_eq!(registry.catalog_export_row_count_for_tests(), export_rows);
    assert_eq!(registry.catalog_test_row_count_for_tests(), test_rows);
}

#[test]
fn failed_duplicate_value_write_leaves_catalog_and_projection_caches_unchanged() {
    let mut registry = Registry::default();
    let symbol = Symbol::new("direct-dup-value");
    let first = bool_value(true);
    registry
        .register_value(symbol.clone(), first.clone())
        .unwrap();

    let export_rows = registry.catalog_export_row_count_for_tests();
    let runtime_rows = registry.catalog_runtime_row_count_for_tests();
    let err = registry
        .register_value(symbol.clone(), bool_value(false))
        .unwrap_err();

    assert_duplicate_export(err, ExportKind::VALUE, &symbol);
    assert_eq!(registry.value_by_symbol(&symbol), Some(&first));
    assert_eq!(registry.catalog_export_row_count_for_tests(), export_rows);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), runtime_rows);
    assert!(registry.catalog_runtime_projection_matches(
        &ExportKind::named(ExportKind::VALUE),
        &symbol,
        RuntimeId::Value,
    ));
}
