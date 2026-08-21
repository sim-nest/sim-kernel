use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    error::{Error, Result},
    id::LibId,
    library::{Export, ExportRecord, LibManifest, LoadTransaction, Version},
};

use super::Registry;
use super::commit::commit_loaded_lib;
use crate::library::loaders::compare_version_text;
use crate::library::transaction::PendingExports;

impl Registry {
    /// Topologically orders the given manifests so each library's dependencies
    /// load first.
    ///
    /// Already-loaded libraries satisfy dependencies. Errors with
    /// [`DependencyVersionMismatch`](crate::error::Error::DependencyVersionMismatch),
    /// [`MissingDependency`](crate::error::Error::MissingDependency), or
    /// [`CyclicDependency`](crate::error::Error::CyclicDependency) when no order
    /// exists.
    pub fn dependency_order(&self, manifests: &[LibManifest]) -> Result<Vec<LibManifest>> {
        let mut remaining = manifests.to_vec();
        remaining.sort_by(|left, right| left.id.cmp(&right.id));
        let mut loaded = self.libs_by_symbol.keys().cloned().collect::<BTreeSet<_>>();
        let mut loaded_versions = self
            .libs
            .iter()
            .map(|loaded| (loaded.manifest.id.clone(), loaded.manifest.version.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut ordered = Vec::with_capacity(remaining.len());

        while !remaining.is_empty() {
            let mut progressed = false;
            let mut index = 0;
            while index < remaining.len() {
                let ready = remaining[index].requires.iter().all(|dependency| {
                    if !loaded.contains(&dependency.id) {
                        return false;
                    }
                    match (
                        loaded_versions.get(&dependency.id),
                        dependency.minimum_version.as_ref(),
                    ) {
                        (Some(loaded_version), Some(required_version)) => {
                            compare_version_text(&loaded_version.0, &required_version.0)
                                != Ordering::Less
                        }
                        _ => true,
                    }
                });
                if ready {
                    let manifest = remaining.remove(index);
                    loaded.insert(manifest.id.clone());
                    loaded_versions.insert(manifest.id.clone(), manifest.version.clone());
                    ordered.push(manifest);
                    progressed = true;
                } else {
                    index += 1;
                }
            }

            if !progressed {
                let blocked = &remaining[0];
                if let Some(dependency) = blocked.requires.iter().find(|dependency| {
                    loaded_versions
                        .get(&dependency.id)
                        .zip(dependency.minimum_version.as_ref())
                        .is_some_and(|(loaded_version, minimum)| {
                            compare_version_text(&loaded_version.0, &minimum.0) == Ordering::Less
                        })
                }) {
                    return Err(Error::DependencyVersionMismatch {
                        lib: blocked.id.clone(),
                        dependency: dependency.id.clone(),
                        required: dependency
                            .minimum_version
                            .clone()
                            .unwrap_or_else(|| Version(String::from("0"))),
                        loaded: loaded_versions
                            .get(&dependency.id)
                            .cloned()
                            .unwrap_or_else(|| Version(String::from("0"))),
                    });
                }
                let missing = blocked
                    .requires
                    .iter()
                    .find(|dependency| !loaded.contains(&dependency.id))
                    .map(|dependency| dependency.id.clone())
                    .unwrap_or_else(|| blocked.id.clone());
                return Err(if missing == blocked.id {
                    Error::CyclicDependency {
                        symbol: blocked.id.clone(),
                    }
                } else {
                    Error::MissingDependency {
                        lib: blocked.id.clone(),
                        dependency: missing,
                    }
                });
            }
        }

        Ok(ordered)
    }

    /// Starts a [`LoadTransaction`] for a library on a private registry copy,
    /// reserving its stable id.
    ///
    /// Nothing reaches `self` until the transaction is handed to
    /// [`commit_load`](Registry::commit_load).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use sim_kernel::library::{
    ///     AbiVersion, Export, LibManifest, LibTarget, Registry, Version,
    /// };
    /// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
    ///
    /// let mut cx = Cx::new(
    ///     Arc::new(NoopEvalPolicy),
    ///     Arc::new(DefaultFactory),
    ///     sim_kernel::HandleSeed::new(7),
    /// );
    /// let answer = cx.factory().bool(true).unwrap();
    ///
    /// let manifest = LibManifest {
    ///     id: Symbol::new("demo"),
    ///     version: Version("0.1.0".to_owned()),
    ///     abi: AbiVersion { major: 0, minor: 1 },
    ///     target: LibTarget::HostRegistered,
    ///     requires: Vec::new(),
    ///     capabilities: Vec::new(),
    ///     exports: vec![Export::Value { symbol: Symbol::new("answer") }],
    /// };
    ///
    /// let mut registry = Registry::default();
    /// let mut txn = registry.begin_load(manifest, true);
    /// txn.linker().value(Symbol::new("answer"), answer.clone()).unwrap();
    /// // Staged on the transaction; the live registry is still empty.
    /// assert!(registry.value_by_symbol(&Symbol::new("answer")).is_none());
    ///
    /// let id = registry.commit_load(txn).unwrap();
    /// assert!(registry.lib(&Symbol::new("demo")).is_some());
    /// assert_eq!(registry.value_by_symbol(&Symbol::new("answer")), Some(&answer));
    /// let _ = id;
    /// ```
    pub fn begin_load(&self, manifest: LibManifest, trusted: bool) -> LoadTransaction {
        self.try_begin_load(manifest, trusted)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Starts a load transaction, reporting catalog sequence allocation errors.
    pub fn try_begin_load(&self, manifest: LibManifest, trusted: bool) -> Result<LoadTransaction> {
        let mut registry = self.clone();
        let lib_id = registry.try_fresh_lib_id()?;
        Ok(LoadTransaction {
            lib_id,
            manifest,
            trusted,
            registry,
            pending: PendingExports::default(),
        })
    }

    /// Commits a [`LoadTransaction`], folding its staged registrations into this
    /// registry and returning the new library id.
    pub fn commit_load(&mut self, txn: LoadTransaction) -> Result<LibId> {
        let lib_id = txn.lib_id;
        let mut registry = txn.registry;
        let sequence_before = self.catalog_sequence_snapshot();
        commit_loaded_lib(
            txn.lib_id,
            &mut registry,
            txn.manifest,
            txn.trusted,
            txn.pending,
            sequence_before,
        )?;
        *self = registry;
        Ok(lib_id)
    }

    pub(crate) fn ensure_export_available(&self, export: &Export) -> Result<()> {
        let duplicate = self
            .export_symbols
            .get(&export.kind_symbol())
            .is_some_and(|entries| entries.contains_key(export.symbol()));
        if duplicate {
            Err(Error::DuplicateExport {
                kind: export.kind(),
                symbol: export.symbol().clone(),
            })
        } else {
            Ok(())
        }
    }

    pub(crate) fn validate_export_record_against_manifest(
        manifest: &LibManifest,
        record: &ExportRecord,
    ) -> Result<()> {
        let declared = manifest
            .exports
            .iter()
            .any(|export| export.kind_symbol() == record.kind && export.symbol() == &record.symbol);
        if declared {
            Ok(())
        } else {
            Err(Error::UndeclaredExportRecord {
                kind: record.kind.clone(),
                symbol: record.symbol.clone(),
            })
        }
    }
}
