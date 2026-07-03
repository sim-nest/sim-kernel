use std::collections::BTreeSet;

use crate::{
    error::{Error, Result},
    id::{LibId, RuntimeId},
    library::LibManifest,
};

use super::{
    DeltaRange, LoadDelta, Registry, catalog::CatalogRuntimeValue,
    commit_plan::resolve_loaded_lib_plan,
};
use crate::library::transaction::PendingExports;

pub(crate) fn commit_loaded_lib(
    id: LibId,
    registry: &mut Registry,
    manifest: LibManifest,
    trusted: bool,
    pending: PendingExports,
    sequence_before: std::collections::BTreeMap<crate::Symbol, u64>,
) -> Result<()> {
    if registry.libs_by_symbol.contains_key(&manifest.id) {
        return Err(Error::DuplicateLib {
            symbol: manifest.id.clone(),
        });
    }

    let mut manifest_symbols = BTreeSet::new();
    for export in &manifest.exports {
        let key = (export.kind_symbol(), export.symbol().clone());
        if !manifest_symbols.insert(key) {
            return Err(Error::DuplicateExport {
                kind: export.kind(),
                symbol: export.symbol().clone(),
            });
        }
        registry.ensure_export_available(export)?;
    }

    let mut pending_symbols = BTreeSet::new();
    for export in &pending.exports {
        let kind = export.kind_symbol();
        if !pending_symbols.insert((kind.clone(), export.symbol().clone())) {
            return Err(Error::DuplicateExport {
                kind: kind.duplicate_error_kind(),
                symbol: export.symbol().clone(),
            });
        }
        registry.ensure_export_available(export)?;
    }
    for export in &pending.export_records {
        if !pending_symbols.insert((export.kind.clone(), export.symbol.clone())) {
            return Err(Error::DuplicateExport {
                kind: export.kind.duplicate_error_kind(),
                symbol: export.symbol.clone(),
            });
        }
    }

    let plan = resolve_loaded_lib_plan(&manifest, &pending)?;
    let dependencies = manifest
        .requires
        .iter()
        .filter_map(|dependency| registry.libs_by_symbol.get(&dependency.id).copied())
        .collect();
    let delta = LoadDelta::from_pending(registry, &pending, sequence_before);
    registry.commit_loaded_lib_catalog(
        id,
        &manifest,
        trusted,
        &plan.export_records,
        &plan.runtime_values,
        &pending,
    )?;
    registry.libs_by_symbol.insert(manifest.id.clone(), id);
    for runtime in plan.runtime_values {
        apply_runtime_value(registry, runtime);
    }
    for op in pending.number_binary_ops {
        registry.number_binary_ops.push(op);
    }
    for op in pending.value_number_binary_ops {
        registry.value_number_binary_ops.push(op);
    }
    for op in pending.number_unary_ops {
        registry.number_unary_ops.push(op);
    }
    for op in pending.value_number_unary_ops {
        registry.value_number_unary_ops.push(op);
    }
    for op in pending.number_reduction_ops {
        registry.number_reduction_ops.push(op);
    }
    for op in pending.value_number_reduction_ops {
        registry.value_number_reduction_ops.push(op);
    }
    for rule in pending.promotion_rules {
        registry.promotion_rules.push(rule);
    }
    for rule in pending.value_promotion_rules {
        registry.value_promotion_rules.push(rule);
    }
    registry.load_deltas.insert(id, delta);
    registry.load_dependencies.insert(id, dependencies);
    registry.libs.push(crate::library::LoadedLib {
        id,
        manifest,
        exports: plan.export_records,
        trusted,
    });
    Ok(())
}

fn apply_runtime_value(registry: &mut Registry, runtime: CatalogRuntimeValue) {
    let kind = runtime.kind;
    let symbol = runtime.symbol;
    let runtime_id = runtime.runtime_id;
    match runtime_id {
        RuntimeId::Class(id) => {
            registry.class_symbol_cache.insert(symbol.clone(), id);
            registry.class_value_cache.insert(id, runtime.value);
        }
        RuntimeId::Function(id) => {
            registry.function_symbol_cache.insert(symbol.clone(), id);
            registry.function_value_cache.insert(id, runtime.value);
        }
        RuntimeId::Macro(id) => {
            registry.macro_symbol_cache.insert(symbol.clone(), id);
            registry.macro_value_cache.insert(id, runtime.value);
        }
        RuntimeId::Shape(id) => {
            registry.shape_symbol_cache.insert(symbol.clone(), id);
            registry.shape_value_cache.insert(id, runtime.value);
        }
        RuntimeId::Codec(id) => {
            registry.codec_symbol_cache.insert(symbol.clone(), id);
            registry.codec_value_cache.insert(id, runtime.value);
        }
        RuntimeId::NumberDomain(id) => {
            registry.insert_number_domain_value(symbol.clone(), id, runtime.value);
        }
        RuntimeId::Site(id) => {
            registry.site_symbol_cache.insert(symbol.clone(), id);
            registry.site_value_cache.insert(id, runtime.value);
        }
        RuntimeId::Value => {
            registry
                .plain_value_cache
                .insert(symbol.clone(), runtime.value);
        }
    }
    registry.insert_runtime_export(kind, symbol, runtime_id);
}

impl LoadDelta {
    fn from_pending(
        registry: &Registry,
        pending: &PendingExports,
        sequence_before: std::collections::BTreeMap<crate::Symbol, u64>,
    ) -> Self {
        let mut sequence_after = registry.catalog_sequence_snapshot();
        for (kind, before) in sequence_before {
            sequence_after.entry(kind).or_insert(before);
        }
        Self {
            sequence_after,
            number_unary_ops: DeltaRange::new(
                registry.number_unary_ops.len(),
                pending.number_unary_ops.len(),
            ),
            number_reduction_ops: DeltaRange::new(
                registry.number_reduction_ops.len(),
                pending.number_reduction_ops.len(),
            ),
            number_binary_ops: DeltaRange::new(
                registry.number_binary_ops.len(),
                pending.number_binary_ops.len(),
            ),
            value_number_unary_ops: DeltaRange::new(
                registry.value_number_unary_ops.len(),
                pending.value_number_unary_ops.len(),
            ),
            value_number_reduction_ops: DeltaRange::new(
                registry.value_number_reduction_ops.len(),
                pending.value_number_reduction_ops.len(),
            ),
            value_number_binary_ops: DeltaRange::new(
                registry.value_number_binary_ops.len(),
                pending.value_number_binary_ops.len(),
            ),
            promotion_rules: DeltaRange::new(
                registry.promotion_rules.len(),
                pending.promotion_rules.len(),
            ),
            value_promotion_rules: DeltaRange::new(
                registry.value_promotion_rules.len(),
                pending.value_promotion_rules.len(),
            ),
        }
    }
}
