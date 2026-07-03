use crate::{
    error::{Error, Result},
    id::{
        CaseId, ClassId, CodecId, FunctionId, LibId, MacroId, NumberDomainId, RuntimeId, ShapeId,
        SiteId, Symbol,
    },
    number_domain::{
        NumberBinaryOp, NumberReductionOp, NumberUnaryOp, ValueNumberBinaryOp,
        ValueNumberReductionOp, ValueNumberUnaryOp, ValuePromotionRule,
    },
    value::Value,
};

use super::{Registry, catalog};
use crate::library::{ExportKind, ExportRecord, ExportState, RegisteredTest, Test};

impl Registry {
    /// Registers a class value directly, reserving a fresh class id; errors on a
    /// duplicate class symbol.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use sim_kernel::library::Registry;
    /// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
    ///
    /// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    /// let class = cx.factory().bool(true).unwrap();
    ///
    /// let mut registry = Registry::default();
    /// let id = registry
    ///     .register_class_value(Symbol::new("flag"), class.clone())
    ///     .unwrap();
    ///
    /// assert_eq!(registry.class_by_symbol(&Symbol::new("flag")), Some(&class));
    /// assert_eq!(registry.class_value(id), Some(&class));
    ///
    /// // A duplicate class symbol is rejected.
    /// assert!(registry.register_class_value(Symbol::new("flag"), class).is_err());
    /// ```
    pub fn register_class_value(&mut self, symbol: Symbol, value: Value) -> Result<ClassId> {
        let kind = ExportKind::named(ExportKind::CLASS);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "class",
                symbol,
            });
        }
        let id = self.fresh_class_id();
        self.register_runtime_value(symbol, value, kind, RuntimeId::Class(id))?;
        Ok(id)
    }

    /// Registers a function value directly, reserving a fresh function id;
    /// errors on a duplicate function symbol.
    pub fn register_function_value(&mut self, symbol: Symbol, value: Value) -> Result<FunctionId> {
        let kind = ExportKind::named(ExportKind::FUNCTION);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "function",
                symbol,
            });
        }
        let id = self.fresh_function_id();
        self.register_runtime_value(symbol, value, kind, RuntimeId::Function(id))?;
        Ok(id)
    }

    /// Registers a macro value directly, reserving a fresh macro id; errors on a
    /// duplicate macro symbol.
    pub fn register_macro_value(&mut self, symbol: Symbol, value: Value) -> Result<MacroId> {
        let kind = ExportKind::named(ExportKind::MACRO);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "macro",
                symbol,
            });
        }
        let id = self.fresh_macro_id();
        self.register_runtime_value(symbol, value, kind, RuntimeId::Macro(id))?;
        Ok(id)
    }

    /// Registers a shape value directly, reserving a fresh shape id; errors on a
    /// duplicate shape symbol.
    pub fn register_shape_value(&mut self, symbol: Symbol, value: Value) -> Result<ShapeId> {
        let kind = ExportKind::named(ExportKind::SHAPE);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "shape",
                symbol,
            });
        }
        let id = self.fresh_shape_id();
        self.register_runtime_value(symbol, value, kind, RuntimeId::Shape(id))?;
        Ok(id)
    }

    /// Registers a codec value directly, reserving a fresh codec id; errors on a
    /// duplicate codec symbol.
    pub fn register_codec_value(&mut self, symbol: Symbol, value: Value) -> Result<CodecId> {
        let kind = ExportKind::named(ExportKind::CODEC);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "codec",
                symbol,
            });
        }
        let id = self.fresh_codec_id();
        self.register_runtime_value(symbol, value, kind, RuntimeId::Codec(id))?;
        Ok(id)
    }

    /// Registers a number-domain value directly, reserving a fresh id; errors on
    /// a duplicate number-domain symbol.
    pub fn register_number_domain_value(
        &mut self,
        symbol: Symbol,
        value: Value,
    ) -> Result<NumberDomainId> {
        let kind = ExportKind::named(ExportKind::NUMBER_DOMAIN);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "number-domain",
                symbol,
            });
        }
        let id = self.fresh_number_domain_id();
        self.register_runtime_value(symbol, value, kind, RuntimeId::NumberDomain(id))?;
        Ok(id)
    }

    /// Registers an opaque site value directly, reserving a fresh site id;
    /// errors on a duplicate site symbol.
    pub fn register_site_value(&mut self, symbol: Symbol, value: Value) -> Result<RuntimeId> {
        let kind = ExportKind::named(ExportKind::SITE);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "site",
                symbol,
            });
        }
        let id = self.fresh_site_id();
        let runtime_id = RuntimeId::Site(id);
        self.register_runtime_value(symbol, value, kind, runtime_id)?;
        Ok(runtime_id)
    }

    /// Returns registered number domains ordered by parse priority (descending),
    /// then by symbol; the order is computed once and cached.
    pub fn sorted_number_domains(&mut self) -> Vec<(Symbol, Value)> {
        if self.number_domain_order.is_none() {
            self.rebuild_number_domain_order();
        }

        self.number_domain_order
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|id| {
                let symbol = self
                    .number_domain_symbol_cache
                    .iter()
                    .find_map(|(symbol, candidate)| (*candidate == *id).then(|| symbol.clone()))?;
                let value = self.number_domain_value_cache.get(id)?.clone();
                Some((symbol, value))
            })
            .collect()
    }

    /// Registers a library-supplied test under `symbol`; errors on a duplicate
    /// test symbol.
    pub fn register_test(
        &mut self,
        symbol: Symbol,
        lib: Symbol,
        test: std::sync::Arc<dyn Test>,
        subjects: Vec<Symbol>,
    ) -> Result<()> {
        if self.catalog_test_by_symbol(&symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "test",
                symbol,
            });
        }
        self.commit_direct_test_registration(
            symbol.clone(),
            lib.clone(),
            test.clone(),
            subjects.clone(),
        )?;
        self.tests.insert(
            symbol.clone(),
            RegisteredTest {
                symbol: symbol.clone(),
                lib: lib.clone(),
                test,
                subjects,
            },
        );
        self.tests_by_lib
            .entry(lib)
            .or_default()
            .push(symbol.clone());
        Ok(())
    }

    /// Registers a plain value export by symbol; errors on a duplicate value
    /// symbol.
    pub fn register_value(&mut self, symbol: Symbol, value: Value) -> Result<()> {
        let kind = ExportKind::named(ExportKind::VALUE);
        if self.export_row_by_kind_symbol(&kind, &symbol).is_some() {
            return Err(Error::DuplicateExport {
                kind: "value",
                symbol,
            });
        }
        self.register_runtime_value(symbol, value, kind, RuntimeId::Value)?;
        Ok(())
    }

    /// Registers a plain value and records it as a resolved export of `lib`.
    pub fn register_value_for_lib(
        &mut self,
        lib: &Symbol,
        symbol: Symbol,
        value: Value,
    ) -> Result<()> {
        self.register_value(symbol.clone(), value)?;
        self.append_export_record(
            lib,
            ExportRecord {
                kind: ExportKind::named(ExportKind::VALUE),
                symbol,
                state: ExportState::Resolved {
                    id: RuntimeId::Value,
                },
            },
        )
    }

    /// Appends an export record to an already-loaded library, indexing it as a
    /// runtime export when resolved; errors on an unknown lib or duplicate
    /// kind+symbol.
    pub fn append_export_record(&mut self, lib: &Symbol, record: ExportRecord) -> Result<()> {
        let kind = record.kind.clone();
        let symbol = record.symbol.clone();
        let runtime_id = match &record.state {
            ExportState::Resolved { id } => Some(*id),
            _ => None,
        };

        let Some(loaded) = self.lib_mut(lib) else {
            return Err(Error::Lib(format!("unknown lib {lib}")));
        };
        if loaded
            .exports
            .iter()
            .any(|existing| existing.kind == kind && existing.symbol == symbol)
        {
            return Err(Error::DuplicateExport {
                kind: kind.duplicate_error_kind(),
                symbol,
            });
        }
        loaded.exports.push(record);
        if let Some(runtime_id) = runtime_id {
            self.insert_runtime_export(kind, symbol, runtime_id);
        }
        Ok(())
    }

    /// Registers a typed number binary operator.
    pub fn register_number_binary_op(&mut self, op: NumberBinaryOp) {
        self.number_binary_ops.push(op);
    }

    /// Registers a value-level number binary operator.
    pub fn register_value_number_binary_op(&mut self, op: ValueNumberBinaryOp) {
        self.value_number_binary_ops.push(op);
    }

    /// Registers a typed number unary operator.
    pub fn register_number_unary_op(&mut self, op: NumberUnaryOp) {
        self.number_unary_ops.push(op);
    }

    /// Registers a value-level number unary operator.
    pub fn register_value_number_unary_op(&mut self, op: ValueNumberUnaryOp) {
        self.value_number_unary_ops.push(op);
    }

    /// Registers a typed number reduction operator.
    pub fn register_number_reduction_op(&mut self, op: NumberReductionOp) {
        self.number_reduction_ops.push(op);
    }

    /// Registers a value-level number reduction operator.
    pub fn register_value_number_reduction_op(&mut self, op: ValueNumberReductionOp) {
        self.value_number_reduction_ops.push(op);
    }

    /// Registers a typed number-domain promotion rule.
    pub fn register_promotion_rule(&mut self, rule: crate::number_domain::PromotionRule) {
        self.promotion_rules.push(rule);
    }

    /// Registers a value-level number-domain promotion rule.
    pub fn register_value_promotion_rule(&mut self, rule: ValuePromotionRule) {
        self.value_promotion_rules.push(rule);
    }

    /// Returns the cheapest promotion rule from `from_domain` to `to_domain`, if
    /// any.
    pub fn promotion_rule(
        &self,
        from_domain: &Symbol,
        to_domain: &Symbol,
    ) -> Option<&crate::number_domain::PromotionRule> {
        self.promotion_rules
            .iter()
            .filter(|rule| &rule.from_domain == from_domain && &rule.to_domain == to_domain)
            .min_by_key(|rule| rule.cost)
    }

    /// All registered typed promotion rules.
    pub fn promotion_rules(&self) -> &[crate::number_domain::PromotionRule] {
        &self.promotion_rules
    }

    /// All registered value-level promotion rules.
    pub fn value_promotion_rules(&self) -> &[ValuePromotionRule] {
        &self.value_promotion_rules
    }

    /// All registered typed number binary operators.
    pub fn number_binary_ops(&self) -> &[NumberBinaryOp] {
        &self.number_binary_ops
    }

    /// All registered value-level number binary operators.
    pub fn value_number_binary_ops(&self) -> &[ValueNumberBinaryOp] {
        &self.value_number_binary_ops
    }

    /// All registered typed number unary operators.
    pub fn number_unary_ops(&self) -> &[NumberUnaryOp] {
        &self.number_unary_ops
    }

    /// All registered value-level number unary operators.
    pub fn value_number_unary_ops(&self) -> &[ValueNumberUnaryOp] {
        &self.value_number_unary_ops
    }

    /// All registered typed number reduction operators.
    pub fn number_reduction_ops(&self) -> &[NumberReductionOp] {
        &self.number_reduction_ops
    }

    /// All registered value-level number reduction operators.
    pub fn value_number_reduction_ops(&self) -> &[ValueNumberReductionOp] {
        &self.value_number_reduction_ops
    }

    /// Returns the cheapest typed binary operator matching the operator and
    /// operand domains, if any.
    pub fn number_binary_op(
        &self,
        operator: &Symbol,
        left_domain: &Symbol,
        right_domain: &Symbol,
    ) -> Option<&NumberBinaryOp> {
        self.number_binary_ops
            .iter()
            .filter(|op| {
                &op.operator == operator
                    && &op.left_domain == left_domain
                    && &op.right_domain == right_domain
            })
            .min_by_key(|op| op.cost)
    }

    /// Reserves a fresh stable library id from the catalog sequence.
    pub fn fresh_lib_id(&mut self) -> LibId {
        LibId(self.reserve_catalog_sequence_id(catalog::SEQ_LIB))
    }

    /// Reserves a fresh stable class id from the catalog sequence.
    pub fn fresh_class_id(&mut self) -> ClassId {
        ClassId(self.reserve_catalog_sequence_id(catalog::SEQ_CLASS))
    }

    pub(crate) fn reserve_class_id(&mut self, id: ClassId) {
        self.reserve_catalog_sequence_at_least(catalog::SEQ_CLASS, id.0);
    }

    /// Reserves a fresh stable function id from the catalog sequence.
    pub fn fresh_function_id(&mut self) -> FunctionId {
        FunctionId(self.reserve_catalog_sequence_id(catalog::SEQ_FUNCTION))
    }

    /// Reserves a fresh stable macro id from the catalog sequence.
    pub fn fresh_macro_id(&mut self) -> MacroId {
        MacroId(self.reserve_catalog_sequence_id(catalog::SEQ_MACRO))
    }

    /// Reserves a fresh stable case id from the catalog sequence.
    pub fn fresh_case_id(&mut self) -> CaseId {
        CaseId(self.reserve_catalog_sequence_id(catalog::SEQ_CASE))
    }

    /// Reserves a fresh stable shape id from the catalog sequence.
    pub fn fresh_shape_id(&mut self) -> ShapeId {
        ShapeId(self.reserve_catalog_sequence_id(catalog::SEQ_SHAPE))
    }

    /// Reserves a fresh stable codec id from the catalog sequence.
    pub fn fresh_codec_id(&mut self) -> CodecId {
        CodecId(self.reserve_catalog_sequence_id(catalog::SEQ_CODEC))
    }

    /// Reserves a fresh stable number-domain id from the catalog sequence.
    pub fn fresh_number_domain_id(&mut self) -> NumberDomainId {
        NumberDomainId(self.reserve_catalog_sequence_id(catalog::SEQ_NUMBER_DOMAIN))
    }

    /// Reserves a fresh stable site id from the catalog sequence.
    pub fn fresh_site_id(&mut self) -> SiteId {
        SiteId(self.reserve_catalog_sequence_id(catalog::SEQ_SITE))
    }

    pub(crate) fn insert_runtime_export(
        &mut self,
        kind: ExportKind,
        symbol: Symbol,
        id: RuntimeId,
    ) {
        self.export_symbols
            .entry(kind)
            .or_default()
            .insert(symbol, id);
    }

    fn register_runtime_value(
        &mut self,
        symbol: Symbol,
        value: Value,
        kind: ExportKind,
        runtime_id: RuntimeId,
    ) -> Result<()> {
        self.commit_direct_runtime_registration(
            kind.clone(),
            symbol.clone(),
            runtime_id,
            value.clone(),
        )?;
        match runtime_id {
            RuntimeId::Class(id) => {
                self.class_symbol_cache.insert(symbol.clone(), id);
                self.class_value_cache.insert(id, value);
            }
            RuntimeId::Function(id) => {
                self.function_symbol_cache.insert(symbol.clone(), id);
                self.function_value_cache.insert(id, value);
            }
            RuntimeId::Macro(id) => {
                self.macro_symbol_cache.insert(symbol.clone(), id);
                self.macro_value_cache.insert(id, value);
            }
            RuntimeId::Shape(id) => {
                self.shape_symbol_cache.insert(symbol.clone(), id);
                self.shape_value_cache.insert(id, value);
            }
            RuntimeId::Codec(id) => {
                self.codec_symbol_cache.insert(symbol.clone(), id);
                self.codec_value_cache.insert(id, value);
            }
            RuntimeId::NumberDomain(id) => {
                self.insert_number_domain_value(symbol.clone(), id, value);
            }
            RuntimeId::Site(id) => {
                self.site_symbol_cache.insert(symbol.clone(), id);
                self.site_value_cache.insert(id, value);
            }
            RuntimeId::Value => {
                self.plain_value_cache.insert(symbol.clone(), value);
            }
        }
        self.insert_runtime_export(kind.clone(), symbol.clone(), runtime_id);
        Ok(())
    }

    pub(crate) fn insert_number_domain_value(
        &mut self,
        symbol: Symbol,
        id: NumberDomainId,
        value: Value,
    ) {
        self.number_domain_symbol_cache.insert(symbol, id);
        self.number_domain_value_cache.insert(id, value);
        self.number_domain_order = None;
    }

    pub(crate) fn rebuild_number_domain_order(&mut self) {
        let mut order = self
            .number_domain_symbol_cache
            .iter()
            .filter_map(|(symbol, id)| {
                let value = self.number_domain_value_cache.get(id)?;
                let priority = value
                    .object()
                    .as_number_domain()
                    .map(|domain| domain.parse_priority())
                    .unwrap_or(0);
                Some((priority, symbol.clone(), *id))
            })
            .collect::<Vec<_>>();
        order.sort_by(
            |(left_priority, left_symbol, _), (right_priority, right_symbol, _)| {
                right_priority
                    .cmp(left_priority)
                    .then_with(|| left_symbol.cmp(right_symbol))
            },
        );
        self.number_domain_order = Some(order.into_iter().map(|(_, _, id)| id).collect());
    }
}
