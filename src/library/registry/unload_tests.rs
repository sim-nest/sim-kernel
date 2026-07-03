use crate::library::{
    AbiVersion, Dependency, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Registry, Version,
};
use crate::{Claim, ClaimPattern, Cx, Error, FunctionId, LibId, Ref, Result, Symbol, Value};

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

fn load_value_lib_with_id(
    registry: &mut Registry,
    cx: &mut Cx,
    lib: &str,
    export: &str,
) -> (LibId, Symbol, Value) {
    let export = Symbol::new(export);
    let value = bool_value(cx, true);
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![Export::Value {
                symbol: export.clone(),
            }],
            ..manifest(lib, "0.1.0")
        },
        true,
    );
    txn.linker().value(export.clone(), value.clone()).unwrap();
    let lib_id = registry.commit_load(txn).unwrap();
    (lib_id, export, value)
}

fn load_function_lib(
    registry: &mut Registry,
    cx: &mut Cx,
    lib: &str,
    export: &str,
) -> (LibId, Symbol, FunctionId, Value) {
    let export = Symbol::new(export);
    let value = bool_value(cx, true);
    let mut txn = registry.begin_load(
        LibManifest {
            exports: vec![Export::Function {
                symbol: export.clone(),
                function_id: None,
            }],
            ..manifest(lib, "0.1.0")
        },
        true,
    );
    let function_id = txn
        .linker()
        .function_value(export.clone(), value.clone())
        .unwrap();
    let lib_id = registry.commit_load(txn).unwrap();
    (lib_id, export, function_id, value)
}

fn load_manifest_only_lib(registry: &mut Registry, lib: LibManifest) -> LibId {
    let txn = registry.begin_load(lib, true);
    registry.commit_load(txn).unwrap()
}

struct SimpleValueLib;

impl Lib for SimpleValueLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            exports: vec![Export::Value {
                symbol: Symbol::new("cx-unload-value"),
            }],
            ..manifest("cx-unload-lib", "0.1.0")
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> Result<()> {
        linker.value(
            Symbol::new("cx-unload-value"),
            cx.factory().bool(true).unwrap(),
        )
    }
}

