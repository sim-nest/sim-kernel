mod lookup;
mod rows;
mod schema;
mod write;

use std::sync::Arc;

use crate::{
    Error, Expr, NumberLiteral, Result, RuntimeId, Symbol, Test, Value,
    catalog::{CatalogStore, CatalogTx},
    library::{ExportKind, ExportState},
};

use super::Registry;
use write::{push_number_op_rows, push_promotion_rule_rows};

#[cfg(test)]
use crate::library::RegisteredTest;

pub(crate) use rows::{
    export_row, lib_row, number_binary_op_row, number_reduction_op_row, number_unary_op_row,
    plain_value_row, promotion_rule_row, runtime_value_row, sequence_row, test_row,
    value_number_binary_op_row, value_number_reduction_op_row, value_number_unary_op_row,
    value_promotion_rule_row,
};
pub(crate) use schema::{
    export_key, export_record_key, exports_table, install_registry_catalog_schema, lib_key,
    libs_table, number_ops_table, plain_value_key, promotion_rules_table, runtime_key,
    runtime_table, sequences_table, test_key, tests_table, value_promotion_rules_table,
};
#[cfg(test)]
pub(crate) use schema::{schema_table, tests_by_lib_table};

pub(crate) const SEQ_LIB: &str = "lib";
pub(crate) const SEQ_CLASS: &str = "class";
pub(crate) const SEQ_FUNCTION: &str = "function";
pub(crate) const SEQ_MACRO: &str = "macro";
pub(crate) const SEQ_CASE: &str = "case";
pub(crate) const SEQ_SHAPE: &str = "shape";
pub(crate) const SEQ_CODEC: &str = "codec";
pub(crate) const SEQ_NUMBER_DOMAIN: &str = "number-domain";
pub(crate) const SEQ_SITE: &str = "site";

const TEST_EXPORT_KIND: &str = "test";

const SEQUENCE_KINDS: [&str; 9] = [
    SEQ_LIB,
    SEQ_CLASS,
    SEQ_FUNCTION,
    SEQ_MACRO,
    SEQ_CASE,
    SEQ_SHAPE,
    SEQ_CODEC,
    SEQ_NUMBER_DOMAIN,
    SEQ_SITE,
];

#[derive(Clone)]
pub(crate) struct CatalogRuntimeValue {
    pub(crate) kind: ExportKind,
    pub(crate) symbol: Symbol,
    pub(crate) runtime_id: RuntimeId,
    pub(crate) value: Value,
}

pub(crate) fn new_registry_catalog() -> CatalogStore {
    let mut store = CatalogStore::new();
    install_registry_catalog_schema(&mut store)
        .expect("registry catalog schema should install into a fresh store");
    let mut tx = CatalogTx::new();
    for kind in SEQUENCE_KINDS {
        let kind = Symbol::new(kind);
        tx.put_row(sequence_row(kind.clone(), 1));
        tx.bump_sequence(schema::sequence_key(&kind), 1);
    }
    tx.commit(&mut store)
        .expect("registry catalog sequence rows should validate");
    store
}

impl Registry {
    #[cfg(test)]
    pub(crate) fn catalog_sequence_next(&self, kind: &Symbol) -> Option<u64> {
        sequence_next(&self.catalog, kind)
    }

    pub(crate) fn catalog_sequence_snapshot(&self) -> std::collections::BTreeMap<Symbol, u64> {
        SEQUENCE_KINDS
            .into_iter()
            .filter_map(|kind| {
                let symbol = Symbol::new(kind);
                sequence_next(&self.catalog, &symbol).map(|next| (symbol, next))
            })
            .collect()
    }

