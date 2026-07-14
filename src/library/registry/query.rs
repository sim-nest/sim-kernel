use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{
    catalog::CatalogSnapshot,
    id::{
        ClassId, CodecId, FunctionId, MacroId, NumberDomainId, RuntimeId, ShapeId, SiteId, Symbol,
    },
    value::Value,
};

use super::Registry;
use crate::library::{
    ExportKind, LibBootDependency, LibBootReceipt, LibManifest, LibSourceSpec, LoadedLib,
    RegisteredTest, Test,
};

impl Registry {
    /// Returns a snapshot of the registry's identity catalog.
    pub fn catalog_snapshot(&self) -> CatalogSnapshot {
        CatalogSnapshot::from_store(&self.catalog)
    }

    /// All loaded libraries, in load order.
    pub fn libs(&self) -> &[LoadedLib] {
        &self.libs
    }

    /// Builds the data-only boot receipt for a loaded library id.
    pub fn boot_receipt(
        &self,
        lib_id: crate::LibId,
        requested_source: LibSourceSpec,
        resolved_source: LibSourceSpec,
    ) -> Option<LibBootReceipt> {
        let loaded = self.libs.iter().find(|loaded| loaded.id == lib_id)?;
        let dependencies = self
            .load_dependencies
            .get(&lib_id)
            .into_iter()
            .flat_map(|dependencies| dependencies.iter())
            .filter_map(|dependency| {
                let symbol = self
                    .libs
                    .iter()
                    .find(|loaded| loaded.id == *dependency)?
                    .manifest
                    .id
                    .clone();
                Some(LibBootDependency {
                    lib_id: *dependency,
                    symbol,
                })
            })
            .collect();
        Some(LibBootReceipt {
            lib_id,
            requested_source,
            resolved_source,
            manifest: loaded.manifest.clone(),
            dependencies,
            exports: loaded.exports.clone(),
        })
    }

