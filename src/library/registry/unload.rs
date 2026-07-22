use std::collections::BTreeSet;

use crate::{
    Error, LibId, Result, RuntimeId, Symbol,
    library::{ExportState, LoadedLib},
};

use super::{
    DeltaRange, LoadDelta, Registry,
    catalog::{
        SEQ_CASE, SEQ_CLASS, SEQ_CODEC, SEQ_FUNCTION, SEQ_LIB, SEQ_MACRO, SEQ_NUMBER_DOMAIN,
        SEQ_SHAPE, SEQ_SITE, export_key, export_record_key, exports_table, lib_key, libs_table,
        number_binary_op_row, number_ops_table, number_reduction_op_row, number_unary_op_row,
        plain_value_key, promotion_rule_row, promotion_rules_table, runtime_key, runtime_table,
        test_key, tests_table, value_number_binary_op_row, value_number_reduction_op_row,
        value_number_unary_op_row, value_promotion_rule_row, value_promotion_rules_table,
    },
};

impl Registry {
    /// Unloads a single library by stable id.
    ///
    /// This is a bare unload: if any loaded library depends on `lib_id`, the
    /// call refuses with [`Error::LibHasDependents`]. Passing an absent id is a
    /// no-op and returns an empty list. On success the returned list contains
    /// the unloaded id.
    pub fn unload(&mut self, lib_id: LibId) -> Result<Vec<LibId>> {
        let Some(lib) = self.loaded_lib(lib_id) else {
            return Ok(Vec::new());
        };
        let dependents = self.dependent_ids_for(lib_id);
        if !dependents.is_empty() {
            return Err(Error::LibHasDependents {
                lib: lib.manifest.id.clone(),
                dependents: self.symbols_for_lib_ids(&dependents),
            });
        }
        Ok(self.unload_one(lib_id)?.into_iter().collect())
    }

    /// Unloads a library and its dependents in reverse load order.
    ///
    /// Passing an absent id is a no-op and returns an empty list. The returned
    /// ids are ordered by the actual unload sequence, with dependents before
    /// their dependencies.
    pub fn unload_cascade(&mut self, lib_id: LibId) -> Result<Vec<LibId>> {
        if self.loaded_lib(lib_id).is_none() {
            return Ok(Vec::new());
        }
        let mut selected = BTreeSet::new();
        self.collect_dependents(lib_id, &mut selected);
        selected.insert(lib_id);

        let mut ordered = self
            .libs
            .iter()
            .filter_map(|loaded| selected.contains(&loaded.id).then_some(loaded.id))
            .collect::<Vec<_>>();
        ordered.reverse();

        let mut unloaded = Vec::with_capacity(ordered.len());
        for id in ordered {
            if let Some(id) = self.unload_one(id)? {
                unloaded.push(id);
            }
        }
        Ok(unloaded)
    }

    fn unload_one(&mut self, lib_id: LibId) -> Result<Option<LibId>> {
        let Some(index) = self.libs.iter().position(|loaded| loaded.id == lib_id) else {
            return Ok(None);
        };
        let loaded = self.libs.remove(index);
        let delta = self.load_deltas.remove(&lib_id).unwrap_or_default();
        self.load_dependencies.remove(&lib_id);
        for dependencies in self.load_dependencies.values_mut() {
            dependencies.remove(&lib_id);
        }

        self.delete_loaded_catalog_rows(&loaded);
        self.delete_registered_tests_for_lib(&loaded.manifest.id);
        self.remove_number_delta(delta);
        self.rebuild_number_catalog_rows();
        self.rebuild_projection_caches_from_catalog();
        self.recompute_sequence_rows()?;
        Ok(Some(lib_id))
    }

    fn loaded_lib(&self, lib_id: LibId) -> Option<&LoadedLib> {
        self.libs.iter().find(|loaded| loaded.id == lib_id)
    }

    fn dependent_ids_for(&self, lib_id: LibId) -> Vec<LibId> {
        self.load_dependencies
            .iter()
            .filter_map(|(dependent, dependencies)| {
                dependencies.contains(&lib_id).then_some(*dependent)
            })
            .filter(|dependent| self.loaded_lib(*dependent).is_some())
            .collect()
    }