    pub(crate) fn commit_direct_runtime_registration(
        &mut self,
        kind: ExportKind,
        symbol: Symbol,
        runtime_id: RuntimeId,
        value: Value,
    ) -> Result<()> {
        let mut tx = CatalogTx::new();
        tx.put_row(export_row(
            kind.clone(),
            symbol.clone(),
            direct_registration_lib(),
            ExportState::Resolved { id: runtime_id },
        ));
        tx.put_row(runtime_row(runtime_id, symbol.clone(), value));
        tx.commit(&mut self.catalog)
            .map(|_| ())
            .map_err(|err| map_catalog_conflict(err, kind.duplicate_error_kind(), symbol))
    }

    pub(crate) fn commit_direct_test_registration(
        &mut self,
        symbol: Symbol,
        lib: Symbol,
        test: Arc<dyn Test>,
        subjects: Vec<Symbol>,
    ) -> Result<()> {
        let mut tx = CatalogTx::new();
        tx.put_row(export_row(
            test_export_kind(),
            symbol.clone(),
            lib.clone(),
            ExportState::Declared,
        ));
        tx.put_row(test_row(symbol.clone(), lib, test, subjects));
        tx.commit(&mut self.catalog)
            .map(|_| ())
            .map_err(|err| map_catalog_conflict(err, TEST_EXPORT_KIND, symbol))
    }

    pub(crate) fn commit_loaded_lib_catalog(
        &mut self,
        id: crate::LibId,
        manifest: &crate::LibManifest,
        trusted: bool,
        export_records: &[crate::ExportRecord],
        runtime_values: &[CatalogRuntimeValue],
        pending: &crate::library::transaction::PendingExports,
    ) -> Result<()> {
        let mut tx = CatalogTx::new();
        tx.put_row(lib_row(id, manifest, trusted));
        for record in export_records {
            tx.put_row(export_row(
                record.kind.clone(),
                record.symbol.clone(),
                manifest.id.clone(),
                record.state.clone(),
            ));
        }
        for runtime in runtime_values {
            tx.put_row(runtime_row(
                runtime.runtime_id,
                runtime.symbol.clone(),
                runtime.value.clone(),
            ));
        }
        push_number_op_rows(&mut tx, self, pending);
        push_promotion_rule_rows(&mut tx, self, pending);
        tx.commit(&mut self.catalog).map(|_| ()).map_err(|err| {
            map_loaded_lib_catalog_error(err, manifest, export_records, runtime_values)
        })
    }

    #[cfg(test)]
    pub(crate) fn catalog_runtime_registration_matches(
        &self,
        kind: &ExportKind,
        symbol: &Symbol,
        runtime_id: RuntimeId,
        value: &Value,
    ) -> bool {
        let export_key = schema::export_key(kind.symbol(), symbol);
        let runtime_key = runtime_key_for_id(runtime_id, symbol);
        let field_value = schema::field("value");
        self.catalog.row(&exports_table(), &export_key).is_some()
            && self
                .catalog
                .row(&runtime_table(), &runtime_key)
                .and_then(|row| row.live_value(&field_value))
                == Some(value)
    }

    #[cfg(test)]
    pub(crate) fn catalog_runtime_projection_matches(
        &self,
        kind: &ExportKind,
        symbol: &Symbol,
        runtime_id: RuntimeId,
    ) -> bool {
        let Some(value) = self.registered_runtime_value(symbol, runtime_id) else {
            return false;
        };
        self.export_symbols
            .get(kind)
            .and_then(|symbols| symbols.get(symbol))
            == Some(&runtime_id)
            && self.runtime_symbol_map_matches(symbol, runtime_id)
            && self.catalog_runtime_registration_matches(kind, symbol, runtime_id, value)
    }

