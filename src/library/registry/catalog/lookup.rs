use std::{collections::BTreeMap, collections::BTreeSet, sync::Arc};

use crate::{
    ClassId, CodecId, Expr, FunctionId, MacroId, NumberDomainId, NumberLiteral, RuntimeId, ShapeId,
    SiteId, Symbol, Value,
    catalog::CatalogRow,
    library::{ExportKind, ExportState, RegisteredTest},
};

use super::{
    SEQ_CLASS, SEQ_CODEC, SEQ_FUNCTION, SEQ_MACRO, SEQ_NUMBER_DOMAIN, SEQ_SHAPE, SEQ_SITE,
    exports_table, runtime_table, schema,
};

use super::super::Registry;

impl Registry {
    pub(crate) fn runtime_row_by_kind_id(&self, kind: &Symbol, id: u64) -> Option<&CatalogRow> {
        self.catalog
            .row(&runtime_table(), &schema::runtime_key(kind, id))
    }

    pub(crate) fn export_row_by_kind_symbol(
        &self,
        kind: &ExportKind,
        symbol: &Symbol,
    ) -> Option<&CatalogRow> {
        self.catalog
            .row(&exports_table(), &schema::export_key(kind.symbol(), symbol))
    }

    pub(crate) fn catalog_test_by_symbol(&self, symbol: &Symbol) -> Option<&CatalogRow> {
        self.catalog
            .row(&super::tests_table(), &schema::test_key(symbol))
    }

    pub(crate) fn catalog_value_by_runtime_id(&self, runtime_id: RuntimeId) -> Option<&Value> {
        let row = runtime_row_for_runtime_id(self, runtime_id, None)?;
        value_from_runtime_row(row)
    }

    pub(crate) fn catalog_value_by_export(
        &self,
        kind: &ExportKind,
        symbol: &Symbol,
    ) -> Option<&Value> {
        let export = self.export_row_by_kind_symbol(kind, symbol)?;
        let runtime_id = runtime_id_from_export_row(export)?;
        let row = runtime_row_for_runtime_id(self, runtime_id, Some(symbol))?;
        value_from_runtime_row(row)
    }