    fn collect_dependents(&self, lib_id: LibId, selected: &mut BTreeSet<LibId>) {
        for dependent in self.dependent_ids_for(lib_id) {
            if selected.insert(dependent) {
                self.collect_dependents(dependent, selected);
            }
        }
    }

    fn symbols_for_lib_ids(&self, ids: &[LibId]) -> Vec<Symbol> {
        ids.iter()
            .filter_map(|id| {
                self.loaded_lib(*id)
                    .map(|loaded| loaded.manifest.id.clone())
            })
            .collect()
    }

    fn delete_loaded_catalog_rows(&mut self, loaded: &LoadedLib) {
        self.catalog
            .delete_row(&libs_table(), &lib_key(&loaded.manifest.id));
        for record in &loaded.exports {
            self.catalog.delete_row(
                &exports_table(),
                &export_catalog_key(&loaded.manifest.id, record),
            );
            if let ExportState::Resolved { id } = record.state {
                self.catalog
                    .delete_row(&runtime_table(), &runtime_catalog_key(id, &record.symbol));
            }
        }
    }

    fn delete_registered_tests_for_lib(&mut self, lib: &Symbol) {
        let test_kind = Symbol::new("test");
        let symbols = self.tests_by_lib.get(lib).cloned().unwrap_or_default();
        for symbol in symbols {
            self.catalog.delete_row(&tests_table(), &test_key(&symbol));
            self.catalog.delete_row(
                &exports_table(),
                &export_record_key(lib, &test_kind, &symbol),
            );
        }
    }

    fn remove_number_delta(&mut self, removed: LoadDelta) {
        drain_range(&mut self.number_unary_ops, removed.number_unary_ops);
        drain_range(&mut self.number_reduction_ops, removed.number_reduction_ops);
        drain_range(&mut self.number_binary_ops, removed.number_binary_ops);
        drain_range(
            &mut self.value_number_unary_ops,
            removed.value_number_unary_ops,
        );
        drain_range(
            &mut self.value_number_reduction_ops,
            removed.value_number_reduction_ops,
        );
        drain_range(
            &mut self.value_number_binary_ops,
            removed.value_number_binary_ops,
        );
        drain_range(&mut self.promotion_rules, removed.promotion_rules);
        drain_range(
            &mut self.value_promotion_rules,
            removed.value_promotion_rules,
        );

        for delta in self.load_deltas.values_mut() {
            delta
                .number_unary_ops
                .adjust_after_removed(removed.number_unary_ops);
            delta
                .number_reduction_ops
                .adjust_after_removed(removed.number_reduction_ops);
            delta
                .number_binary_ops
                .adjust_after_removed(removed.number_binary_ops);
            delta
                .value_number_unary_ops
                .adjust_after_removed(removed.value_number_unary_ops);
            delta
                .value_number_reduction_ops
                .adjust_after_removed(removed.value_number_reduction_ops);
            delta
                .value_number_binary_ops
                .adjust_after_removed(removed.value_number_binary_ops);
            delta
                .promotion_rules
                .adjust_after_removed(removed.promotion_rules);
            delta
                .value_promotion_rules
                .adjust_after_removed(removed.value_promotion_rules);
        }
    }

    fn rebuild_number_catalog_rows(&mut self) {
        clear_table(self, &number_ops_table());
        clear_table(self, &promotion_rules_table());
        clear_table(self, &value_promotion_rules_table());

        for (index, op) in self.number_unary_ops.iter().cloned().enumerate() {
            self.catalog.put_row(number_unary_op_row(index as u64, op));
        }
        for (index, op) in self.number_reduction_ops.iter().cloned().enumerate() {
            self.catalog
                .put_row(number_reduction_op_row(index as u64, op));
        }
        for (index, op) in self.number_binary_ops.iter().cloned().enumerate() {
            self.catalog.put_row(number_binary_op_row(index as u64, op));
        }
        for (index, op) in self.value_number_unary_ops.iter().cloned().enumerate() {
            self.catalog
                .put_row(value_number_unary_op_row(index as u64, op));
        }
        for (index, op) in self.value_number_reduction_ops.iter().cloned().enumerate() {
            self.catalog
                .put_row(value_number_reduction_op_row(index as u64, op));
        }
        for (index, op) in self.value_number_binary_ops.iter().cloned().enumerate() {
            self.catalog
                .put_row(value_number_binary_op_row(index as u64, op));
        }
        for (index, rule) in self.promotion_rules.iter().cloned().enumerate() {
            self.catalog.put_row(promotion_rule_row(index as u64, rule));
        }
        for (index, rule) in self.value_promotion_rules.iter().cloned().enumerate() {
            self.catalog
                .put_row(value_promotion_rule_row(index as u64, rule));
        }
    }

