use crate::library::{
    AbiVersion, Dependency, Export, ExportKind, ExportState, LibManifest, LibTarget, Registry,
    Version,
};
use crate::{
    CORE_NIL_CLASS_ID, CaseId, ClassId, CodecId, Cx, Error, FunctionId, LibId, MacroId,
    NumberDomainId, RuntimeId, ShapeId, SiteId, Symbol, Value,
};

fn manifest(id: &str, version: &str) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: Version(version.to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target: LibTarget::HostRegistered,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: Vec::new(),
    }
}

fn bool_value(cx: &mut Cx, value: bool) -> Value {
    cx.factory().bool(value).unwrap()
}

fn sequence_next(registry: &Registry, kind: &str) -> Option<u64> {
    registry.catalog_sequence_next(&Symbol::new(kind))
}

const REGISTRY_SEQUENCE_KINDS: [&str; 9] = [
    "lib",
    "class",
    "function",
    "macro",
    "case",
    "shape",
    "codec",
    "number-domain",
    "site",
];

fn load_value_lib(registry: &mut Registry, cx: &mut Cx, lib: &str, export: &str) -> Symbol {
    let export = Symbol::new(export);
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![Export::Value {
                symbol: export.clone(),
            }],
            ..manifest(lib, "0.1.0")
        },
        true,
    );
    txn.linker()
        .value(export.clone(), bool_value(cx, true))
        .unwrap();
    registry.commit_load(txn).unwrap();
    export
}

#[test]
fn dependency_order_accepts_equal_and_higher_minimum_versions() {
    let registry = Registry::default();
    let dep = manifest("dep", "2.1.0");
    let mut user = manifest("user", "0.1.0");
    user.requires.push(Dependency {
        id: Symbol::new("dep"),
        minimum_version: Some(Version("2.0.0".to_owned())),
    });

    let ordered = registry
        .dependency_order(&[user.clone(), dep.clone()])
        .unwrap();
    assert_eq!(ordered, vec![dep, user]);
}

#[test]
fn dependency_order_rejects_lower_minimum_versions() {
    let registry = Registry::default();
    let dep = manifest("dep", "1.5.0");
    let mut user = manifest("user", "0.1.0");
    user.requires.push(Dependency {
        id: Symbol::new("dep"),
        minimum_version: Some(Version("2.0.0".to_owned())),
    });

    let err = registry.dependency_order(&[user, dep]).unwrap_err();
    assert!(matches!(
        err,
        Error::DependencyVersionMismatch {
            lib,
            dependency,
            required,
            loaded
        } if lib == Symbol::new("user")
            && dependency == Symbol::new("dep")
            && required == Version("2.0.0".to_owned())
            && loaded == Version("1.5.0".to_owned())
    ));
}

#[test]
fn export_site_round_trips_symbol_and_kind() {
    let symbol = Symbol::qualified("model", "local");
    let export = Export::Site {
        symbol: symbol.clone(),
        runtime_id: Some(RuntimeId::Site(SiteId(7))),
    };

    assert_eq!(export.symbol(), &symbol);
    assert_eq!(export.kind(), "site");
    assert_eq!(export.kind_symbol(), ExportKind::named(ExportKind::SITE));
}

// guards the registry catalog contract
#[test]
fn duplicate_class_export_is_rejected() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let symbol = Symbol::new("dup-class");

    registry
        .register_class_value(symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    let err = registry
        .register_class_value(symbol.clone(), bool_value(&mut cx, false))
        .unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateExport {
            kind: "class",
            symbol: duplicate,
        } if duplicate == symbol
    ));
}

// guards the registry catalog contract
#[test]
fn duplicate_function_export_is_rejected() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let symbol = Symbol::new("dup-function");

    registry
        .register_function_value(symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    let err = registry
        .register_function_value(symbol.clone(), bool_value(&mut cx, false))
        .unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateExport {
            kind: "function",
            symbol: duplicate,
        } if duplicate == symbol
    ));
}

// guards the registry catalog contract
#[test]
fn duplicate_value_export_is_rejected() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let symbol = Symbol::new("dup-value");

    registry
        .register_value(symbol.clone(), bool_value(&mut cx, true))
        .unwrap();
    let err = registry
        .register_value(symbol.clone(), bool_value(&mut cx, false))
        .unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateExport {
            kind: "value",
            symbol: duplicate,
        } if duplicate == symbol
    ));
}

// guards the registry catalog contract
#[test]
fn duplicate_lib_is_rejected() {
    let mut registry = Registry::default();
    let first = registry.begin_load(manifest("dup-lib", "0.1.0"), true);
    registry.commit_load(first).unwrap();

    let second = registry.begin_load(manifest("dup-lib", "0.2.0"), true);
    let err = registry.commit_load(second).unwrap_err();

    assert!(matches!(
        err,
        Error::DuplicateLib { symbol } if symbol == Symbol::new("dup-lib")
    ));
}

