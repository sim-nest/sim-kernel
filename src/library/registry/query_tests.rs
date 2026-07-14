use std::sync::Arc;

use crate::{
    AbiVersion, Cx, Export, ExportKind, LibManifest, LibTarget, Registry, Result, RuntimeId,
    Symbol, Test, TestReport, Value, Version,
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

fn assert_same_ref(left: Option<&Value>, right: Option<&Value>) {
    let left = left.expect("left lookup should return a value");
    let right = right.expect("right lookup should return a value");
    assert!(std::ptr::eq(left, right));
}

struct DummyTest;

impl Test for DummyTest {
    fn symbol(&self) -> Symbol {
        Symbol::new("query-cache-test")
    }

    fn lib(&self) -> Symbol {
        Symbol::new("query-cache-lib")
    }

    fn describe(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().nil()
    }

    fn run(&self, _cx: &mut Cx) -> Result<TestReport> {
        Ok(TestReport::from_result(self.symbol(), true, None))
    }
}

fn load_value_lib(registry: &mut Registry, cx: &mut Cx, lib: &str, export: &str) -> Symbol {
    let symbol = Symbol::new(export);
    let mut txn = registry.begin_load(
        manifest(
            lib,
            vec![Export::Value {
                symbol: symbol.clone(),
            }],
        ),
        true,
    );
    txn.linker()
        .value(symbol.clone(), bool_value(cx, true))
        .unwrap();
    registry.commit_load(txn).unwrap();
    symbol
}

#[test]
fn direct_typed_lookup_apis_return_borrowed_values() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();

    let class_symbol = Symbol::new("direct-query-class");
    let function_symbol = Symbol::new("direct-query-function");
    let macro_symbol = Symbol::new("direct-query-macro");
    let shape_symbol = Symbol::new("direct-query-shape");
    let codec_symbol = Symbol::new("direct-query-codec");
    let domain_symbol = Symbol::new("direct-query-domain");
    let site_symbol = Symbol::new("direct-query-site");
    let value_symbol = Symbol::new("direct-query-value");
    let value = bool_value(&mut cx, true);

    let class_id = registry
        .register_class_value(class_symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    let function_id = registry
        .register_function_value(function_symbol.clone(), bool_value(&mut cx, false))
        .unwrap();
    let macro_id = registry
        .register_macro_value(macro_symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    let shape_id = registry
        .register_shape_value(shape_symbol.clone(), bool_value(&mut cx, false))
        .unwrap();
    let codec_id = registry
        .register_codec_value(codec_symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    let domain_id = registry
        .register_number_domain_value(domain_symbol.clone(), bool_value(&mut cx, false))
        .unwrap();
    let site_id = registry
        .register_site_value(site_symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    registry
        .register_value(value_symbol.clone(), value.clone())
        .unwrap();

    assert_same_ref(
        registry.class_value(class_id),
        registry.class_by_symbol(&class_symbol),
    );
    assert_same_ref(
        registry.function_value(function_id),
        registry.function_by_symbol(&function_symbol),
    );
    assert_same_ref(
        registry.macro_value(macro_id),
        registry.macro_by_symbol(&macro_symbol),
    );
    assert_same_ref(
        registry.shape_value(shape_id),
        registry.shape_by_symbol(&shape_symbol),
    );
    assert_same_ref(
        registry.codec_value(codec_id),
        registry.codec_by_symbol(&codec_symbol),
    );
    assert_same_ref(
        registry.number_domain_value(domain_id),
        registry.number_domain_by_symbol(&domain_symbol),
    );
    assert_same_ref(
        registry.site_value(site_id),
        registry.site_by_symbol(&site_symbol),
    );
    let first = registry.value_by_symbol(&value_symbol).unwrap();
    let second = registry.value_by_symbol(&value_symbol).unwrap();
    assert!(std::ptr::eq(first, second));
    assert_eq!(first, &value);
}

#[test]
fn loaded_lib_typed_lookup_apis_return_borrowed_values() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();

    let class_symbol = Symbol::new("loaded-query-class");
    let function_symbol = Symbol::new("loaded-query-function");
    let macro_symbol = Symbol::new("loaded-query-macro");
    let shape_symbol = Symbol::new("loaded-query-shape");
    let codec_symbol = Symbol::new("loaded-query-codec");
    let domain_symbol = Symbol::new("loaded-query-domain");
    let site_symbol = Symbol::new("loaded-query-site");
    let value_symbol = Symbol::new("loaded-query-value");
    let value = bool_value(&mut cx, true);
    let mut txn = registry.begin_load(
        manifest(
            "loaded-query-lib",
            vec![
                Export::Class {
                    symbol: class_symbol.clone(),
                    class_id: None,
                },
                Export::Function {
                    symbol: function_symbol.clone(),
                    function_id: None,
                },
                Export::Macro {
                    symbol: macro_symbol.clone(),
                    macro_id: None,
                },
                Export::Shape {
                    symbol: shape_symbol.clone(),
                    shape_id: None,
                },
                Export::Codec {
                    symbol: codec_symbol.clone(),
                    codec_id: None,
                },
                Export::NumberDomain {
                    symbol: domain_symbol.clone(),
                    number_domain_id: None,
                },
                Export::Site {
                    symbol: site_symbol.clone(),
                    runtime_id: None,
                },
                Export::Value {
                    symbol: value_symbol.clone(),
                },
            ],
        ),
        true,
    );

    let (class_id, function_id, macro_id, shape_id, codec_id, domain_id, site_id) = {
        let mut linker = txn.linker();
        let class_id = linker
            .class_value(class_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        let function_id = linker
            .function_value(function_symbol.clone(), bool_value(&mut cx, false))
            .unwrap();
        let macro_id = linker
            .macro_value(macro_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        let shape_id = linker
            .shape_value(shape_symbol.clone(), bool_value(&mut cx, false))
            .unwrap();
        let codec_id = linker
            .codec_value(codec_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        let domain_id = linker
            .number_domain_value(domain_symbol.clone(), bool_value(&mut cx, false))
            .unwrap();
        let site_id = linker
            .site_value(site_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker.value(value_symbol.clone(), value.clone()).unwrap();
        (
            class_id,
            function_id,
            macro_id,
            shape_id,
            codec_id,
            domain_id,
            site_id,
        )
    };
    registry.commit_load(txn).unwrap();

    assert_same_ref(
        registry.class_value(class_id),
        registry.class_by_symbol(&class_symbol),
    );
    assert_same_ref(
        registry.function_value(function_id),
        registry.function_by_symbol(&function_symbol),
    );
    assert_same_ref(
        registry.macro_value(macro_id),
        registry.macro_by_symbol(&macro_symbol),
    );
    assert_same_ref(
        registry.shape_value(shape_id),
        registry.shape_by_symbol(&shape_symbol),
    );
    assert_same_ref(
        registry.codec_value(codec_id),
        registry.codec_by_symbol(&codec_symbol),
    );
    assert_same_ref(
        registry.number_domain_value(domain_id),
        registry.number_domain_by_symbol(&domain_symbol),
    );
    assert_same_ref(
        registry.site_value(site_id),
        registry.site_by_symbol(&site_symbol),
    );
    let first = registry.value_by_symbol(&value_symbol).unwrap();
    let second = registry.value_by_symbol(&value_symbol).unwrap();
    assert!(std::ptr::eq(first, second));
    assert_eq!(first, &value);
}

#[test]
fn export_site_value_registered_through_linker_is_queryable_by_symbol_and_id() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let site_symbol = Symbol::qualified("model", "fixture");
    let site_value = bool_value(&mut cx, true);
    let mut txn = registry.begin_load(
        manifest(
            "export-site-query-lib",
            vec![Export::Site {
                symbol: site_symbol.clone(),
                runtime_id: None,
            }],
        ),
        true,
    );
    let site_id = txn
        .linker()
        .site_value(site_symbol.clone(), site_value.clone())
        .unwrap();

    registry.commit_load(txn).unwrap();

    assert!(matches!(site_id, RuntimeId::Site(_)));
    assert_eq!(registry.site_value(site_id), Some(&site_value));
    assert_eq!(registry.site_by_symbol(&site_symbol), Some(&site_value));
    assert_eq!(
        registry.export_symbol_for_value(&site_value),
        Some(site_symbol.clone())
    );
    assert_eq!(
        registry
            .export_symbols()
            .get(&ExportKind::named(ExportKind::SITE))
            .and_then(|symbols| symbols.get(&site_symbol)),
        Some(&site_id)
    );
}

#[test]
fn rebuilding_projection_caches_from_catalog_preserves_query_results() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("cache-rebuild-class");
    let function_symbol = Symbol::new("cache-rebuild-function");
    let direct_value_symbol = Symbol::new("cache-rebuild-direct-value");
    let loaded_lib = Symbol::new("cache-rebuild-lib");
    let loaded_value_symbol = load_value_lib(
        &mut registry,
        &mut cx,
        "cache-rebuild-lib",
        "cache-rebuild-value",
    );
    let class_value = bool_value(&mut cx, true);
    let function_value = bool_value(&mut cx, false);
    let direct_value = bool_value(&mut cx, true);
    let class_id = registry
        .register_class_value(class_symbol.clone(), class_value.clone())
        .unwrap();
    let function_id = registry
        .register_function_value(function_symbol.clone(), function_value.clone())
        .unwrap();
    registry
        .register_value(direct_value_symbol.clone(), direct_value.clone())
        .unwrap();
    let test_symbol = Symbol::new("cache-rebuild-test");
    let test_lib = Symbol::new("query-cache-lib");
    registry
        .register_test(
            test_symbol.clone(),
            test_lib.clone(),
            Arc::new(DummyTest),
            vec![class_symbol.clone(), loaded_value_symbol.clone()],
        )
        .unwrap();

    let export_symbols = registry.export_symbols().clone();
    let classes = registry.classes().clone();
    let functions = registry.functions().clone();
    let tests_for_lib = registry.tests_for_lib(&test_lib).map(<[_]>::to_vec);
    let loaded_value = registry.value_by_symbol(&loaded_value_symbol).cloned();

    registry.rebuild_projection_caches_from_catalog();
    registry.assert_projection_caches_match_catalog_for_tests();

    assert_eq!(registry.export_symbols(), &export_symbols);
    assert_eq!(registry.classes(), &classes);
    assert_eq!(registry.functions(), &functions);
    assert_eq!(registry.class_value(class_id), Some(&class_value));
    assert_eq!(registry.function_value(function_id), Some(&function_value));
    assert_eq!(
        registry.value_by_symbol(&direct_value_symbol),
        Some(&direct_value)
    );
    assert_eq!(
        registry.value_by_symbol(&loaded_value_symbol).cloned(),
        loaded_value
    );
    assert!(registry.lib(&loaded_lib).is_some());
    assert_eq!(
        registry.tests_for_lib(&test_lib).map(<[_]>::to_vec),
        tests_for_lib
    );
    assert_eq!(
        registry
            .registered_test(&test_symbol)
            .map(|test| &test.subjects),
        Some(&vec![class_symbol, loaded_value_symbol])
    );
}

#[test]
fn subset_for_libs_rebuilds_projection_caches_from_catalog() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let kept_lib = Symbol::new("subset-cache-kept-lib");
    let dropped_lib = Symbol::new("subset-cache-dropped-lib");
    let kept_value = load_value_lib(
        &mut registry,
        &mut cx,
        "subset-cache-kept-lib",
        "subset-cache-kept-value",
    );
    let dropped_value = load_value_lib(
        &mut registry,
        &mut cx,
        "subset-cache-dropped-lib",
        "subset-cache-dropped-value",
    );

    let subset = registry.subset_for_libs(std::slice::from_ref(&kept_lib));

    subset.assert_projection_caches_match_catalog_for_tests();
    assert!(subset.lib(&kept_lib).is_some());
    assert!(subset.lib(&dropped_lib).is_none());
    assert!(subset.value_by_symbol(&kept_value).is_some());
    assert!(subset.value_by_symbol(&dropped_value).is_none());
    let value_exports = subset
        .export_symbols()
        .get(&ExportKind::named(ExportKind::VALUE))
        .unwrap();
    assert!(value_exports.contains_key(&kept_value));
    assert!(!value_exports.contains_key(&dropped_value));
}

#[test]
#[should_panic(expected = "projection cache class_symbol_cache mismatch")]
fn projection_cache_mismatch_panics_in_debug_helper() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("cache-mismatch-class");
    let class_id = registry
        .register_class_value(class_symbol.clone(), bool_value(&mut cx, true))
        .unwrap();

    registry
        .class_symbol_cache
        .insert(Symbol::new("cache-mismatch-wrong-class"), class_id);
    registry.assert_projection_caches_match_catalog_for_tests();
}