    pub(crate) fn retain_catalog_rows_for_subset(&mut self) {
        let lib_keys = self
            .libs
            .iter()
            .map(|loaded| schema::lib_key(&loaded.manifest.id))
            .collect::<BTreeSet<_>>();
        retain_catalog_keys(self, &super::libs_table(), &lib_keys);

        let mut export_keys = BTreeSet::new();
        for loaded in &self.libs {
            for record in &loaded.exports {
                export_keys.insert(export_key_for_record(&loaded.manifest.id, record));
            }
        }
        let test_kind = Symbol::new("test");
        for registered in self.tests.values() {
            export_keys.insert(schema::export_record_key(
                &registered.lib,
                &test_kind,
                &registered.symbol,
            ));
        }
        retain_catalog_keys(self, &exports_table(), &export_keys);

        let mut runtime_keys = BTreeSet::new();
        runtime_keys.extend(
            self.class_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_CLASS), u64::from(id.0))),
        );
        runtime_keys.extend(
            self.function_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_FUNCTION), u64::from(id.0))),
        );
        runtime_keys.extend(
            self.macro_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_MACRO), u64::from(id.0))),
        );
        runtime_keys.extend(
            self.shape_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_SHAPE), u64::from(id.0))),
        );
        runtime_keys.extend(
            self.codec_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_CODEC), u64::from(id.0))),
        );
        runtime_keys.extend(
            self.number_domain_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_NUMBER_DOMAIN), u64::from(id.0))),
        );
        runtime_keys.extend(
            self.site_value_cache
                .keys()
                .map(|id| schema::runtime_key(&Symbol::new(SEQ_SITE), u64::from(id.0))),
        );
        runtime_keys.extend(self.plain_value_cache.keys().map(schema::plain_value_key));
        retain_catalog_keys(self, &runtime_table(), &runtime_keys);

        let test_keys = self
            .tests
            .keys()
            .map(schema::test_key)
            .collect::<BTreeSet<_>>();
        retain_catalog_keys(self, &super::tests_table(), &test_keys);
    }

    pub(crate) fn rebuild_projection_caches_from_catalog(&mut self) {
        let libs_by_symbol = self
            .catalog
            .rows(&super::libs_table())
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter_map(lib_cache_entry_from_row)
            .collect::<BTreeMap<_, _>>();

        let resolved_exports = self
            .catalog
            .rows(&exports_table())
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter_map(resolved_export_from_row)
            .collect::<Vec<_>>();

        let mut export_symbols = BTreeMap::new();
        let mut class_symbol_cache = BTreeMap::new();
        let mut function_symbol_cache = BTreeMap::new();
        let mut macro_symbol_cache = BTreeMap::new();
        let mut shape_symbol_cache = BTreeMap::new();
        let mut codec_symbol_cache = BTreeMap::new();
        let mut number_domain_symbol_cache = BTreeMap::new();
        let mut site_symbol_cache = BTreeMap::new();
        let mut plain_value_cache = BTreeMap::new();
        let mut class_value_cache = BTreeMap::new();
        let mut function_value_cache = BTreeMap::new();
        let mut macro_value_cache = BTreeMap::new();
        let mut shape_value_cache = BTreeMap::new();
        let mut codec_value_cache = BTreeMap::new();
        let mut number_domain_value_cache = BTreeMap::new();
        let mut site_value_cache = BTreeMap::new();

        for (kind, symbol, runtime_id) in resolved_exports {
            export_symbols
                .entry(kind.clone())
                .or_insert_with(BTreeMap::new)
                .insert(symbol.clone(), runtime_id);
            let value = self.catalog_value_by_export(&kind, &symbol).cloned();
            match (runtime_id, value) {
                (RuntimeId::Class(id), Some(value)) => {
                    class_symbol_cache.insert(symbol, id);
                    class_value_cache.insert(id, value);
                }
                (RuntimeId::Function(id), Some(value)) => {
                    function_symbol_cache.insert(symbol, id);
                    function_value_cache.insert(id, value);
                }
                (RuntimeId::Macro(id), Some(value)) => {
                    macro_symbol_cache.insert(symbol, id);
                    macro_value_cache.insert(id, value);
                }
                (RuntimeId::Shape(id), Some(value)) => {
                    shape_symbol_cache.insert(symbol, id);
                    shape_value_cache.insert(id, value);
                }
                (RuntimeId::Codec(id), Some(value)) => {
                    codec_symbol_cache.insert(symbol, id);
                    codec_value_cache.insert(id, value);
                }
                (RuntimeId::NumberDomain(id), Some(value)) => {
                    number_domain_symbol_cache.insert(symbol, id);
                    number_domain_value_cache.insert(id, value);
                }
                (RuntimeId::Site(id), Some(value)) => {
                    site_symbol_cache.insert(symbol, id);
                    site_value_cache.insert(id, value);
                }
                (RuntimeId::Value, Some(value)) => {
                    plain_value_cache.insert(symbol, value);
                }
                _ => {}
            }
        }

        let tests = self
            .catalog
            .rows(&super::tests_table())
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter_map(test_cache_entry_from_row)
            .collect::<BTreeMap<_, _>>();
        let mut tests_by_lib = BTreeMap::<Symbol, Vec<Symbol>>::new();
        for registered in tests.values() {
            tests_by_lib
                .entry(registered.lib.clone())
                .or_default()
                .push(registered.symbol.clone());
        }

        self.libs_by_symbol = libs_by_symbol;
        self.export_symbols = export_symbols;
        self.class_symbol_cache = class_symbol_cache;
        self.function_symbol_cache = function_symbol_cache;
        self.macro_symbol_cache = macro_symbol_cache;
        self.shape_symbol_cache = shape_symbol_cache;
        self.codec_symbol_cache = codec_symbol_cache;
        self.number_domain_symbol_cache = number_domain_symbol_cache;
        self.site_symbol_cache = site_symbol_cache;
        self.plain_value_cache = plain_value_cache;
        self.class_value_cache = class_value_cache;
        self.function_value_cache = function_value_cache;
        self.macro_value_cache = macro_value_cache;
        self.shape_value_cache = shape_value_cache;
        self.codec_value_cache = codec_value_cache;
        self.number_domain_value_cache = number_domain_value_cache;
        self.site_value_cache = site_value_cache;
        self.number_domain_order = None;
        self.tests = tests;
        self.tests_by_lib = tests_by_lib;
    }

    #[cfg(test)]
    pub(crate) fn assert_projection_caches_match_catalog_for_tests(&self) {
        let mut rebuilt = self.clone();
        rebuilt.rebuild_projection_caches_from_catalog();
        assert_eq!(
            self.libs_by_symbol, rebuilt.libs_by_symbol,
            "projection cache libs_by_symbol mismatch"
        );
        assert_eq!(
            self.export_symbols, rebuilt.export_symbols,
            "projection cache export_symbols mismatch"
        );
        assert_eq!(
            self.class_symbol_cache, rebuilt.class_symbol_cache,
            "projection cache class_symbol_cache mismatch"
        );
        assert_eq!(
            self.function_symbol_cache, rebuilt.function_symbol_cache,
            "projection cache function_symbol_cache mismatch"
        );
        assert_eq!(
            self.macro_symbol_cache, rebuilt.macro_symbol_cache,
            "projection cache macro_symbol_cache mismatch"
        );
        assert_eq!(
            self.shape_symbol_cache, rebuilt.shape_symbol_cache,
            "projection cache shape_symbol_cache mismatch"
        );
        assert_eq!(
            self.codec_symbol_cache, rebuilt.codec_symbol_cache,
            "projection cache codec_symbol_cache mismatch"
        );
        assert_eq!(
            self.number_domain_symbol_cache, rebuilt.number_domain_symbol_cache,
            "projection cache number_domain_symbol_cache mismatch"
        );
        assert_eq!(
            self.site_symbol_cache, rebuilt.site_symbol_cache,
            "projection cache site_symbol_cache mismatch"
        );
        assert_eq!(
            self.plain_value_cache, rebuilt.plain_value_cache,
            "projection cache plain_value_cache mismatch"
        );
        assert_eq!(
            self.class_value_cache, rebuilt.class_value_cache,
            "projection cache class_value_cache mismatch"
        );
        assert_eq!(
            self.function_value_cache, rebuilt.function_value_cache,
            "projection cache function_value_cache mismatch"
        );
        assert_eq!(
            self.macro_value_cache, rebuilt.macro_value_cache,
            "projection cache macro_value_cache mismatch"
        );
        assert_eq!(
            self.shape_value_cache, rebuilt.shape_value_cache,
            "projection cache shape_value_cache mismatch"
        );
        assert_eq!(
            self.codec_value_cache, rebuilt.codec_value_cache,
            "projection cache codec_value_cache mismatch"
        );
        assert_eq!(
            self.number_domain_value_cache, rebuilt.number_domain_value_cache,
            "projection cache number_domain_value_cache mismatch"
        );
        assert_eq!(
            self.site_value_cache, rebuilt.site_value_cache,
            "projection cache site_value_cache mismatch"
        );
        assert_registered_tests_cache_matches(&self.tests, &rebuilt.tests);
        assert_eq!(
            self.tests_by_lib, rebuilt.tests_by_lib,
            "projection cache tests_by_lib mismatch"
        );
    }
}