// guards the registry catalog contract
#[test]
fn symbol_lookup_matches_id_lookup_for_class_and_function_values() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("lookup-class");
    let function_symbol = Symbol::new("lookup-function");
    let class_value = bool_value(&mut cx, true);
    let function_value = bool_value(&mut cx, false);

    let class_id = registry
        .register_class_value(class_symbol.clone(), class_value.clone())
        .unwrap();
    let function_id = registry
        .register_function_value(function_symbol.clone(), function_value.clone())
        .unwrap();

    assert_eq!(
        registry.class_by_symbol(&class_symbol),
        registry.class_value(class_id)
    );
    assert_eq!(registry.class_value(class_id), Some(&class_value));
    assert_eq!(
        registry.function_by_symbol(&function_symbol),
        registry.function_value(function_id)
    );
    assert_eq!(registry.function_value(function_id), Some(&function_value));
}

// guards the registry catalog contract
#[test]
fn export_symbols_records_runtime_id_for_registered_value() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let symbol = Symbol::new("exported-value");

    registry
        .register_value(symbol.clone(), bool_value(&mut cx, true))
        .unwrap();

    let runtime_id = registry
        .export_symbols()
        .get(&ExportKind::named(ExportKind::VALUE))
        .and_then(|symbols| symbols.get(&symbol));
    assert_eq!(runtime_id, Some(&RuntimeId::Value));
}

// guards the registry catalog contract
#[test]
fn subset_for_libs_drops_exports_outside_selected_libs() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let kept_lib = Symbol::new("kept-lib");
    let dropped_lib = Symbol::new("dropped-lib");
    let kept_export = load_value_lib(&mut registry, &mut cx, "kept-lib", "kept-value");
    let dropped_export = load_value_lib(&mut registry, &mut cx, "dropped-lib", "dropped-value");

    let subset = registry.subset_for_libs(std::slice::from_ref(&kept_lib));

    assert!(subset.lib(&kept_lib).is_some());
    assert!(subset.lib(&dropped_lib).is_none());
    assert!(subset.value_by_symbol(&kept_export).is_some());
    assert!(subset.value_by_symbol(&dropped_export).is_none());
    let value_exports = subset
        .export_symbols()
        .get(&ExportKind::named(ExportKind::VALUE))
        .unwrap();
    assert!(value_exports.contains_key(&kept_export));
    assert!(!value_exports.contains_key(&dropped_export));
}

// guards the registry catalog contract
#[test]
fn number_domain_order_cache_is_invalidated_after_insert() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let later = Symbol::new("z-later-domain");
    let earlier = Symbol::new("a-earlier-domain");

    registry
        .register_number_domain_value(later.clone(), bool_value(&mut cx, true))
        .unwrap();
    let symbols = registry
        .sorted_number_domains()
        .into_iter()
        .map(|(symbol, _)| symbol)
        .collect::<Vec<_>>();
    assert_eq!(symbols, vec![later.clone()]);

    registry
        .register_number_domain_value(earlier.clone(), bool_value(&mut cx, false))
        .unwrap();
    let symbols = registry
        .sorted_number_domains()
        .into_iter()
        .map(|(symbol, _)| symbol)
        .collect::<Vec<_>>();
    assert_eq!(symbols, vec![earlier, later]);
}

// guards the registry catalog contract
#[test]
fn failed_library_commit_leaves_no_partial_runtime_exports() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("partial-class");
    let function_symbol = Symbol::new("partial-function");
    let value_symbol = Symbol::new("partial-value");
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![
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
            ..manifest("partial-lib", "0.1.0")
        },
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
        Error::Lib(message) if message == "value export partial-value has no value"
    ));
    assert!(registry.libs().is_empty());
    assert!(registry.class_by_symbol(&class_symbol).is_none());
    assert!(registry.function_by_symbol(&function_symbol).is_none());
    assert!(registry.value_by_symbol(&value_symbol).is_none());
    assert!(registry.export_symbols().is_empty());
}

#[test]
fn commit_rejects_undeclared_export_records() {
    let mut registry = Registry::default();
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![Export::Function {
                symbol: Symbol::new("declared"),
                function_id: None,
            }],
            ..manifest("declared-lib", "0.1.0")
        },
        true,
    );
    txn.linker()
        .unsupported_export(
            ExportKind::named(ExportKind::CODEC),
            Symbol::new("future-codec"),
            "not implemented",
        )
        .unwrap();

    let err = registry.commit_load(txn).unwrap_err();
    assert!(matches!(
        err,
        Error::UndeclaredExportRecord { kind, symbol }
            if kind == ExportKind::named(ExportKind::CODEC)
                && symbol == Symbol::new("future-codec")
    ));
}

#[test]
fn commit_keeps_declared_unsupported_export_records() {
    let mut registry = Registry::default();
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![Export::Codec {
                symbol: Symbol::new("future-codec"),
                codec_id: None,
            }],
            ..manifest("declared-lib", "0.1.0")
        },
        true,
    );
    txn.linker()
        .unsupported_export(
            ExportKind::named(ExportKind::CODEC),
            Symbol::new("future-codec"),
            "not implemented",
        )
        .unwrap();

    registry.commit_load(txn).unwrap();
    let loaded = registry.lib(&Symbol::new("declared-lib")).unwrap();
    assert!(loaded.exports.iter().any(|record| {
        record.kind == ExportKind::named(ExportKind::CODEC)
            && record.symbol == Symbol::new("future-codec")
            && matches!(record.state, ExportState::Unsupported { .. })
    }));
}