    /// Returns a registry restricted to the named libraries and their exports.
    pub fn subset_for_libs(&self, libs: &[Symbol]) -> Self {
        if libs.is_empty() {
            return Self::default();
        }

        let allowed = libs.iter().cloned().collect::<BTreeSet<_>>();
        let mut registry = self.clone();
        registry
            .libs
            .retain(|loaded| allowed.contains(&loaded.manifest.id));
        registry
            .libs_by_symbol
            .retain(|symbol, _| allowed.contains(symbol));
        let allowed_ids = registry
            .libs
            .iter()
            .map(|loaded| loaded.id)
            .collect::<BTreeSet<_>>();
        registry
            .load_deltas
            .retain(|lib_id, _| allowed_ids.contains(lib_id));
        registry.load_dependencies.retain(|lib_id, dependencies| {
            if !allowed_ids.contains(lib_id) {
                return false;
            }
            dependencies.retain(|dependency| allowed_ids.contains(dependency));
            true
        });

        let exported_symbols = registry
            .libs
            .iter()
            .flat_map(|loaded| loaded.exports.iter())
            .map(|record| (record.kind.clone(), record.symbol.clone()))
            .collect::<BTreeSet<_>>();

        registry.export_symbols.retain(|kind, symbols| {
            symbols.retain(|symbol, _| exported_symbols.contains(&(kind.clone(), symbol.clone())));
            !symbols.is_empty()
        });

        registry.class_symbol_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::CLASS), symbol.clone()))
        });
        registry.function_symbol_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::FUNCTION), symbol.clone()))
        });
        registry.macro_symbol_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::MACRO), symbol.clone()))
        });
        registry.shape_symbol_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::SHAPE), symbol.clone()))
        });
        registry.codec_symbol_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::CODEC), symbol.clone()))
        });
        registry.number_domain_symbol_cache.retain(|symbol, _| {
            exported_symbols
                .contains(&(ExportKind::named(ExportKind::NUMBER_DOMAIN), symbol.clone()))
        });
        registry.site_symbol_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::SITE), symbol.clone()))
        });
        registry.plain_value_cache.retain(|symbol, _| {
            exported_symbols.contains(&(ExportKind::named(ExportKind::VALUE), symbol.clone()))
        });

        registry.class_value_cache.retain(|id, _| {
            registry
                .class_symbol_cache
                .values()
                .any(|value| value == id)
        });
        registry.function_value_cache.retain(|id, _| {
            registry
                .function_symbol_cache
                .values()
                .any(|value| value == id)
        });
        registry.macro_value_cache.retain(|id, _| {
            registry
                .macro_symbol_cache
                .values()
                .any(|value| value == id)
        });
        registry.shape_value_cache.retain(|id, _| {
            registry
                .shape_symbol_cache
                .values()
                .any(|value| value == id)
        });
        registry.codec_value_cache.retain(|id, _| {
            registry
                .codec_symbol_cache
                .values()
                .any(|value| value == id)
        });
        registry.number_domain_value_cache.retain(|id, _| {
            registry
                .number_domain_symbol_cache
                .values()
                .any(|value| value == id)
        });
        registry
            .site_value_cache
            .retain(|id, _| registry.site_symbol_cache.values().any(|value| value == id));
        registry.tests.retain(|_, test| allowed.contains(&test.lib));
        registry
            .tests_by_lib
            .retain(|symbol, _| allowed.contains(symbol));
        registry.retain_catalog_rows_for_subset();
        registry.rebuild_projection_caches_from_catalog();
        registry
    }

    /// Looks up a loaded library by symbol.
    pub fn lib(&self, symbol: &Symbol) -> Option<&LoadedLib> {
        let id = self.libs_by_symbol.get(symbol)?;
        self.libs.iter().find(|loaded| loaded.id == *id)
    }

    pub(crate) fn lib_mut(&mut self, symbol: &Symbol) -> Option<&mut LoadedLib> {
        let id = *self.libs_by_symbol.get(symbol)?;
        self.libs.iter_mut().find(|loaded| loaded.id == id)
    }

    /// The full export index, keyed by kind then symbol.
    pub fn export_symbols(
        &self,
    ) -> &std::collections::BTreeMap<ExportKind, std::collections::BTreeMap<Symbol, crate::RuntimeId>>
    {
        &self.export_symbols
    }

    /// The class symbol-to-id index.
    pub fn classes(&self) -> &std::collections::BTreeMap<Symbol, ClassId> {
        &self.class_symbol_cache
    }

    /// The function symbol-to-id index.
    pub fn functions(&self) -> &std::collections::BTreeMap<Symbol, FunctionId> {
        &self.function_symbol_cache
    }

    /// The macro symbol-to-id index.
    pub fn macros(&self) -> &std::collections::BTreeMap<Symbol, MacroId> {
        &self.macro_symbol_cache
    }

    /// The shape symbol-to-id index.
    pub fn shapes(&self) -> &std::collections::BTreeMap<Symbol, ShapeId> {
        &self.shape_symbol_cache
    }

    /// The codec symbol-to-id index.
    pub fn codecs(&self) -> &std::collections::BTreeMap<Symbol, CodecId> {
        &self.codec_symbol_cache
    }

    /// The number-domain symbol-to-id index.
    pub fn number_domains(&self) -> &std::collections::BTreeMap<Symbol, NumberDomainId> {
        &self.number_domain_symbol_cache
    }

    /// The site symbol-to-id index.
    pub fn sites(&self) -> &std::collections::BTreeMap<Symbol, SiteId> {
        &self.site_symbol_cache
    }

    /// All registered tests, keyed by symbol.
    pub fn tests(&self) -> &std::collections::BTreeMap<Symbol, RegisteredTest> {
        &self.tests
    }

    /// The symbols of tests owned by the given library, if any.
    pub fn tests_for_lib(&self, symbol: &Symbol) -> Option<&[Symbol]> {
        self.tests_by_lib.get(symbol).map(Vec::as_slice)
    }

    /// Resolves a class value by stable id.
    pub fn class_value(&self, id: ClassId) -> Option<&Value> {
        self.catalog_value_by_runtime_id(RuntimeId::Class(id))
    }

    /// Resolves a function value by stable id.
    pub fn function_value(&self, id: FunctionId) -> Option<&Value> {
        self.catalog_value_by_runtime_id(RuntimeId::Function(id))
    }

    /// Resolves a macro value by stable id.
    pub fn macro_value(&self, id: MacroId) -> Option<&Value> {
        self.catalog_value_by_runtime_id(RuntimeId::Macro(id))
    }

    /// Resolves a shape value by stable id.
    pub fn shape_value(&self, id: ShapeId) -> Option<&Value> {
        self.catalog_value_by_runtime_id(RuntimeId::Shape(id))
    }

    /// Resolves a codec value by stable id.
    pub fn codec_value(&self, id: CodecId) -> Option<&Value> {
        self.catalog_value_by_runtime_id(RuntimeId::Codec(id))
    }

    /// Resolves a number-domain value by stable id.
    pub fn number_domain_value(&self, id: NumberDomainId) -> Option<&Value> {
        self.catalog_value_by_runtime_id(RuntimeId::NumberDomain(id))
    }

    /// Resolves an opaque site value by runtime id.
    pub fn site_value(&self, id: RuntimeId) -> Option<&Value> {
        match id {
            RuntimeId::Site(_) => self.catalog_value_by_runtime_id(id),
            _ => None,
        }
    }

    /// Resolves a class value by symbol.
    pub fn class_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::CLASS), symbol)
    }

    /// Resolves a function value by symbol.
    pub fn function_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::FUNCTION), symbol)
    }

    /// Resolves a macro value by symbol.
    pub fn macro_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::MACRO), symbol)
    }

    /// Resolves a shape value by symbol.
    pub fn shape_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::SHAPE), symbol)
    }

    /// Resolves a codec value by symbol.
    pub fn codec_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::CODEC), symbol)
    }

    /// Resolves a number-domain value by symbol.
    pub fn number_domain_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::NUMBER_DOMAIN), symbol)
    }

    /// Resolves an opaque site value by symbol.
    pub fn site_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::SITE), symbol)
    }

    /// Returns the manifest of a loaded library by symbol.
    pub fn manifest_by_symbol(&self, symbol: &Symbol) -> Option<&LibManifest> {
        self.lib(symbol).map(|loaded| &loaded.manifest)
    }

    /// Returns the test implementation registered under `symbol`.
    pub fn test_by_symbol(&self, symbol: &Symbol) -> Option<&Arc<dyn Test>> {
        self.tests.get(symbol).map(|test| &test.test)
    }

    /// Returns the full [`RegisteredTest`] record for `symbol`.
    pub fn registered_test(&self, symbol: &Symbol) -> Option<&RegisteredTest> {
        self.tests.get(symbol)
    }

    /// Resolves a plain value export by symbol.
    pub fn value_by_symbol(&self, symbol: &Symbol) -> Option<&Value> {
        self.catalog_value_by_export(&ExportKind::named(ExportKind::VALUE), symbol)
    }

    /// Returns the export symbol a registered value is bound under, searching
    /// every export kind, if any.
    pub fn export_symbol_for_value(&self, value: &Value) -> Option<Symbol> {
        self.class_symbol_cache
            .iter()
            .find_map(|(symbol, id)| {
                self.class_value_cache
                    .get(id)
                    .filter(|candidate| *candidate == value)
                    .map(|_| symbol.clone())
            })
            .or_else(|| {
                self.function_symbol_cache.iter().find_map(|(symbol, id)| {
                    self.function_value_cache
                        .get(id)
                        .filter(|candidate| *candidate == value)
                        .map(|_| symbol.clone())
                })
            })
            .or_else(|| {
                self.macro_symbol_cache.iter().find_map(|(symbol, id)| {
                    self.macro_value_cache
                        .get(id)
                        .filter(|candidate| *candidate == value)
                        .map(|_| symbol.clone())
                })
            })
            .or_else(|| {
                self.shape_symbol_cache.iter().find_map(|(symbol, id)| {
                    self.shape_value_cache
                        .get(id)
                        .filter(|candidate| *candidate == value)
                        .map(|_| symbol.clone())
                })
            })
            .or_else(|| {
                self.codec_symbol_cache.iter().find_map(|(symbol, id)| {
                    self.codec_value_cache
                        .get(id)
                        .filter(|candidate| *candidate == value)
                        .map(|_| symbol.clone())
                })
            })
            .or_else(|| {
                self.number_domain_symbol_cache
                    .iter()
                    .find_map(|(symbol, id)| {
                        self.number_domain_value_cache
                            .get(id)
                            .filter(|candidate| *candidate == value)
                            .map(|_| symbol.clone())
                    })
            })
            .or_else(|| {
                self.site_symbol_cache.iter().find_map(|(symbol, id)| {
                    self.site_value_cache
                        .get(id)
                        .filter(|candidate| *candidate == value)
                        .map(|_| symbol.clone())
                })
            })
            .or_else(|| {
                self.plain_value_cache
                    .iter()
                    .find_map(|(symbol, candidate)| (candidate == value).then(|| symbol.clone()))
            })
    }
}