pub(crate) fn value_from_runtime_row(row: &CatalogRow) -> Option<&Value> {
    row.live_value(&schema::field("value"))
}

pub(crate) fn runtime_id_from_export_row(row: &CatalogRow) -> Option<RuntimeId> {
    let state = row.data.get(&schema::field("state"))?;
    if state != &Expr::Symbol(Symbol::new("resolved")) {
        return None;
    }

    let Expr::Symbol(kind) = row.data.get(&schema::field("runtime-kind"))? else {
        return None;
    };
    match kind.as_qualified_str().as_str() {
        "class" => parse_runtime_id(row).map(|id| RuntimeId::Class(ClassId(id))),
        "function" => parse_runtime_id(row).map(|id| RuntimeId::Function(FunctionId(id))),
        "macro" => parse_runtime_id(row).map(|id| RuntimeId::Macro(MacroId(id))),
        "shape" => parse_runtime_id(row).map(|id| RuntimeId::Shape(ShapeId(id))),
        "codec" => parse_runtime_id(row).map(|id| RuntimeId::Codec(CodecId(id))),
        "number-domain" => {
            parse_runtime_id(row).map(|id| RuntimeId::NumberDomain(NumberDomainId(id)))
        }
        "site" => parse_runtime_id(row).map(|id| RuntimeId::Site(SiteId(id))),
        "value" => Some(RuntimeId::Value),
        _ => None,
    }
}

