use crate::{
    error::Result,
    id::{
        ClassId, CodecId, FunctionId, LibId, MacroId, NumberDomainId, RuntimeId, ShapeId, SiteId,
        Symbol,
    },
    library::{Export, ExportKind, ExportRecord, ExportState, Registry},
    number_domain::{
        NumberBinaryOp, NumberReductionOp, NumberUnaryOp, ValueNumberBinaryOp,
        ValueNumberReductionOp, ValueNumberUnaryOp, ValuePromotionRule,
    },
    value::Value,
};

#[derive(Default)]
pub(crate) struct PendingExports {
    pub(crate) exports: Vec<Export>,
    pub(crate) export_records: Vec<ExportRecord>,
    pub(crate) class_value_cache: Vec<(ClassId, Value)>,
    pub(crate) function_value_cache: Vec<(FunctionId, Value)>,
    pub(crate) macro_value_cache: Vec<(MacroId, Value)>,
    pub(crate) shape_value_cache: Vec<(ShapeId, Value)>,
    pub(crate) codec_value_cache: Vec<(CodecId, Value)>,
    pub(crate) number_domain_value_cache: Vec<(NumberDomainId, Value)>,
    pub(crate) site_value_cache: Vec<(SiteId, Value)>,
    pub(crate) values: Vec<(Symbol, Value)>,
    pub(crate) promotion_rules: Vec<crate::number_domain::PromotionRule>,
    pub(crate) value_promotion_rules: Vec<ValuePromotionRule>,
    pub(crate) number_unary_ops: Vec<NumberUnaryOp>,
    pub(crate) number_reduction_ops: Vec<NumberReductionOp>,
    pub(crate) number_binary_ops: Vec<NumberBinaryOp>,
    pub(crate) value_number_unary_ops: Vec<ValueNumberUnaryOp>,
    pub(crate) value_number_reduction_ops: Vec<ValueNumberReductionOp>,
    pub(crate) value_number_binary_ops: Vec<ValueNumberBinaryOp>,
}

/// An in-progress library load against a private copy of the [`Registry`].
///
/// A transaction stages all of a library's registrations on a cloned registry
/// and an internal pending-exports buffer; nothing reaches the live registry
/// until [`Registry::commit_load`](crate::library::Registry) succeeds, so a
/// failed load leaves the registry untouched.
pub struct LoadTransaction {
    pub(crate) lib_id: LibId,
    pub(crate) manifest: crate::library::LibManifest,
    pub(crate) trusted: bool,
    pub(crate) registry: Registry,
    pub(crate) pending: PendingExports,
}

/// The handle a [`Lib`](crate::library::Lib) uses to register its exports.
///
/// A `Linker` borrows the transaction's registry and pending buffer, reserving
/// stable ids and staging class/function/macro/shape/codec/number-domain/value
/// exports plus number-domain operators. The kernel defines these registration
/// contracts; the library calls them to declare its behavior.
pub struct Linker<'a> {
    registry: &'a mut Registry,
    lib: LibId,
    pending: &'a mut PendingExports,
}

impl<'a> Linker<'a> {
    pub(crate) fn new(
        registry: &'a mut Registry,
        lib: LibId,
        pending: &'a mut PendingExports,
    ) -> Self {
        Self {
            registry,
            lib,
            pending,
        }
    }

    /// The id of the library being loaded.
    pub fn lib_id(&self) -> LibId {
        self.lib
    }

    /// Read access to the transaction's working registry.
    pub fn registry(&self) -> &Registry {
        self.registry
    }

