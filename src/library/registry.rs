mod catalog;
mod commit;
mod commit_plan;
mod load;
mod overlay;
#[cfg(test)]
mod overlay_tests;
mod query;
#[cfg(test)]
mod query_tests;
mod register;
mod unload;
#[cfg(test)]
mod unload_tests;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    catalog::CatalogStore,
    id::{
        ClassId, CodecId, FunctionId, LibId, MacroId, NumberDomainId, RuntimeId, ShapeId, SiteId,
        Symbol,
    },
    number_domain::{
        NumberBinaryOp, NumberReductionOp, NumberUnaryOp, ValueNumberBinaryOp,
        ValueNumberReductionOp, ValueNumberUnaryOp, ValuePromotionRule,
    },
    value::Value,
};

use super::{LoadedLib, RegisteredTest};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LoadDelta {
    sequence_after: BTreeMap<Symbol, u64>,
    number_unary_ops: DeltaRange,
    number_reduction_ops: DeltaRange,
    number_binary_ops: DeltaRange,
    value_number_unary_ops: DeltaRange,
    value_number_reduction_ops: DeltaRange,
    value_number_binary_ops: DeltaRange,
    promotion_rules: DeltaRange,
    value_promotion_rules: DeltaRange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeltaRange {
    start: usize,
    len: usize,
}

impl DeltaRange {
    pub(crate) fn new(start: usize, len: usize) -> Self {
        Self { start, len }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.len == 0
    }

    pub(crate) fn end(self) -> usize {
        self.start + self.len
    }

    pub(crate) fn adjust_after_removed(&mut self, removed: Self) {
        if self.is_empty() || removed.is_empty() || self.start <= removed.start {
            return;
        }
        self.start = self.start.saturating_sub(removed.len);
    }
}

/// The kernel's library registry: the authoritative store of loaded libraries
/// and their resolved exports.
///
/// The registry is catalog-backed for identity (the `catalog` field) and keeps
/// projection caches over that catalog for fast lookup by symbol, id, and kind.
/// It owns stable-id allocation, export registration, querying, transactional
/// loading, and catalog overlays. The kernel defines this contract; libraries
/// supply the behavior that gets registered. See the README "Library system"
/// section.
#[derive(Clone)]
pub struct Registry {
    catalog: CatalogStore,
    // Projection cache for `libs`.
    // Authoritative source: `registry/libs` and `registry/exports`.
    // Updated by: `commit_loaded_lib`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    libs: Vec<LoadedLib>,
    // Projection cache for `lib` and `manifest_by_symbol`.
    // Authoritative source: `registry/libs`.
    // Updated by: `commit_loaded_lib` and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    libs_by_symbol: BTreeMap<Symbol, LibId>,
    // Recorded load deltas keyed by loaded library id.
    // Authoritative source: committed load transactions.
    // Updated by: `commit_loaded_lib` and `Registry::unload`.
    // Covered by: unload roundtrip tests.
    load_deltas: BTreeMap<LibId, LoadDelta>,
    // Load-dependency edges keyed by dependent library id.
    // Authoritative source: manifest dependencies resolved at commit time.
    // Updated by: `commit_loaded_lib` and `Registry::unload`.
    // Covered by: unload refusal and cascade tests.
    load_dependencies: BTreeMap<LibId, BTreeSet<LibId>>,
    // Projection cache for `export_symbols`.
    // Authoritative source: `registry/exports`.
    // Updated by: `insert_runtime_export` and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    export_symbols: BTreeMap<super::ExportKind, BTreeMap<Symbol, RuntimeId>>,
    // Projection cache for `classes`.
    // Authoritative source: `registry/exports`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    class_symbol_cache: BTreeMap<Symbol, ClassId>,
    // Projection cache for `functions`.
    // Authoritative source: `registry/exports`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    function_symbol_cache: BTreeMap<Symbol, FunctionId>,
    // Projection cache for `macros`.
    // Authoritative source: `registry/exports`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    macro_symbol_cache: BTreeMap<Symbol, MacroId>,
    // Projection cache for `shapes`.
    // Authoritative source: `registry/exports`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    shape_symbol_cache: BTreeMap<Symbol, ShapeId>,
    // Projection cache for `codecs`.
    // Authoritative source: `registry/exports`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    codec_symbol_cache: BTreeMap<Symbol, CodecId>,
    // Projection cache for `number_domains`.
    // Authoritative source: `registry/exports`.
    // Updated by: `insert_number_domain_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    number_domain_symbol_cache: BTreeMap<Symbol, NumberDomainId>,
    // Projection cache for `sites`.
    // Authoritative source: `registry/exports`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    site_symbol_cache: BTreeMap<Symbol, SiteId>,
    // Projection cache for `export_symbol_for_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    plain_value_cache: BTreeMap<Symbol, Value>,
    // Projection cache for `class_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    class_value_cache: BTreeMap<ClassId, Value>,
    // Projection cache for `function_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    function_value_cache: BTreeMap<FunctionId, Value>,
    // Projection cache for `macro_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    macro_value_cache: BTreeMap<MacroId, Value>,
    // Projection cache for `shape_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    shape_value_cache: BTreeMap<ShapeId, Value>,
    // Projection cache for `codec_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    codec_value_cache: BTreeMap<CodecId, Value>,
    // Projection cache for `number_domain_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `insert_number_domain_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    number_domain_value_cache: BTreeMap<NumberDomainId, Value>,
    // Projection cache for `site_value`.
    // Authoritative source: `registry/runtime`.
    // Updated by: `register_runtime_value`, `commit_loaded_lib`, and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    site_value_cache: BTreeMap<SiteId, Value>,
    number_domain_order: Option<Vec<NumberDomainId>>,
    // Projection cache for `tests` and `registered_test`.
    // Authoritative source: `registry/tests`.
    // Updated by: `register_test` and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    tests: BTreeMap<Symbol, RegisteredTest>,
    // Projection cache for `tests_for_lib`.
    // Authoritative source: `registry/tests`.
    // Updated by: `register_test` and `rebuild_projection_caches_from_catalog`.
    // Covered by: `rebuilding_projection_caches_from_catalog_preserves_query_results`.
    tests_by_lib: BTreeMap<Symbol, Vec<Symbol>>,
    promotion_rules: Vec<crate::number_domain::PromotionRule>,
    value_promotion_rules: Vec<ValuePromotionRule>,
    number_unary_ops: Vec<NumberUnaryOp>,
    number_reduction_ops: Vec<NumberReductionOp>,
    number_binary_ops: Vec<NumberBinaryOp>,
    value_number_unary_ops: Vec<ValueNumberUnaryOp>,
    value_number_reduction_ops: Vec<ValueNumberReductionOp>,
    value_number_binary_ops: Vec<ValueNumberBinaryOp>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            catalog: catalog::new_registry_catalog(),
            libs: Vec::new(),
            libs_by_symbol: BTreeMap::new(),
            load_deltas: BTreeMap::new(),
            load_dependencies: BTreeMap::new(),
            export_symbols: BTreeMap::new(),
            class_symbol_cache: BTreeMap::new(),
            function_symbol_cache: BTreeMap::new(),
            macro_symbol_cache: BTreeMap::new(),
            shape_symbol_cache: BTreeMap::new(),
            codec_symbol_cache: BTreeMap::new(),
            number_domain_symbol_cache: BTreeMap::new(),
            site_symbol_cache: BTreeMap::new(),
            plain_value_cache: BTreeMap::new(),
            class_value_cache: BTreeMap::new(),
            function_value_cache: BTreeMap::new(),
            macro_value_cache: BTreeMap::new(),
            shape_value_cache: BTreeMap::new(),
            codec_value_cache: BTreeMap::new(),
            number_domain_value_cache: BTreeMap::new(),
            site_value_cache: BTreeMap::new(),
            number_domain_order: None,
            tests: BTreeMap::new(),
            tests_by_lib: BTreeMap::new(),
            promotion_rules: Vec::new(),
            value_promotion_rules: Vec::new(),
            number_unary_ops: Vec::new(),
            number_reduction_ops: Vec::new(),
            number_binary_ops: Vec::new(),
            value_number_unary_ops: Vec::new(),
            value_number_reduction_ops: Vec::new(),
            value_number_binary_ops: Vec::new(),
        }
    }
}