fn runtime_row_for_runtime_id<'a>(
    registry: &'a Registry,
    runtime_id: RuntimeId,
    value_symbol: Option<&Symbol>,
) -> Option<&'a CatalogRow> {
    match runtime_id {
        RuntimeId::Class(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_CLASS), u64::from(id.0))
        }
        RuntimeId::Function(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_FUNCTION), u64::from(id.0))
        }
        RuntimeId::Macro(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_MACRO), u64::from(id.0))
        }
        RuntimeId::Shape(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_SHAPE), u64::from(id.0))
        }
        RuntimeId::Codec(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_CODEC), u64::from(id.0))
        }
        RuntimeId::NumberDomain(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_NUMBER_DOMAIN), u64::from(id.0))
        }
        RuntimeId::Site(id) => {
            registry.runtime_row_by_kind_id(&Symbol::new(SEQ_SITE), u64::from(id.0))
        }
        RuntimeId::Value => {
            let symbol = value_symbol?;
            registry
                .catalog
                .row(&runtime_table(), &schema::plain_value_key(symbol))
        }
    }
}

fn parse_runtime_id(row: &CatalogRow) -> Option<u32> {
    parse_u32_field(row, "runtime-id")
}

fn export_key_for_record(lib: &Symbol, record: &crate::ExportRecord) -> Symbol {
    match &record.state {
        ExportState::Resolved { .. } => schema::export_key(record.kind.symbol(), &record.symbol),
        _ => schema::export_record_key(lib, record.kind.symbol(), &record.symbol),
    }
}

fn retain_catalog_keys(registry: &mut Registry, table: &Symbol, keep: &BTreeSet<Symbol>) {
    let existing = registry
        .catalog
        .rows(table)
        .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    for key in existing {
        if !keep.contains(&key) {
            registry.catalog.delete_row(table, &key);
        }
    }
}

fn lib_cache_entry_from_row(row: &CatalogRow) -> Option<(Symbol, crate::LibId)> {
    let symbol = symbol_field(row, "symbol")?;
    let id = parse_u32_field(row, "id")?;
    Some((symbol, crate::LibId(id)))
}

fn resolved_export_from_row(row: &CatalogRow) -> Option<(ExportKind, Symbol, RuntimeId)> {
    let kind = ExportKind::new(symbol_field(row, "kind")?);
    let symbol = symbol_field(row, "symbol")?;
    let runtime_id = runtime_id_from_export_row(row)?;
    Some((kind, symbol, runtime_id))
}

fn test_cache_entry_from_row(row: &CatalogRow) -> Option<(Symbol, RegisteredTest)> {
    let symbol = symbol_field(row, "symbol")?;
    let lib = symbol_field(row, "lib")?;
    let subjects = symbol_list_field(row, "subjects")?;
    let test = Arc::clone(row.live_test(&schema::field("test"))?);
    Some((
        symbol.clone(),
        RegisteredTest {
            symbol,
            lib,
            test,
            subjects,
        },
    ))
}

fn symbol_field(row: &CatalogRow, name: &'static str) -> Option<Symbol> {
    let Expr::Symbol(symbol) = row.data.get(&schema::field(name))? else {
        return None;
    };
    Some(symbol.clone())
}

fn symbol_list_field(row: &CatalogRow, name: &'static str) -> Option<Vec<Symbol>> {
    let Expr::List(values) = row.data.get(&schema::field(name))? else {
        return None;
    };
    values
        .iter()
        .map(|value| match value {
            Expr::Symbol(symbol) => Some(symbol.clone()),
            _ => None,
        })
        .collect()
}

fn parse_u32_field(row: &CatalogRow, name: &'static str) -> Option<u32> {
    let Expr::Number(NumberLiteral { canonical, .. }) = row.data.get(&schema::field(name))? else {
        return None;
    };
    canonical.parse().ok()
}

#[cfg(test)]
fn assert_registered_tests_cache_matches(
    left: &BTreeMap<Symbol, RegisteredTest>,
    right: &BTreeMap<Symbol, RegisteredTest>,
) {
    assert_eq!(left.len(), right.len(), "projection cache tests mismatch");
    for (symbol, left) in left {
        let right = right
            .get(symbol)
            .expect("projection cache tests should contain every registered test");
        assert_eq!(left.symbol, right.symbol);
        assert_eq!(left.lib, right.lib);
        assert_eq!(left.subjects, right.subjects);
        assert!(
            Arc::ptr_eq(&left.test, &right.test),
            "projection cache test pointer mismatch for {symbol}"
        );
    }
}