    #[cfg(test)]
    pub(crate) fn catalog_test_registration_matches(
        &self,
        symbol: &Symbol,
        registered: &RegisteredTest,
    ) -> bool {
        let export_key =
            schema::export_record_key(&registered.lib, test_export_kind().symbol(), symbol);
        let test_key = schema::test_key(symbol);
        let field_test = schema::field("test");
        let field_lib = schema::field("lib");
        let field_subjects = schema::field("subjects");
        let expected_subjects = Expr::List(
            registered
                .subjects
                .iter()
                .cloned()
                .map(Expr::Symbol)
                .collect(),
        );
        self.catalog.row(&exports_table(), &export_key).is_some()
            && self
                .catalog
                .row(&tests_table(), &test_key)
                .is_some_and(|row| {
                    row.data.get(&field_lib) == Some(&Expr::Symbol(registered.lib.clone()))
                        && row.data.get(&field_subjects) == Some(&expected_subjects)
                        && row
                            .live_test(&field_test)
                            .is_some_and(|test| Arc::ptr_eq(test, &registered.test))
                })
            && self
                .tests_by_lib
                .get(&registered.lib)
                .is_some_and(|symbols| symbols.contains(symbol))
    }

    #[cfg(test)]
    pub(crate) fn catalog_lib_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &libs_table())
    }

    #[cfg(test)]
    pub(crate) fn catalog_export_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &exports_table())
    }

    #[cfg(test)]
    pub(crate) fn catalog_runtime_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &runtime_table())
    }

    #[cfg(test)]
    pub(crate) fn catalog_test_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &tests_table())
    }

    #[cfg(test)]
    pub(crate) fn catalog_number_op_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &number_ops_table())
    }

    #[cfg(test)]
    pub(crate) fn catalog_promotion_rule_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &promotion_rules_table())
    }

    #[cfg(test)]
    pub(crate) fn catalog_value_promotion_rule_row_count_for_tests(&self) -> usize {
        catalog_row_count(&self.catalog, &value_promotion_rules_table())
    }

    #[cfg(test)]
    pub(crate) fn assert_catalog_projection_caches_for_tests(&self) {
        self.assert_projection_caches_match_catalog_for_tests();
        assert_eq!(self.catalog_lib_row_count_for_tests(), self.libs.len());
        assert_eq!(
            self.catalog_number_op_row_count_for_tests(),
            self.number_unary_ops.len()
                + self.number_reduction_ops.len()
                + self.number_binary_ops.len()
                + self.value_number_unary_ops.len()
                + self.value_number_reduction_ops.len()
                + self.value_number_binary_ops.len()
        );
        assert_eq!(
            self.catalog_promotion_rule_row_count_for_tests(),
            self.promotion_rules.len()
        );
        assert_eq!(
            self.catalog_value_promotion_rule_row_count_for_tests(),
            self.value_promotion_rules.len()
        );

        for loaded in &self.libs {
            let lib_key = schema::lib_key(&loaded.manifest.id);
            assert!(self.catalog.row(&libs_table(), &lib_key).is_some());
            for record in &loaded.exports {
                let export_key = match &record.state {
                    ExportState::Resolved { .. } => {
                        schema::export_key(record.kind.symbol(), &record.symbol)
                    }
                    _ => schema::export_record_key(
                        &loaded.manifest.id,
                        record.kind.symbol(),
                        &record.symbol,
                    ),
                };
                assert!(self.catalog.row(&exports_table(), &export_key).is_some());
                if let ExportState::Resolved { id } = record.state {
                    assert!(self.catalog_runtime_projection_matches(
                        &record.kind,
                        &record.symbol,
                        id
                    ));
                }
            }
        }
    }

    pub(crate) fn try_reserve_catalog_sequence_id(&mut self, kind: &'static str) -> Result<u32> {
        let kind = Symbol::new(kind);
        let reserved = sequence_next(&self.catalog, &kind)
            .ok_or_else(|| sequence_error(&kind, "missing or malformed sequence row"))?;
        let next = reserved
            .checked_add(1)
            .ok_or_else(|| sequence_error(&kind, "sequence overflow"))?;
        let id = u32::try_from(reserved)
            .map_err(|_| sequence_error(&kind, "sequence exceeded u32 id range"))?;
        write_sequence_next(&mut self.catalog, kind, next)?;
        Ok(id)
    }

    pub(crate) fn reserve_catalog_sequence_id(&mut self, kind: &'static str) -> u32 {
        self.try_reserve_catalog_sequence_id(kind)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub(crate) fn try_reserve_catalog_sequence_at_least(
        &mut self,
        kind: &'static str,
        id: u32,
    ) -> Result<()> {
        let kind = Symbol::new(kind);
        let current = sequence_next(&self.catalog, &kind)
            .ok_or_else(|| sequence_error(&kind, "missing or malformed sequence row"))?;
        let required = u64::from(id)
            .checked_add(1)
            .ok_or_else(|| sequence_error(&kind, "sequence overflow"))?;
        let next = current.max(required);
        if next != current {
            write_sequence_next(&mut self.catalog, kind, next)?;
        }
        Ok(())
    }

    pub(crate) fn set_catalog_sequence_next(
        &mut self,
        kind: &'static str,
        next: u64,
    ) -> Result<()> {
        write_sequence_next(&mut self.catalog, Symbol::new(kind), next)
    }
}