    fn recompute_sequence_rows(&mut self) -> Result<()> {
        for kind in [
            SEQ_LIB,
            SEQ_CLASS,
            SEQ_FUNCTION,
            SEQ_MACRO,
            SEQ_CASE,
            SEQ_SHAPE,
            SEQ_CODEC,
            SEQ_NUMBER_DOMAIN,
            SEQ_SITE,
        ] {
            self.set_catalog_sequence_next(kind, self.sequence_next_after_unload(kind))?;
        }
        Ok(())
    }

    fn sequence_next_after_unload(&self, kind: &'static str) -> u64 {
        let symbol = Symbol::new(kind);
        let mut next = self
            .load_deltas
            .values()
            .filter_map(|delta| delta.sequence_after.get(&symbol).copied())
            .max()
            .unwrap_or(1);
        next = next.max(match kind {
            SEQ_LIB => max_id_next(self.libs.iter().map(|loaded| loaded.id.0)),
            SEQ_CLASS => max_id_next(self.class_symbol_cache.values().map(|id| id.0)),
            SEQ_FUNCTION => max_id_next(self.function_symbol_cache.values().map(|id| id.0)),
            SEQ_MACRO => max_id_next(self.macro_symbol_cache.values().map(|id| id.0)),
            SEQ_SHAPE => max_id_next(self.shape_symbol_cache.values().map(|id| id.0)),
            SEQ_CODEC => max_id_next(self.codec_symbol_cache.values().map(|id| id.0)),
            SEQ_NUMBER_DOMAIN => {
                max_id_next(self.number_domain_symbol_cache.values().map(|id| id.0))
            }
            SEQ_SITE => max_id_next(self.site_symbol_cache.values().map(|id| id.0)),
            SEQ_CASE => 1,
            _ => 1,
        });
        next
    }
}

fn export_catalog_key(lib: &Symbol, record: &crate::ExportRecord) -> Symbol {
    match record.state {
        ExportState::Resolved { .. } => export_key(record.kind.symbol(), &record.symbol),
        _ => export_record_key(lib, record.kind.symbol(), &record.symbol),
    }
}

fn runtime_catalog_key(runtime_id: RuntimeId, symbol: &Symbol) -> Symbol {
    match runtime_id {
        RuntimeId::Class(id) => runtime_key(&Symbol::new(SEQ_CLASS), u64::from(id.0)),
        RuntimeId::Function(id) => runtime_key(&Symbol::new(SEQ_FUNCTION), u64::from(id.0)),
        RuntimeId::Macro(id) => runtime_key(&Symbol::new(SEQ_MACRO), u64::from(id.0)),
        RuntimeId::Shape(id) => runtime_key(&Symbol::new(SEQ_SHAPE), u64::from(id.0)),
        RuntimeId::Codec(id) => runtime_key(&Symbol::new(SEQ_CODEC), u64::from(id.0)),
        RuntimeId::NumberDomain(id) => {
            runtime_key(&Symbol::new(SEQ_NUMBER_DOMAIN), u64::from(id.0))
        }
        RuntimeId::Site(id) => runtime_key(&Symbol::new(SEQ_SITE), u64::from(id.0)),
        RuntimeId::Value => plain_value_key(symbol),
    }
}

fn drain_range<T>(items: &mut Vec<T>, range: DeltaRange) {
    if range.is_empty() {
        return;
    }
    items.drain(range.start..range.end());
}

fn clear_table(registry: &mut Registry, table: &Symbol) {
    let keys = registry
        .catalog
        .rows(table)
        .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    for key in keys {
        registry.catalog.delete_row(table, &key);
    }
}

fn max_id_next(ids: impl Iterator<Item = u32>) -> u64 {
    ids.map(u64::from)
        .max()
        .and_then(|id| id.checked_add(1))
        .unwrap_or(1)
}