#[test]
fn unload_removes_resolution_for_every_loaded_export_kind() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let class_symbol = Symbol::new("unload-class");
    let function_symbol = Symbol::new("unload-function");
    let macro_symbol = Symbol::new("unload-macro");
    let shape_symbol = Symbol::new("unload-shape");
    let codec_symbol = Symbol::new("unload-codec");
    let domain_symbol = Symbol::new("unload-domain");
    let value_symbol = Symbol::new("unload-value");
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
                Export::Value {
                    symbol: value_symbol.clone(),
                },
            ],
            ..manifest("unload-all-lib", "0.1.0")
        },
        true,
    );
    {
        let mut linker = txn.linker();
        linker
            .class_value(class_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .function_value(function_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .macro_value(macro_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .shape_value(shape_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .codec_value(codec_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .number_domain_value(domain_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
        linker
            .value(value_symbol.clone(), bool_value(&mut cx, true))
            .unwrap();
    }
    let lib_id = registry.commit_load(txn).unwrap();

    assert_eq!(registry.unload(lib_id).unwrap(), vec![lib_id]);
    assert!(registry.class_by_symbol(&class_symbol).is_none());
    assert!(registry.function_by_symbol(&function_symbol).is_none());
    assert!(registry.macro_by_symbol(&macro_symbol).is_none());
    assert!(registry.shape_by_symbol(&shape_symbol).is_none());
    assert!(registry.codec_by_symbol(&codec_symbol).is_none());
    assert!(registry.number_domain_by_symbol(&domain_symbol).is_none());
    assert!(registry.value_by_symbol(&value_symbol).is_none());
    assert!(registry.lib(&Symbol::new("unload-all-lib")).is_none());
    registry.assert_projection_caches_match_catalog_for_tests();
}

#[test]
fn unload_refuses_loaded_dependents() {
    let mut registry = Registry::default();
    let dep_id = load_manifest_only_lib(&mut registry, manifest("dep-lib", "0.1.0"));
    let mut user = manifest("user-lib", "0.1.0");
    user.requires.push(Dependency {
        id: Symbol::new("dep-lib"),
        minimum_version: None,
    });
    load_manifest_only_lib(&mut registry, user);

    let err = registry.unload(dep_id).unwrap_err();
    assert!(matches!(
        err,
        Error::LibHasDependents { lib, dependents }
            if lib == Symbol::new("dep-lib")
                && dependents == vec![Symbol::new("user-lib")]
    ));
    assert!(registry.lib(&Symbol::new("dep-lib")).is_some());
    assert!(registry.lib(&Symbol::new("user-lib")).is_some());
}

#[test]
fn unload_cascade_runs_dependents_before_dependencies() {
    let mut registry = Registry::default();
    let dep_id = load_manifest_only_lib(&mut registry, manifest("dep-lib", "0.1.0"));
    let mut mid = manifest("mid-lib", "0.1.0");
    mid.requires.push(Dependency {
        id: Symbol::new("dep-lib"),
        minimum_version: None,
    });
    let mid_id = load_manifest_only_lib(&mut registry, mid);
    let mut top = manifest("top-lib", "0.1.0");
    top.requires.push(Dependency {
        id: Symbol::new("mid-lib"),
        minimum_version: None,
    });
    let top_id = load_manifest_only_lib(&mut registry, top);

    assert_eq!(
        registry.unload_cascade(dep_id).unwrap(),
        vec![top_id, mid_id, dep_id]
    );
    assert!(registry.libs().is_empty());
}

#[test]
fn unload_absent_lib_is_noop() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let (loaded, symbol, _) =
        load_value_lib_with_id(&mut registry, &mut cx, "present-lib", "present-value");

    assert!(registry.unload(LibId(999)).unwrap().is_empty());
    assert_eq!(registry.libs().len(), 1);
    assert!(registry.lib(&Symbol::new("present-lib")).is_some());
    assert!(registry.value_by_symbol(&symbol).is_some());
    assert!(registry.unload_cascade(LibId(999)).unwrap().is_empty());
    assert_eq!(registry.unload(loaded).unwrap(), vec![loaded]);
}

#[test]
fn unload_then_reload_reuses_stable_ids_for_same_load() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let (first_lib, symbol, first_function, _) =
        load_function_lib(&mut registry, &mut cx, "reload-lib", "reload-function");

    assert_eq!(registry.unload(first_lib).unwrap(), vec![first_lib]);
    let (second_lib, second_symbol, second_function, _) =
        load_function_lib(&mut registry, &mut cx, "reload-lib", "reload-function");

    assert_eq!(second_lib, first_lib);
    assert_eq!(second_symbol, symbol);
    assert_eq!(second_function, first_function);
    assert!(registry.function_by_symbol(&symbol).is_some());
}

#[test]
fn unload_removes_fresh_resolution_but_live_value_survives() {
    let mut cx = Cx::stub();
    let mut registry = Registry::default();
    let (lib_id, symbol, live_value) =
        load_value_lib_with_id(&mut registry, &mut cx, "live-lib", "live-value");
    assert_eq!(registry.value_by_symbol(&symbol), Some(&live_value));

    registry.unload(lib_id).unwrap();

    assert!(registry.value_by_symbol(&symbol).is_none());
    assert_eq!(
        live_value.object().as_expr(&mut cx).unwrap(),
        crate::Expr::Bool(true)
    );
}

#[test]
fn cx_unload_retracts_published_load_claims() {
    let mut cx = Cx::stub();
    let lib = SimpleValueLib;
    let lib_id = cx.load_lib(&lib).unwrap();
    let subject = Ref::Symbol(Symbol::new("cx-unload-value"));
    let predicate = Symbol::qualified("core", "exported-by");
    let object = Ref::Symbol(Symbol::new("cx-unload-lib"));
    let pattern = ClaimPattern::exact(subject.clone(), predicate.clone(), object.clone());
    let extra_subject = Ref::Symbol(Symbol::new("cx-unload-extra"));
    let extra_predicate = Symbol::qualified("core", "extra");
    let extra_object = Ref::Symbol(Symbol::new("cx-unload-value"));
    let extra_pattern = ClaimPattern::exact(
        extra_subject.clone(),
        extra_predicate.clone(),
        extra_object.clone(),
    );

    assert_eq!(cx.query_facts(pattern.clone()).unwrap().len(), 1);
    cx.insert_fact_for_lib(
        lib_id,
        Claim::public(extra_subject, extra_predicate, extra_object),
    )
    .unwrap();
    assert_eq!(cx.query_facts(extra_pattern.clone()).unwrap().len(), 1);
    assert_eq!(cx.unload_lib(lib_id).unwrap(), vec![lib_id]);

    assert!(cx.query_facts(pattern).unwrap().is_empty());
    assert!(cx.query_facts(extra_pattern).unwrap().is_empty());
    assert!(
        cx.registry()
            .value_by_symbol(&Symbol::new("cx-unload-value"))
            .is_none()
    );
}

#[test]
fn cx_unload_preserves_duplicate_claims_that_predated_load() {
    let mut cx = Cx::stub();
    let subject = Ref::Symbol(Symbol::new("cx-unload-value"));
    let predicate = Symbol::qualified("core", "exported-by");
    let object = Ref::Symbol(Symbol::new("cx-unload-lib"));
    let pattern = ClaimPattern::exact(subject.clone(), predicate.clone(), object.clone());
    cx.insert_fact(Claim::public(subject, predicate, object))
        .unwrap();

    let lib = SimpleValueLib;
    let lib_id = cx.load_lib(&lib).unwrap();

    assert_eq!(cx.query_facts(pattern.clone()).unwrap().len(), 1);
    cx.unload_lib(lib_id).unwrap();
    assert_eq!(cx.query_facts(pattern).unwrap().len(), 1);
}
