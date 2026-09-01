use crate::{Cx, Error, LibId, Symbol};

use super::{AbiVersion, Dependency, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Version};

struct ActivationLib {
    manifest: LibManifest,
    first: bool,
    fail: bool,
}

impl Lib for ActivationLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }
    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> crate::Result<()> {
        linker.function_value(
            Symbol::new("activation-first"),
            cx.factory().bool(self.first)?,
        )?;
        if self.fail {
            return Err(Error::Lib("injected activation failure".to_owned()));
        }
        linker.class_value(
            Symbol::new("activation-second"),
            cx.factory().bool(!self.first)?,
        )?;
        Ok(())
    }
}

fn library(version: &str, first: bool, fail: bool) -> ActivationLib {
    ActivationLib {
        manifest: LibManifest {
            id: Symbol::new("activation-lib"),
            version: Version(version.to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![
                Export::Function {
                    symbol: Symbol::new("activation-first"),
                    function_id: None,
                },
                Export::Class {
                    symbol: Symbol::new("activation-second"),
                    class_id: None,
                },
            ],
        },
        first,
        fail,
    }
}

#[test]
fn replacement_is_atomic_and_preserves_stable_ids() {
    let mut cx = Cx::stub();
    let id = cx
        .activate_lib(None, &library("0.1.0", true, false))
        .unwrap();
    let function = cx.registry().functions()[&Symbol::new("activation-first")];
    let class = cx.registry().classes()[&Symbol::new("activation-second")];
    let captured = cx.registry().function_value(function).unwrap().clone();
    let before = cx.registry().catalog_snapshot();
    assert!(
        cx.activate_lib(Some(id), &library("0.2.0", false, true))
            .is_err()
    );
    assert_eq!(cx.registry().catalog_snapshot(), before);
    assert_eq!(cx.registry().function_value(function), Some(&captured));
    assert_eq!(
        cx.activate_lib(Some(id), &library("0.2.0", false, false))
            .unwrap(),
        id
    );
    assert_eq!(
        cx.registry().functions()[&Symbol::new("activation-first")],
        function
    );
    assert_eq!(
        cx.registry().classes()[&Symbol::new("activation-second")],
        class
    );
    assert_ne!(cx.registry().function_value(function), Some(&captured));
}

#[test]
fn replacement_refuses_wrong_id_and_dependents_without_mutation() {
    let mut cx = Cx::stub();
    let original = library("0.1.0", true, false);
    let id = cx.activate_lib(None, &original).unwrap();
    assert!(cx.activate_lib(Some(LibId(id.0 + 1)), &original).is_err());
    struct Dependent(LibManifest);
    impl Lib for Dependent {
        fn manifest(&self) -> LibManifest {
            self.0.clone()
        }
        fn load(&self, _: &mut LoadCx, _: &mut Linker) -> crate::Result<()> {
            Ok(())
        }
    }
    let mut manifest = library("0.1.0", true, false).manifest;
    manifest.id = Symbol::new("activation-dependent");
    manifest.exports.clear();
    manifest.requires.push(Dependency {
        id: Symbol::new("activation-lib"),
        minimum_version: None,
    });
    cx.load_lib(&Dependent(manifest)).unwrap();
    let before = cx.registry().catalog_snapshot();
    assert!(matches!(
        cx.activate_lib(Some(id), &library("0.2.0", false, false)),
        Err(Error::LibHasDependents { .. })
    ));
    assert_eq!(cx.registry().catalog_snapshot(), before);
}