    /// Stages a class export under `symbol`, reserving a fresh class id.
    pub fn class(&mut self, symbol: Symbol) -> Result<ClassId> {
        let id = self.registry.try_fresh_class_id()?;
        self.pending.exports.push(Export::Class {
            symbol,
            class_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a class export under `symbol` using a caller-chosen class id,
    /// reserving the id sequence up to it.
    pub fn class_with_id(&mut self, symbol: Symbol, id: ClassId) -> Result<ClassId> {
        self.registry.reserve_class_id(id)?;
        self.pending.exports.push(Export::Class {
            symbol,
            class_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a class export and binds its runtime value in one step.
    pub fn class_value(&mut self, symbol: Symbol, value: Value) -> Result<ClassId> {
        let id = self.class(symbol)?;
        self.bind_class_value(id, value)?;
        Ok(id)
    }

    /// Binds a runtime value to an already-staged class id.
    pub fn bind_class_value(&mut self, id: ClassId, value: Value) -> Result<()> {
        self.pending.class_value_cache.push((id, value));
        Ok(())
    }

    /// Stages a function export under `symbol`, reserving a fresh function id.
    pub fn function(&mut self, symbol: Symbol) -> Result<FunctionId> {
        let id = self.registry.try_fresh_function_id()?;
        self.pending.exports.push(Export::Function {
            symbol,
            function_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a function export and binds its runtime value in one step.
    pub fn function_value(&mut self, symbol: Symbol, value: Value) -> Result<FunctionId> {
        let id = self.function(symbol)?;
        self.bind_function_value(id, value)?;
        Ok(id)
    }

    /// Binds a runtime value to an already-staged function id.
    pub fn bind_function_value(&mut self, id: FunctionId, value: Value) -> Result<()> {
        self.pending.function_value_cache.push((id, value));
        Ok(())
    }

    /// Stages a macro export under `symbol`, reserving a fresh macro id.
    pub fn macro_export(&mut self, symbol: Symbol) -> Result<MacroId> {
        let id = self.registry.try_fresh_macro_id()?;
        self.pending.exports.push(Export::Macro {
            symbol,
            macro_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a macro export and binds its runtime value in one step.
    pub fn macro_value(&mut self, symbol: Symbol, value: Value) -> Result<MacroId> {
        let id = self.macro_export(symbol)?;
        self.pending.macro_value_cache.push((id, value));
        Ok(id)
    }

    /// Stages a shape export under `symbol`, reserving a fresh shape id.
    pub fn shape(&mut self, symbol: Symbol) -> Result<ShapeId> {
        let id = self.registry.try_fresh_shape_id()?;
        self.pending.exports.push(Export::Shape {
            symbol,
            shape_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a shape export and binds its runtime value in one step.
    pub fn shape_value(&mut self, symbol: Symbol, value: Value) -> Result<ShapeId> {
        let id = self.shape(symbol)?;
        self.pending.shape_value_cache.push((id, value));
        Ok(id)
    }

    /// Stages a codec export under `symbol`, reserving a fresh codec id.
    pub fn codec(&mut self, symbol: Symbol) -> Result<CodecId> {
        let id = self.registry.try_fresh_codec_id()?;
        self.pending.exports.push(Export::Codec {
            symbol,
            codec_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a codec export and binds its runtime value in one step.
    pub fn codec_value(&mut self, symbol: Symbol, value: Value) -> Result<CodecId> {
        let id = self.codec(symbol)?;
        self.pending.codec_value_cache.push((id, value));
        Ok(id)
    }

    /// Stages a number-domain export under `symbol`, reserving a fresh id.
    pub fn number_domain(&mut self, symbol: Symbol) -> Result<NumberDomainId> {
        let id = self.registry.try_fresh_number_domain_id()?;
        self.pending.exports.push(Export::NumberDomain {
            symbol,
            number_domain_id: Some(id),
        });
        Ok(id)
    }

    /// Stages a number-domain export and binds its runtime value in one step.
    pub fn number_domain_value(&mut self, symbol: Symbol, value: Value) -> Result<NumberDomainId> {
        let id = self.number_domain(symbol)?;
        self.pending.number_domain_value_cache.push((id, value));
        Ok(id)
    }

    /// Stages an opaque site export and binds its runtime value in one step.
    ///
    /// The registry stores the value under the export symbol; concrete
    /// `EvalSite` behavior belongs to libraries that query the site registry.
    pub fn site_value(&mut self, symbol: Symbol, value: Value) -> Result<RuntimeId> {
        let site_id = self.registry.try_fresh_site_id()?;
        let runtime_id = RuntimeId::Site(site_id);
        self.pending.exports.push(Export::Site {
            symbol,
            runtime_id: Some(runtime_id),
        });
        self.pending.site_value_cache.push((site_id, value));
        Ok(runtime_id)
    }

    /// Stages a plain value export declaration (without binding a value).
    pub fn value_export(&mut self, symbol: Symbol) -> Result<()> {
        self.pending.exports.push(Export::Value { symbol });
        Ok(())
    }

    /// Stages a [`Declared`](ExportState::Declared) export record of any kind.
    pub fn declare_export(&mut self, kind: ExportKind, symbol: Symbol) -> Result<()> {
        self.pending.export_records.push(ExportRecord {
            kind,
            symbol,
            state: ExportState::Declared,
        });
        Ok(())
    }

    /// Stages an [`Unsupported`](ExportState::Unsupported) export record with a
    /// reason.
    pub fn unsupported_export(
        &mut self,
        kind: ExportKind,
        symbol: Symbol,
        reason: impl Into<String>,
    ) -> Result<()> {
        self.pending.export_records.push(ExportRecord {
            kind,
            symbol,
            state: ExportState::Unsupported {
                reason: reason.into(),
            },
        });
        Ok(())
    }

    /// Stages an [`Invalid`](ExportState::Invalid) export record with an error.
    pub fn invalid_export(
        &mut self,
        kind: ExportKind,
        symbol: Symbol,
        error: impl Into<String>,
    ) -> Result<()> {
        self.pending.export_records.push(ExportRecord {
            kind,
            symbol,
            state: ExportState::Invalid {
                error: error.into(),
            },
        });
        Ok(())
    }

    /// Stages a plain value export and binds its value in one step.
    pub fn value(&mut self, symbol: Symbol, value: Value) -> Result<()> {
        self.value_export(symbol.clone())?;
        self.pending.values.push((symbol, value));
        Ok(())
    }

    /// Stages a typed number binary operator.
    pub fn number_binary_op(&mut self, op: NumberBinaryOp) {
        self.pending.number_binary_ops.push(op);
    }

    /// Stages a value-level number binary operator.
    pub fn value_number_binary_op(&mut self, op: ValueNumberBinaryOp) {
        self.pending.value_number_binary_ops.push(op);
    }

    /// Stages a typed number unary operator.
    pub fn number_unary_op(&mut self, op: NumberUnaryOp) {
        self.pending.number_unary_ops.push(op);
    }

    /// Stages a value-level number unary operator.
    pub fn value_number_unary_op(&mut self, op: ValueNumberUnaryOp) {
        self.pending.value_number_unary_ops.push(op);
    }

    /// Stages a typed number reduction operator.
    pub fn number_reduction_op(&mut self, op: NumberReductionOp) {
        self.pending.number_reduction_ops.push(op);
    }

    /// Stages a value-level number reduction operator.
    pub fn value_number_reduction_op(&mut self, op: ValueNumberReductionOp) {
        self.pending.value_number_reduction_ops.push(op);
    }

    /// Stages a typed number-domain promotion rule.
    pub fn promotion_rule(&mut self, rule: crate::number_domain::PromotionRule) {
        self.pending.promotion_rules.push(rule);
    }

    /// Stages a value-level number-domain promotion rule.
    pub fn value_promotion_rule(&mut self, rule: ValuePromotionRule) {
        self.pending.value_promotion_rules.push(rule);
    }
}

impl LoadTransaction {
    /// Borrows a [`Linker`] over this transaction's registry and pending buffer.
    pub fn linker(&mut self) -> Linker<'_> {
        Linker::new(&mut self.registry, self.lib_id, &mut self.pending)
    }
}