#[cfg(test)]
impl Registry {
    fn registered_runtime_value(&self, symbol: &Symbol, runtime_id: RuntimeId) -> Option<&Value> {
        match runtime_id {
            RuntimeId::Class(id) => self.class_value_cache.get(&id),
            RuntimeId::Function(id) => self.function_value_cache.get(&id),
            RuntimeId::Macro(id) => self.macro_value_cache.get(&id),
            RuntimeId::Shape(id) => self.shape_value_cache.get(&id),
            RuntimeId::Codec(id) => self.codec_value_cache.get(&id),
            RuntimeId::NumberDomain(id) => self.number_domain_value_cache.get(&id),
            RuntimeId::Site(id) => self.site_value_cache.get(&id),
            RuntimeId::Value => self.plain_value_cache.get(symbol),
        }
    }

    fn runtime_symbol_map_matches(&self, symbol: &Symbol, runtime_id: RuntimeId) -> bool {
        match runtime_id {
            RuntimeId::Class(id) => self.class_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::Function(id) => self.function_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::Macro(id) => self.macro_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::Shape(id) => self.shape_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::Codec(id) => self.codec_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::NumberDomain(id) => self.number_domain_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::Site(id) => self.site_symbol_cache.get(symbol) == Some(&id),
            RuntimeId::Value => self.plain_value_cache.contains_key(symbol),
        }
    }
}

fn direct_registration_lib() -> Symbol {
    Symbol::qualified("registry", "direct")
}

fn test_export_kind() -> ExportKind {
    ExportKind::named(TEST_EXPORT_KIND)
}

fn map_catalog_conflict(err: Error, kind: &'static str, symbol: Symbol) -> Error {
    match err {
        Error::CatalogConflict { .. } => Error::DuplicateExport { kind, symbol },
        other => other,
    }
}

fn map_loaded_lib_catalog_error(
    err: Error,
    manifest: &crate::LibManifest,
    export_records: &[crate::ExportRecord],
    runtime_values: &[CatalogRuntimeValue],
) -> Error {
    match err {
        Error::CatalogConflict { table, key: _ } if table == libs_table() => Error::DuplicateLib {
            symbol: manifest.id.clone(),
        },
        Error::CatalogConflict { table, key } if table == exports_table() => export_records
            .iter()
            .find(|record| export_catalog_key(&manifest.id, record) == key)
            .map(|record| Error::DuplicateExport {
                kind: record.kind.duplicate_error_kind(),
                symbol: record.symbol.clone(),
            })
            .unwrap_or(Error::CatalogConflict { table, key }),
        Error::CatalogConflict { table, key } if table == runtime_table() => runtime_values
            .iter()
            .find(|runtime| runtime_key_for_id(runtime.runtime_id, &runtime.symbol) == key)
            .map(|runtime| Error::DuplicateExport {
                kind: runtime.kind.duplicate_error_kind(),
                symbol: runtime.symbol.clone(),
            })
            .unwrap_or(Error::CatalogConflict { table, key }),
        other => other,
    }
}

