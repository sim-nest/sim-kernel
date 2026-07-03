use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ClassId, CodecId, FunctionId, LibId, MacroId, NumberDomainId, Result, RuntimeId, ShapeId,
    SiteId, Symbol, Value,
    library::{ExportKind, LoadedLib, RegisteredTest},
    number_domain::{
        NumberBinaryOp, NumberReductionOp, NumberUnaryOp, ValueNumberBinaryOp,
        ValueNumberReductionOp, ValueNumberUnaryOp, ValuePromotionRule,
    },
};

use super::{LoadDelta, Registry};

impl Registry {
    /// Runs `f` against the registry under a catalog overlay, committing on
    /// success and rolling both the catalog and the projection caches back to
    /// their captured state on error.
    pub fn with_catalog_overlay<F, R>(&mut self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Self) -> Result<R>,
    {
        let state = RegistryOverlayState::capture(self);
        self.catalog.begin_overlay()?;
        match f(self) {
            Ok(value) => match self.catalog.commit_overlay() {
                Ok(()) => Ok(value),
                Err(err) => {
                    state.restore(self);
                    Err(err)
                }
            },
            Err(err) => {
                self.catalog.rollback_overlay();
                state.restore(self);
                Err(err)
            }
        }
    }
}

#[derive(Clone)]
struct RegistryOverlayState {
    libs: Vec<LoadedLib>,
    libs_by_symbol: BTreeMap<Symbol, LibId>,
    load_deltas: BTreeMap<LibId, LoadDelta>,
    load_dependencies: BTreeMap<LibId, BTreeSet<LibId>>,
    export_symbols: BTreeMap<ExportKind, BTreeMap<Symbol, RuntimeId>>,
    class_symbol_cache: BTreeMap<Symbol, ClassId>,
    function_symbol_cache: BTreeMap<Symbol, FunctionId>,
    macro_symbol_cache: BTreeMap<Symbol, MacroId>,
    shape_symbol_cache: BTreeMap<Symbol, ShapeId>,
    codec_symbol_cache: BTreeMap<Symbol, CodecId>,
    number_domain_symbol_cache: BTreeMap<Symbol, NumberDomainId>,
    site_symbol_cache: BTreeMap<Symbol, SiteId>,
    plain_value_cache: BTreeMap<Symbol, Value>,
    class_value_cache: BTreeMap<ClassId, Value>,
    function_value_cache: BTreeMap<FunctionId, Value>,
    macro_value_cache: BTreeMap<MacroId, Value>,
    shape_value_cache: BTreeMap<ShapeId, Value>,
    codec_value_cache: BTreeMap<CodecId, Value>,
    number_domain_value_cache: BTreeMap<NumberDomainId, Value>,
    site_value_cache: BTreeMap<SiteId, Value>,
    number_domain_order: Option<Vec<NumberDomainId>>,
    tests: BTreeMap<Symbol, RegisteredTest>,
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

impl RegistryOverlayState {
    fn capture(registry: &Registry) -> Self {
        Self {
            libs: registry.libs.clone(),
            libs_by_symbol: registry.libs_by_symbol.clone(),
            load_deltas: registry.load_deltas.clone(),
            load_dependencies: registry.load_dependencies.clone(),
            export_symbols: registry.export_symbols.clone(),
            class_symbol_cache: registry.class_symbol_cache.clone(),
            function_symbol_cache: registry.function_symbol_cache.clone(),
            macro_symbol_cache: registry.macro_symbol_cache.clone(),
            shape_symbol_cache: registry.shape_symbol_cache.clone(),
            codec_symbol_cache: registry.codec_symbol_cache.clone(),
            number_domain_symbol_cache: registry.number_domain_symbol_cache.clone(),
            site_symbol_cache: registry.site_symbol_cache.clone(),
            plain_value_cache: registry.plain_value_cache.clone(),
            class_value_cache: registry.class_value_cache.clone(),
            function_value_cache: registry.function_value_cache.clone(),
            macro_value_cache: registry.macro_value_cache.clone(),
            shape_value_cache: registry.shape_value_cache.clone(),
            codec_value_cache: registry.codec_value_cache.clone(),
            number_domain_value_cache: registry.number_domain_value_cache.clone(),
            site_value_cache: registry.site_value_cache.clone(),
            number_domain_order: registry.number_domain_order.clone(),
            tests: registry.tests.clone(),
            tests_by_lib: registry.tests_by_lib.clone(),
            promotion_rules: registry.promotion_rules.clone(),
            value_promotion_rules: registry.value_promotion_rules.clone(),
            number_unary_ops: registry.number_unary_ops.clone(),
            number_reduction_ops: registry.number_reduction_ops.clone(),
            number_binary_ops: registry.number_binary_ops.clone(),
            value_number_unary_ops: registry.value_number_unary_ops.clone(),
            value_number_reduction_ops: registry.value_number_reduction_ops.clone(),
            value_number_binary_ops: registry.value_number_binary_ops.clone(),
        }
    }

    fn restore(self, registry: &mut Registry) {
        registry.libs = self.libs;
        registry.libs_by_symbol = self.libs_by_symbol;
        registry.load_deltas = self.load_deltas;
        registry.load_dependencies = self.load_dependencies;
        registry.export_symbols = self.export_symbols;
        registry.class_symbol_cache = self.class_symbol_cache;
        registry.function_symbol_cache = self.function_symbol_cache;
        registry.macro_symbol_cache = self.macro_symbol_cache;
        registry.shape_symbol_cache = self.shape_symbol_cache;
        registry.codec_symbol_cache = self.codec_symbol_cache;
        registry.number_domain_symbol_cache = self.number_domain_symbol_cache;
        registry.site_symbol_cache = self.site_symbol_cache;
        registry.plain_value_cache = self.plain_value_cache;
        registry.class_value_cache = self.class_value_cache;
        registry.function_value_cache = self.function_value_cache;
        registry.macro_value_cache = self.macro_value_cache;
        registry.shape_value_cache = self.shape_value_cache;
        registry.codec_value_cache = self.codec_value_cache;
        registry.number_domain_value_cache = self.number_domain_value_cache;
        registry.site_value_cache = self.site_value_cache;
        registry.number_domain_order = self.number_domain_order;
        registry.tests = self.tests;
        registry.tests_by_lib = self.tests_by_lib;
        registry.promotion_rules = self.promotion_rules;
        registry.value_promotion_rules = self.value_promotion_rules;
        registry.number_unary_ops = self.number_unary_ops;
        registry.number_reduction_ops = self.number_reduction_ops;
        registry.number_binary_ops = self.number_binary_ops;
        registry.value_number_unary_ops = self.value_number_unary_ops;
        registry.value_number_reduction_ops = self.value_number_reduction_ops;
        registry.value_number_binary_ops = self.value_number_binary_ops;
    }
}
