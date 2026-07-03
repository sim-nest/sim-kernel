use crate::{
    AbiVersion, Cx, Error, Export, LibManifest, LibTarget, NumberBinaryOp, NumberLiteral,
    PromotionRule, Registry, Result, Symbol, Value, ValuePromotionRule, Version,
};

fn manifest(id: &str, exports: Vec<Export>) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: Version(env!("CARGO_PKG_VERSION").to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target: LibTarget::HostRegistered,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports,
    }
}

fn bool_value(cx: &mut Cx, value: bool) -> Value {
    cx.factory().bool(value).unwrap()
}

fn assert_no_loaded_catalog_rows(registry: &Registry) {
    assert_eq!(registry.catalog_lib_row_count_for_tests(), 0);
    assert_eq!(registry.catalog_export_row_count_for_tests(), 0);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), 0);
    assert_eq!(registry.catalog_number_op_row_count_for_tests(), 0);
    assert_eq!(registry.catalog_promotion_rule_row_count_for_tests(), 0);
    assert_eq!(
        registry.catalog_value_promotion_rule_row_count_for_tests(),
        0
    );
}

fn dummy_number_binary(cx: &mut Cx, _left: NumberLiteral, _right: NumberLiteral) -> Result<Value> {
    cx.factory().nil()
}

fn dummy_promote(_cx: &mut Cx, literal: NumberLiteral) -> Result<NumberLiteral> {
    Ok(literal)
}

fn dummy_value_promote(_cx: &mut Cx, value: Value) -> Result<Value> {
    Ok(value)
}

#[test]
fn duplicate_manifest_export_leaves_no_catalog_rows() {
    let mut registry = Registry::default();
    let symbol = Symbol::new("dup-manifest-value");
    let txn = registry.begin_load(
        manifest(
            "dup-manifest-lib",
            vec![
                Export::Value {
                    symbol: symbol.clone(),
                },
                Export::Value {
                    symbol: symbol.clone(),
                },
            ],
        ),
        true,
    );

    let err = registry.commit_load(txn).unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateExport {
            kind: "value",
            symbol: duplicate,
        } if duplicate == symbol
    ));
    assert_no_loaded_catalog_rows(&registry);
}

#[test]
fn duplicate_pending_export_leaves_no_catalog_rows() {
    let mut registry = Registry::default();
    let symbol = Symbol::new("dup-pending-value");
    let mut txn = registry.begin_load(
        manifest(
            "dup-pending-lib",
            vec![Export::Value {
                symbol: symbol.clone(),
            }],
        ),
        true,
    );
    {
        let mut linker = txn.linker();
        linker.value_export(symbol.clone()).unwrap();
        linker.value_export(symbol.clone()).unwrap();
    }

    let err = registry.commit_load(txn).unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateExport {
            kind: "value",
            symbol: duplicate,
        } if duplicate == symbol
    ));
    assert_no_loaded_catalog_rows(&registry);
}

#[test]
fn failed_load_after_first_export_leaves_no_catalog_rows() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("catalog-partial-class");
    let function_symbol = Symbol::new("catalog-partial-function");
    let value_symbol = Symbol::new("catalog-partial-value");
    let mut txn = registry.begin_load(
        manifest(
            "catalog-partial-lib",
            vec![
                Export::Class {
                    symbol: class_symbol.clone(),
                    class_id: None,
                },
                Export::Function {
                    symbol: function_symbol.clone(),
                    function_id: None,
                },
                Export::Value {
                    symbol: value_symbol.clone(),
                },
            ],
        ),
        true,
    );
    {
        let mut linker = txn.linker();
        linker
            .class_value(class_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .function_value(function_symbol.clone(), bool_value(&mut cx, false))
            .unwrap();
        linker.value_export(value_symbol.clone()).unwrap();
    }

    let err = registry.commit_load(txn).unwrap_err();

    assert!(matches!(
        err,
        Error::Lib(message) if message == "value export catalog-partial-value has no value"
    ));
    assert_no_loaded_catalog_rows(&registry);
    assert!(registry.classes().is_empty());
    assert!(registry.functions().is_empty());
    assert!(registry.value_by_symbol(&value_symbol).is_none());
}

#[test]
fn successful_load_makes_catalog_rows_match_projection_caches() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("catalog-loaded-class");
    let function_symbol = Symbol::new("catalog-loaded-function");
    let value_symbol = Symbol::new("catalog-loaded-value");
    let domain = Symbol::new("test-domain");
    let mut txn = registry.begin_load(
        manifest(
            "catalog-loaded-lib",
            vec![
                Export::Class {
                    symbol: class_symbol.clone(),
                    class_id: None,
                },
                Export::Function {
                    symbol: function_symbol.clone(),
                    function_id: None,
                },
                Export::Value {
                    symbol: value_symbol.clone(),
                },
            ],
        ),
        true,
    );
    {
        let mut linker = txn.linker();
        linker
            .class_value(class_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .function_value(function_symbol.clone(), bool_value(&mut cx, false))
            .unwrap();
        linker
            .value(value_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker.number_binary_op(NumberBinaryOp {
            operator: Symbol::qualified("math", "add"),
            left_domain: domain.clone(),
            right_domain: domain.clone(),
            cost: 1,
            apply: dummy_number_binary,
        });
        linker.promotion_rule(PromotionRule {
            from_domain: domain.clone(),
            to_domain: Symbol::new("promoted-domain"),
            cost: 2,
            convert: dummy_promote,
        });
        linker.value_promotion_rule(ValuePromotionRule {
            from_domain: domain,
            to_domain: Symbol::new("value-promoted-domain"),
            cost: 3,
            convert: dummy_value_promote,
        });
    }

    registry.commit_load(txn).unwrap();

    registry.assert_catalog_projection_caches_for_tests();
    assert_eq!(registry.catalog_lib_row_count_for_tests(), 1);
    assert_eq!(registry.catalog_export_row_count_for_tests(), 3);
    assert_eq!(registry.catalog_runtime_row_count_for_tests(), 3);
    assert_eq!(registry.catalog_number_op_row_count_for_tests(), 1);
    assert_eq!(registry.catalog_promotion_rule_row_count_for_tests(), 1);
    assert_eq!(
        registry.catalog_value_promotion_rule_row_count_for_tests(),
        1
    );
}