fn export_catalog_key(lib: &Symbol, record: &crate::ExportRecord) -> Symbol {
    match &record.state {
        ExportState::Resolved { .. } => schema::export_key(record.kind.symbol(), &record.symbol),
        _ => schema::export_record_key(lib, record.kind.symbol(), &record.symbol),
    }
}

fn runtime_row(runtime_id: RuntimeId, symbol: Symbol, value: Value) -> crate::catalog::CatalogRow {
    match runtime_id {
        RuntimeId::Class(id) => {
            runtime_value_row(Symbol::new(SEQ_CLASS), u64::from(id.0), symbol, value)
        }
        RuntimeId::Function(id) => {
            runtime_value_row(Symbol::new(SEQ_FUNCTION), u64::from(id.0), symbol, value)
        }
        RuntimeId::Macro(id) => {
            runtime_value_row(Symbol::new(SEQ_MACRO), u64::from(id.0), symbol, value)
        }
        RuntimeId::Shape(id) => {
            runtime_value_row(Symbol::new(SEQ_SHAPE), u64::from(id.0), symbol, value)
        }
        RuntimeId::Codec(id) => {
            runtime_value_row(Symbol::new(SEQ_CODEC), u64::from(id.0), symbol, value)
        }
        RuntimeId::NumberDomain(id) => runtime_value_row(
            Symbol::new(SEQ_NUMBER_DOMAIN),
            u64::from(id.0),
            symbol,
            value,
        ),
        RuntimeId::Site(id) => {
            runtime_value_row(Symbol::new(SEQ_SITE), u64::from(id.0), symbol, value)
        }
        RuntimeId::Value => plain_value_row(symbol, value),
    }
}

fn runtime_key_for_id(runtime_id: RuntimeId, symbol: &Symbol) -> Symbol {
    match runtime_id {
        RuntimeId::Class(id) => schema::runtime_key(&Symbol::new(SEQ_CLASS), u64::from(id.0)),
        RuntimeId::Function(id) => schema::runtime_key(&Symbol::new(SEQ_FUNCTION), u64::from(id.0)),
        RuntimeId::Macro(id) => schema::runtime_key(&Symbol::new(SEQ_MACRO), u64::from(id.0)),
        RuntimeId::Shape(id) => schema::runtime_key(&Symbol::new(SEQ_SHAPE), u64::from(id.0)),
        RuntimeId::Codec(id) => schema::runtime_key(&Symbol::new(SEQ_CODEC), u64::from(id.0)),
        RuntimeId::NumberDomain(id) => {
            schema::runtime_key(&Symbol::new(SEQ_NUMBER_DOMAIN), u64::from(id.0))
        }
        RuntimeId::Site(id) => schema::runtime_key(&Symbol::new(SEQ_SITE), u64::from(id.0)),
        RuntimeId::Value => schema::plain_value_key(symbol),
    }
}

#[cfg(test)]
fn catalog_row_count(catalog: &CatalogStore, table: &Symbol) -> usize {
    catalog.rows(table).map_or(0, |rows| rows.len())
}

fn sequence_next(catalog: &CatalogStore, kind: &Symbol) -> Option<u64> {
    let row = catalog.row(&sequences_table(), &schema::sequence_key(kind))?;
    let Expr::Number(NumberLiteral { canonical, .. }) = row.data.get(&schema::field("next"))?
    else {
        return None;
    };
    canonical.parse().ok()
}

fn sequence_error(kind: &Symbol, message: &'static str) -> Error {
    Error::CatalogSchema {
        table: sequences_table(),
        message: format!("{message} for {kind}"),
    }
}

fn write_sequence_next(catalog: &mut CatalogStore, kind: Symbol, next: u64) -> Result<()> {
    let mut tx = CatalogTx::new();
    tx.put_row(sequence_row(kind.clone(), next));
    tx.bump_sequence(schema::sequence_key(&kind), next);
    tx.commit(catalog).map(|_| ())
}

#[cfg(test)]
mod commit_tests;
#[cfg(test)]
mod tests;
