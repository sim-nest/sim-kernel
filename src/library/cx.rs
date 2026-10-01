// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{
    LibId,
    env::Cx,
    error::{Error, Result},
    library::{Lib, LibTarget, LoadedLib, Version},
};

use super::claims::publish_loaded_lib_claims;
use super::loaders::dependency_satisfied;
use std::collections::BTreeMap;

impl Cx {
    /// Atomically activates an already constructed library candidate.
    ///
    /// `None` admits only an absent symbol. `Some(id)` replaces exactly that
    /// loaded leaf while retaining the library and matching export ids. Only
    /// kernel-observable library state is staged; provider effects are outside
    /// this transaction because `Lib::load` receives only `LoadCx` and `Linker`.
    pub fn activate_lib(&mut self, expected: Option<LibId>, lib: &dyn Lib) -> Result<LibId> {
        self.activate_lib_checked(expected, lib, |_| Ok(()), |_| Ok(()))
    }

    /// Same as [`activate_lib`](Self::activate_lib), but `check` runs against
    /// the library's actual committed [`LoadedLib`] while that commit is
    /// still staged, not yet visible outside this call.
    ///
    /// `lib.load` runs exactly once more here than in a caller's own
    /// preflight probe of the same candidate (for example a shadow-context
    /// dry run): nothing before this method's own staged commit can observe
    /// what THIS specific invocation of `Lib::load` actually produces. A
    /// hostile library can return a small, compliant result to a probe load
    /// and a large or otherwise noncompliant one here. `check` is the only
    /// place that result can be rejected before it is real: once this method
    /// returns `Ok`, the activation has committed and a caller cannot safely
    /// report failure without risking a partial effect. If `check` returns
    /// `Err`, this call also returns `Err` and the live registry, datum
    /// store, facts and load ledger are byte-for-byte what they were before
    /// this call -- the same staged-then-conditionally-committed transaction
    /// `activate_lib` itself already used, just with one more fallible step
    /// before the commit.
    ///
    /// `check_manifest` is the FIRST thing that touches the candidate: the library's manifest
    /// is sampled exactly once and `check_manifest` runs on that borrow before any registry
    /// lookup, formatting, iteration or clone of it, so a hostile or stateful manifest is
    /// refused before the kernel does any work proportional to its size. That same sampled
    /// manifest is what is committed (there is no second `Lib::manifest()` call).
    pub fn activate_lib_checked(
        &mut self,
        expected: Option<LibId>,
        lib: &dyn Lib,
        check_manifest: impl FnOnce(&crate::library::LibManifest) -> std::result::Result<(), String>,
        check: impl FnOnce(&LoadedLib) -> std::result::Result<(), String>,
    ) -> Result<LibId> {
        let manifest = lib.manifest();
        check_manifest(&manifest).map_err(Error::Lib)?;
        let current = self.registry.lib(&manifest.id).cloned();
        match (expected, current.as_ref()) {
            (None, None) => {}
            (None, Some(_)) => {
                return Err(Error::DuplicateLib {
                    symbol: manifest.id.clone(),
                });
            }
            (Some(id), Some(loaded)) if loaded.id == id => {}
            (Some(id), _) => {
                return Err(Error::Lib(format!(
                    "activation expected library id {id:?} for {}",
                    manifest.id
                )));
            }
        }

        if expected.is_none() {
            let mut staged = self.library_state();
            // The check runs on the stored committed library BEFORE its claims are published
            // (publication clones and iterates every export), inside the staged transaction.
            let result: Result<LibId> = self.with_library_state(&mut staged, |cx| {
                cx.load_manifest_checked(lib, manifest, check)
            });
            let id = result?;
            self.commit_library_state(staged);
            return Ok(id);
        }

        let loaded = current.expect("validated loaded replacement");
        let id = loaded.id;
        let stable_exports = loaded
            .exports
            .iter()
            .filter_map(|record| match record.state {
                crate::library::ExportState::Resolved { id } => {
                    Some(((record.kind.clone(), record.symbol.clone()), id))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let mut staged = self.library_state();
        let result = self.with_library_state(&mut staged, |cx| {
            for capability in &manifest.capabilities {
                cx.require(capability)?;
            }
            for dependency in &manifest.requires {
                if let Some(found) = cx.registry.manifest_by_symbol(&dependency.id)
                    && !dependency_satisfied(found, dependency)
                {
                    return Err(Error::DependencyVersionMismatch {
                        lib: manifest.id.clone(),
                        dependency: dependency.id.clone(),
                        required: dependency
                            .minimum_version
                            .clone()
                            .unwrap_or_else(|| Version(String::from("0"))),
                        loaded: found.version.clone(),
                    });
                }
            }
            let sequence_floor = cx.registry.catalog_sequence_snapshot();
            cx.registry.unload(id)?;
            cx.remove_load_claims(&[id]);
            cx.registry.retain_sequence_floor(&sequence_floor)?;
            let trusted = matches!(manifest.target, LibTarget::HostRegistered);
            let mut txn = cx.registry.try_begin_load(manifest.clone(), trusted)?;
            txn.lib_id = id;
            txn.stable_exports = stable_exports;
            {
                let mut load_cx = cx.load_cx();
                lib.load(&mut load_cx, &mut txn.linker())?;
            }
            let committed = cx.registry.commit_load(txn)?;
            // Checked by reference on the stored committed library: no clone precedes the
            // check. The clone below exists only to publish claims.
            let stored = cx
                .registry
                .libs()
                .iter()
                .find(|item| item.id == committed)
                .ok_or_else(|| Error::Lib("activated library missing after commit".to_owned()))?;
            check(stored).map_err(Error::Lib)?;
            let loaded = stored.clone();
            let claims = publish_loaded_lib_claims(cx, &loaded)?;
            cx.record_load_claims(committed, claims);
            Ok(committed)
        })?;
        self.commit_library_state(staged);
        Ok(result)
    }

    /// Loads a single library: checks its requested capabilities and dependency
    /// versions, runs its [`Lib::load`](crate::library::Lib::load) against a
    /// load transaction, commits the result, and publishes its load claims.
    pub fn load_lib(&mut self, lib: &dyn Lib) -> Result<LibId> {
        self.load_lib_checked(lib, |_| Ok(()), |_| Ok(()))
    }

    /// [`Self::load_lib`] with two validation hooks. `check_manifest` runs on the single
    /// sampled manifest before anything else touches it; `check` runs on the stored
    /// committed [`LoadedLib`], by reference, after the registry commit and BEFORE its load
    /// claims are published. An `Err` from either is returned as [`Error::Lib`]. The registry
    /// commit has already happened when `check` fails, so callers that need an atomic refusal
    /// run this inside a staged library state (as [`Self::activate_lib_checked`] does) or in
    /// a throwaway context.
    pub fn load_lib_checked(
        &mut self,
        lib: &dyn Lib,
        check_manifest: impl FnOnce(&crate::library::LibManifest) -> std::result::Result<(), String>,
        check: impl FnOnce(&LoadedLib) -> std::result::Result<(), String>,
    ) -> Result<LibId> {
        let manifest = lib.manifest();
        check_manifest(&manifest).map_err(Error::Lib)?;
        self.load_manifest_checked(lib, manifest, check)
    }

    /// The load path shared by [`Self::load_lib_checked`] and the activation transaction:
    /// takes the ALREADY sampled (and already checked) manifest, so `Lib::manifest()` is
    /// never called a second time.
    fn load_manifest_checked(
        &mut self,
        lib: &dyn Lib,
        manifest: crate::library::LibManifest,
        check: impl FnOnce(&LoadedLib) -> std::result::Result<(), String>,
    ) -> Result<LibId> {
        for capability in &manifest.capabilities {
            self.require(capability)?;
        }
        for dependency in &manifest.requires {
            if let Some(loaded_manifest) = self.registry.manifest_by_symbol(&dependency.id)
                && !dependency_satisfied(loaded_manifest, dependency)
            {
                return Err(Error::DependencyVersionMismatch {
                    lib: manifest.id.clone(),
                    dependency: dependency.id.clone(),
                    required: dependency
                        .minimum_version
                        .clone()
                        .unwrap_or_else(|| Version(String::from("0"))),
                    loaded: loaded_manifest.version.clone(),
                });
            }
        }
        let trusted = matches!(manifest.target, LibTarget::HostRegistered);
        let mut txn = self.registry.try_begin_load(manifest, trusted)?;
        {
            let mut load_cx = self.load_cx();
            let mut linker = txn.linker();
            lib.load(&mut load_cx, &mut linker)?;
        }
        let lib_id = self.registry.commit_load(txn)?;
        let stored = self
            .registry
            .libs()
            .iter()
            .find(|loaded| loaded.id == lib_id)
            .ok_or_else(|| Error::Lib("loaded library missing after commit".to_owned()))?;
        check(stored).map_err(Error::Lib)?;
        let loaded = stored.clone();
        let claim_ids = publish_loaded_lib_claims(self, &loaded)?;
        self.record_load_claims(lib_id, claim_ids);
        Ok(lib_id)
    }

    /// Loads several libraries in dependency order, returning their ids in load
    /// order.
    pub fn load_libs(&mut self, libs: &[&dyn Lib]) -> Result<Vec<LibId>> {
        let manifests = libs.iter().map(|lib| lib.manifest()).collect::<Vec<_>>();
        let ordered = self.registry.dependency_order(&manifests)?;
        let mut ids = Vec::with_capacity(ordered.len());

        for manifest in ordered {
            let lib = libs
                .iter()
                .find(|candidate| candidate.manifest().id == manifest.id)
                .ok_or_else(|| {
                    Error::Lib(format!("missing lib implementation for {}", manifest.id))
                })?;
            ids.push(self.load_lib(*lib)?);
        }

        Ok(ids)
    }

    /// Unloads a loaded library by id and retracts the claims published for it.
    ///
    /// Already-live values may continue to exist, but fresh registry resolution
    /// for the unloaded library's exports is removed.
    pub fn unload_lib(&mut self, lib_id: LibId) -> Result<Vec<LibId>> {
        let unloaded = self.registry.unload(lib_id)?;
        self.remove_load_claims(&unloaded);
        Ok(unloaded)
    }

    /// Unloads a library and every loaded dependent in reverse dependency
    /// order, retracting the claims published for each unloaded lib.
    pub fn unload_lib_cascade(&mut self, lib_id: LibId) -> Result<Vec<LibId>> {
        let unloaded = self.registry.unload_cascade(lib_id)?;
        self.remove_load_claims(&unloaded);
        Ok(unloaded)
    }
}