#[test]
fn commit_keeps_declared_open_export_records() {
    let mut registry = Registry::default();
    let kind = ExportKind::new(Symbol::qualified("surface", "projection"));
    let declared = Symbol::new("declared-surface");
    let unsupported = Symbol::new("unsupported-surface");
    let invalid = Symbol::new("invalid-surface");
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![
                Export::Open {
                    kind: kind.clone(),
                    symbol: declared.clone(),
                },
                Export::Open {
                    kind: kind.clone(),
                    symbol: unsupported.clone(),
                },
                Export::Open {
                    kind: kind.clone(),
                    symbol: invalid.clone(),
                },
            ],
            ..manifest("open-export-lib", "0.1.0")
        },
        true,
    );
    {
        let mut linker = txn.linker();
        linker
            .declare_export(kind.clone(), declared.clone())
            .unwrap();
        linker
            .unsupported_export(kind.clone(), unsupported.clone(), "not available")
            .unwrap();
        linker
            .invalid_export(kind.clone(), invalid.clone(), "bad declaration")
            .unwrap();
    }

    registry.commit_load(txn).unwrap();
    let loaded = registry.lib(&Symbol::new("open-export-lib")).unwrap();

    assert!(loaded.exports.iter().any(|record| {
        record.kind == kind && record.symbol == declared && record.state == ExportState::Declared
    }));
    assert!(loaded.exports.iter().any(|record| {
        record.kind == kind
            && record.symbol == unsupported
            && matches!(record.state, ExportState::Unsupported { .. })
    }));
    assert!(loaded.exports.iter().any(|record| {
        record.kind == kind
            && record.symbol == invalid
            && matches!(record.state, ExportState::Invalid { .. })
    }));
    assert!(!registry.export_symbols().contains_key(&kind));
}

// guards the registry catalog contract
#[test]
fn catalog_backed_sequences_preserve_representative_lib_load_ids() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("loaded-class");
    let function_symbol = Symbol::new("loaded-function");
    let value_symbol = Symbol::new("loaded-value");
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![
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
            ..manifest("loaded-lib", "0.1.0")
        },
        true,
    );

    {
        let mut linker = txn.linker();
        assert_eq!(linker.lib_id(), LibId(1));

        let class_id = linker
            .class_value(class_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        let function_id = linker
            .function_value(function_symbol.clone(), bool_value(&mut cx, false))
            .unwrap();
        linker
            .value(value_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();

        assert_eq!(class_id, CORE_NIL_CLASS_ID);
        assert_eq!(function_id, FunctionId(1));
    }

    assert_eq!(registry.commit_load(txn).unwrap(), LibId(1));
    assert_eq!(
        registry.classes().get(&class_symbol),
        Some(&CORE_NIL_CLASS_ID)
    );
    assert_eq!(
        registry.functions().get(&function_symbol),
        Some(&FunctionId(1))
    );
    assert!(registry.value_by_symbol(&value_symbol).is_some());
    for kind in ["lib", "class", "function"] {
        assert_eq!(sequence_next(&registry, kind), Some(2));
    }
}

// guards the registry catalog contract
#[test]
fn class_with_id_still_reserves_next_class_id() {
    let registry = Registry::default();
    let mut txn = registry.begin_load(manifest("reserved-class-lib", "0.1.0"), true);

    let mut linker = txn.linker();
    assert_eq!(
        linker
            .class_with_id(Symbol::new("explicit-class"), ClassId(100))
            .unwrap(),
        ClassId(100)
    );
    assert_eq!(sequence_next(linker.registry(), "class"), Some(101));
    assert_eq!(
        linker.class(Symbol::new("after-explicit-class")).unwrap(),
        ClassId(101)
    );
    assert_eq!(sequence_next(linker.registry(), "class"), Some(102));
}

// guards the registry catalog contract
#[test]
fn sequence_rows_show_expected_next_value() {
    let mut registry = Registry::default();

    for kind in REGISTRY_SEQUENCE_KINDS {
        assert_eq!(sequence_next(&registry, kind), Some(1));
    }

    assert_eq!(registry.fresh_lib_id(), LibId(1));
    assert_eq!(registry.fresh_class_id(), CORE_NIL_CLASS_ID);
    assert_eq!(registry.fresh_function_id(), FunctionId(1));
    assert_eq!(registry.fresh_macro_id(), MacroId(1));
    assert_eq!(registry.fresh_case_id(), CaseId(1));
    assert_eq!(registry.fresh_shape_id(), ShapeId(1));
    assert_eq!(registry.fresh_codec_id(), CodecId(1));
    assert_eq!(registry.fresh_number_domain_id(), NumberDomainId(1));
    assert_eq!(registry.fresh_site_id(), SiteId(1));

    for kind in REGISTRY_SEQUENCE_KINDS {
        assert_eq!(sequence_next(&registry, kind), Some(2));
    }
}
