// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

#[test]
fn checked_initial_install_refuses_without_committing_when_check_fails() {
    // The check runs against the ACTUAL committed LoadedLib, inside the
    // still-staged transaction: a caller cannot distinguish "check ran and
    // rejected" from "Lib::load itself failed" by registry side effects --
    // both leave the live state byte-for-byte unchanged.
    let mut cx = Cx::stub();
    let before = cx.registry().catalog_snapshot();
    let error = cx
        .activate_lib_checked(
            None,
            &library("0.1.0", true, false),
            |_| Ok(()),
            |loaded| Err(format!("rejected {} exports", loaded.exports.len())),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Lib(message) if message.contains("rejected")));
    assert_eq!(cx.registry().catalog_snapshot(), before);
    assert!(cx.registry().lib(&Symbol::new("activation-lib")).is_none());
}

#[test]
fn checked_initial_install_commits_when_check_passes() {
    let mut cx = Cx::stub();
    let id = cx
        .activate_lib_checked(
            None,
            &library("0.1.0", true, false),
            |_| Ok(()),
            |loaded| {
                assert_eq!(loaded.exports.len(), 2);
                Ok(())
            },
        )
        .unwrap();
    assert!(cx.registry().lib(&Symbol::new("activation-lib")).is_some());
    assert_eq!(
        cx.registry()
            .lib(&Symbol::new("activation-lib"))
            .unwrap()
            .id,
        id
    );
}

#[test]
fn checked_replacement_refuses_without_committing_when_check_fails() {
    // Same property on the replace path (`expected: Some(id)`), which is its
    // own, separately staged transaction in `activate_lib`/`activate_lib_checked`.
    let mut cx = Cx::stub();
    let id = cx
        .activate_lib(None, &library("0.1.0", true, false))
        .unwrap();
    let function = cx.registry().functions()[&Symbol::new("activation-first")];
    let captured = cx.registry().function_value(function).unwrap().clone();
    let before = cx.registry().catalog_snapshot();
    let error = cx
        .activate_lib_checked(
            Some(id),
            &library("0.2.0", false, false),
            |_| Ok(()),
            |_| Err("rejected replacement".to_owned()),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Lib(message) if message.contains("rejected replacement")));
    assert_eq!(cx.registry().catalog_snapshot(), before);
    assert_eq!(cx.registry().function_value(function), Some(&captured));
}

#[test]
fn the_check_sees_the_stored_committed_library_not_a_clone_on_both_paths() {
    // The check must run on the stored `LoadedLib` BY REFERENCE: a clone made before the
    // check would sit at a different address (and cost an attacker-sized allocation before
    // the bound is known). The staged state is swapped into the live context by value, so
    // the registry's element address is the same one the check observed.
    let mut cx = Cx::stub();
    let mut seen = 0usize;
    let id = cx
        .activate_lib_checked(
            None,
            &library("0.1.0", true, false),
            |_| Ok(()),
            |loaded| {
                seen = loaded as *const _ as usize;
                Ok(())
            },
        )
        .unwrap();
    let stored = cx.registry().lib(&Symbol::new("activation-lib")).unwrap();
    assert_eq!(
        stored as *const _ as usize, seen,
        "fresh install checked a clone"
    );

    let mut seen_replacement = 0usize;
    cx.activate_lib_checked(
        Some(id),
        &library("0.2.0", false, false),
        |_| Ok(()),
        |loaded| {
            seen_replacement = loaded as *const _ as usize;
            Ok(())
        },
    )
    .unwrap();
    let stored = cx.registry().lib(&Symbol::new("activation-lib")).unwrap();
    assert_eq!(
        stored as *const _ as usize, seen_replacement,
        "replacement checked a clone"
    );
}

#[test]
fn the_manifest_hook_runs_first_on_the_single_sampled_manifest_and_refuses_before_any_load() {
    // A stateful library: every `manifest()` call after the first answers a different id. The
    // kernel must sample the manifest ONCE, run the manifest hook on that sample before any
    // lookup/format/clone/commit, and commit that same sample; a hook refusal leaves the
    // registry untouched and `Lib::load` is never reached.
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counting {
        inner: Box<dyn Lib>,
        manifest_calls: AtomicUsize,
        load_calls: AtomicUsize,
    }
    impl Lib for Counting {
        fn manifest(&self) -> LibManifest {
            self.manifest_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.manifest()
        }
        fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> crate::Result<()> {
            self.load_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.load(cx, linker)
        }
    }
    let make = || Counting {
        inner: Box::new(library("0.1.0", true, false)),
        manifest_calls: AtomicUsize::new(0),
        load_calls: AtomicUsize::new(0),
    };

    // Refusal: hook rejects; nothing loaded, registry unchanged, manifest sampled once.
    let mut cx = Cx::stub();
    let before = cx.registry().catalog_snapshot();
    let refused = make();
    let error = cx
        .activate_lib_checked(
            None,
            &refused,
            |_| Err("manifest refused".to_owned()),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Lib(message) if message.contains("manifest refused")));
    assert_eq!(cx.registry().catalog_snapshot(), before);
    assert_eq!(refused.manifest_calls.load(Ordering::SeqCst), 1);
    assert_eq!(refused.load_calls.load(Ordering::SeqCst), 0);

    // Success: still ONE manifest sample on the fresh path (no second `Lib::manifest()`).
    let accepted = make();
    cx.activate_lib_checked(None, &accepted, |_| Ok(()), |_| Ok(()))
        .unwrap();
    assert_eq!(accepted.manifest_calls.load(Ordering::SeqCst), 1);
}
