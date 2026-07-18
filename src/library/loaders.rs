use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    capability::{CapabilityName, CapabilitySet},
    env::Cx,
    error::{Error, Result},
    factory::Factory,
    id::{CaseId, ClassId, CodecId, FunctionId, MacroId, NumberDomainId, ShapeId, Symbol},
    library::{
        Dependency, LibBootReceipt, LibManifest, LibSourceSpec, Registry, RegistryBootState,
    },
    value::Value,
};

use super::transaction::Linker;

/// The context a [`Lib`] sees while loading.
///
/// It exposes the object [`Factory`], read access to the [`Registry`], fresh
/// stable-id allocation, capability checks, and symbol resolution against
/// already-loaded exports. The kernel supplies this surface; the library uses
/// it to wire its behavior in.
pub struct LoadCx {
    capabilities: CapabilitySet,
    factory: Arc<dyn Factory>,
    registry: Registry,
}

impl LoadCx {
    pub(crate) fn new(
        capabilities: CapabilitySet,
        factory: Arc<dyn Factory>,
        registry: Registry,
    ) -> Self {
        Self {
            capabilities,
            factory,
            registry,
        }
    }

    /// The object factory available during load.
    pub fn factory(&self) -> &dyn Factory {
        self.factory.as_ref()
    }

    /// Read access to the registry as loading proceeds.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Reserves a fresh stable class id.
    pub fn fresh_class_id(&mut self) -> ClassId {
        self.registry.fresh_class_id()
    }

    /// Reserves a fresh stable class id, reporting catalog sequence failures.
    pub fn try_fresh_class_id(&mut self) -> Result<ClassId> {
        self.registry.try_fresh_class_id()
    }

    /// Reserves a fresh stable function id.
    pub fn fresh_function_id(&mut self) -> FunctionId {
        self.registry.fresh_function_id()
    }

    /// Reserves a fresh stable function id, reporting catalog sequence failures.
    pub fn try_fresh_function_id(&mut self) -> Result<FunctionId> {
        self.registry.try_fresh_function_id()
    }

    /// Reserves a fresh stable macro id.
    pub fn fresh_macro_id(&mut self) -> MacroId {
        self.registry.fresh_macro_id()
    }

    /// Reserves a fresh stable macro id, reporting catalog sequence failures.
    pub fn try_fresh_macro_id(&mut self) -> Result<MacroId> {
        self.registry.try_fresh_macro_id()
    }

    /// Reserves a fresh stable case id.
    pub fn fresh_case_id(&mut self) -> CaseId {
        self.registry.fresh_case_id()
    }

    /// Reserves a fresh stable case id, reporting catalog sequence failures.
    pub fn try_fresh_case_id(&mut self) -> Result<CaseId> {
        self.registry.try_fresh_case_id()
    }

    /// Reserves a fresh stable shape id.
    pub fn fresh_shape_id(&mut self) -> ShapeId {
        self.registry.fresh_shape_id()
    }

    /// Reserves a fresh stable shape id, reporting catalog sequence failures.
    pub fn try_fresh_shape_id(&mut self) -> Result<ShapeId> {
        self.registry.try_fresh_shape_id()
    }

    /// Reserves a fresh stable codec id.
    pub fn fresh_codec_id(&mut self) -> CodecId {
        self.registry.fresh_codec_id()
    }

    /// Reserves a fresh stable codec id, reporting catalog sequence failures.
    pub fn try_fresh_codec_id(&mut self) -> Result<CodecId> {
        self.registry.try_fresh_codec_id()
    }

    /// Reserves a fresh stable number-domain id.
    pub fn fresh_number_domain_id(&mut self) -> NumberDomainId {
        self.registry.fresh_number_domain_id()
    }

    /// Reserves a fresh stable number-domain id, reporting catalog sequence
    /// failures.
    pub fn try_fresh_number_domain_id(&mut self) -> Result<NumberDomainId> {
        self.registry.try_fresh_number_domain_id()
    }

    /// Checks that the given capability is granted, returning
    /// [`Error::CapabilityDenied`](crate::error::Error::CapabilityDenied)
    /// otherwise.
    pub fn require(&self, capability: &CapabilityName) -> Result<()> {
        if self.capabilities.contains(capability) {
            Ok(())
        } else {
            Err(Error::CapabilityDenied {
                capability: capability.clone(),
            })
        }
    }

    /// Resolves an already-registered class value by symbol.
    pub fn resolve_class(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .class_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownClass {
                class: symbol.clone(),
            })
    }

    /// Resolves an already-registered function value by symbol.
    pub fn resolve_function(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .function_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownFunction {
                function: symbol.clone(),
            })
    }

    /// Resolves an already-registered macro value by symbol.
    pub fn resolve_macro(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .macro_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownSymbol {
                symbol: symbol.clone(),
            })
    }

    /// Resolves an already-registered shape value by symbol.
    pub fn resolve_shape(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .shape_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownSymbol {
                symbol: symbol.clone(),
            })
    }

    /// Resolves an already-registered codec value by symbol.
    pub fn resolve_codec(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .codec_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownSymbol {
                symbol: symbol.clone(),
            })
    }

    /// Resolves an already-registered number-domain value by symbol.
    pub fn resolve_number_domain(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .number_domain_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownSymbol {
                symbol: symbol.clone(),
            })
    }

    /// Resolves an already-registered plain value by symbol.
    pub fn resolve_value(&self, symbol: &Symbol) -> Result<Value> {
        self.registry
            .value_by_symbol(symbol)
            .cloned()
            .ok_or_else(|| Error::UnknownSymbol {
                symbol: symbol.clone(),
            })
    }
}

fn trim_trailing_numeric_zero_components<'a>(components: &'a [&'a str]) -> &'a [&'a str] {
    let mut end = components.len();
    while end > 0 && components[end - 1].parse::<u64>() == Ok(0) {
        end -= 1;
    }
    &components[..end]
}

pub(crate) fn compare_version_text(left: &str, right: &str) -> Ordering {
    let left_components = left.split('.').collect::<Vec<_>>();
    let right_components = right.split('.').collect::<Vec<_>>();
    let left = trim_trailing_numeric_zero_components(&left_components);
    let right = trim_trailing_numeric_zero_components(&right_components);
    let len = left.len().max(right.len());
    for index in 0..len {
        let left_component = left.get(index).copied().unwrap_or("0");
        let right_component = right.get(index).copied().unwrap_or("0");
        let ordering = match (
            left_component.parse::<u64>(),
            right_component.parse::<u64>(),
        ) {
            (Ok(left_number), Ok(right_number)) => left_number.cmp(&right_number),
            _ => left_component.cmp(right_component),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

pub(crate) fn dependency_satisfied(loaded: &LibManifest, dependency: &Dependency) -> bool {
    match &dependency.minimum_version {
        Some(minimum) => compare_version_text(&loaded.version.0, &minimum.0) != Ordering::Less,
        None => true,
    }
}

/// The contract every loadable library implements.
///
/// A library presents its [`LibManifest`] and wires its behavior into the
/// registry through a [`Linker`] during [`load`](Lib::load). The kernel defines
/// this contract; libraries provide the behavior.
pub trait Lib {
    /// Returns the library's manifest.
    fn manifest(&self) -> LibManifest;
    /// Registers the library's exports against the registry via `linker`.
    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> Result<()>;

    /// Tears down external resources that cannot be represented in the
    /// registry's recorded load delta.
    ///
    /// Normal export removal is driven by [`Registry::unload`], which retracts
    /// the committed registry records for a loaded [`LibId`](crate::LibId).
    /// Libraries should only override this hook when they acquire external
    /// resources outside the registry receipt; the default means there is no
    /// custom external teardown.
    fn unload(&self, _cx: &mut Cx, _linker: &mut Linker) -> Result<()> {
        Ok(())
    }
}

/// Where a library is loaded from.
pub enum LibSource {
    /// A catalog-resolved library symbol.
    Symbol(Symbol),
    /// A filesystem path.
    Path(PathBuf),
    /// A URL.
    Url(String),
    /// In-memory bytes (e.g. an ABI frame).
    Bytes(Vec<u8>),
    /// A host-constructed library object.
    Host(Box<dyn Lib>),
}

/// A catalog-registered source for a library symbol.
///
/// Unlike [`LibSource`], this excludes the in-process `Symbol`/`Host` variants,
/// so it can be stored and resolved by symbol; it converts into a [`LibSource`]
/// on use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogSource {
    /// A filesystem path.
    Path(PathBuf),
    /// A URL.
    Url(String),
    /// In-memory bytes.
    Bytes(Vec<u8>),
}

impl From<CatalogSource> for LibSource {
    fn from(source: CatalogSource) -> Self {
        match source {
            CatalogSource::Path(path) => Self::Path(path),
            CatalogSource::Url(url) => Self::Url(url),
            CatalogSource::Bytes(bytes) => Self::Bytes(bytes),
        }
    }
}

/// A loader that can turn a [`LibSource`] into a [`Lib`].
///
/// Loaders are themselves library behavior; the kernel only defines the
/// contract for plugging them in.
pub trait LibLoader: Send + Sync {
    /// Reports whether this loader accepts the given source.
    fn can_load(&self, source: &LibSource) -> bool;
    /// Loads the source into a library object.
    fn load(&self, cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>>;

    /// Inspects a source's manifest without fully loading it; the default
    /// returns `None` (not supported).
    fn inspect_manifest(&self, _cx: &mut Cx, _source: &LibSource) -> Result<Option<LibManifest>> {
        Ok(None)
    }
}

/// A registry of [`LibLoader`]s plus catalog-resolvable library sources.
#[derive(Default)]
pub struct LoaderRegistry {
    loaders: Vec<Box<dyn LibLoader>>,
    sources: std::collections::BTreeMap<Symbol, CatalogSource>,
}

impl LoaderRegistry {
    /// Creates an empty loader registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a loader, builder-style.
    pub fn with_loader(mut self, loader: impl LibLoader + 'static) -> Self {
        self.loaders.push(Box::new(loader));
        self
    }

    /// Adds a loader.
    pub fn add_loader(&mut self, loader: impl LibLoader + 'static) {
        self.loaders.push(Box::new(loader));
    }

    /// Registers a catalog source for a library symbol, builder-style.
    pub fn with_source(mut self, symbol: Symbol, source: CatalogSource) -> Self {
        self.sources.insert(symbol, source);
        self
    }

    /// Registers a catalog source for a library symbol.
    pub fn add_source(&mut self, symbol: Symbol, source: CatalogSource) {
        self.sources.insert(symbol, source);
    }

    /// Loads a library, resolving catalog symbols and dispatching to the first
    /// accepting loader.
    pub fn load_lib(&self, cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>> {
        if let LibSource::Symbol(symbol) = &source
            && let Some(resolved) = self.sources.get(symbol).cloned()
        {
            return self.load_lib(cx, resolved.into());
        }
        for loader in &self.loaders {
            if loader.can_load(&source) {
                return loader.load(cx, source);
            }
        }
        match source {
            LibSource::Symbol(symbol) => Err(Error::HostError(format!(
                "no loader accepted lib source symbol {}",
                symbol
            ))),
            _ => Err(Error::HostError("no loader accepted lib source".to_owned())),
        }
    }

    /// Loads a library and registers it into the registry, returning its id.
    pub fn load_and_register(&self, cx: &mut Cx, source: LibSource) -> Result<crate::LibId> {
        let lib = self.load_lib(cx, source)?;
        cx.load_lib(lib.as_ref())
    }

    /// Resolves a data-only source spec through this registry's catalog.
    pub fn resolve_source_spec(&self, source: &LibSourceSpec) -> LibSourceSpec {
        match source {
            LibSourceSpec::Symbol(symbol) => self
                .sources
                .get(symbol)
                .cloned()
                .map(LibSourceSpec::from)
                .unwrap_or_else(|| source.clone()),
            _ => source.clone(),
        }
    }

    /// Loads a data-only source and returns the boot receipt for the committed
    /// library.
    pub fn load_and_register_with_receipt(
        &self,
        cx: &mut Cx,
        source: LibSourceSpec,
    ) -> Result<LibBootReceipt> {
        let resolved = self.resolve_source_spec(&source);
        self.load_and_register_from_specs(cx, source, resolved)
    }

    /// Replays load-order boot receipts, verifying that each replayed library
    /// produces the same receipt data.
    pub fn replay_boot_receipts(
        &self,
        cx: &mut Cx,
        receipts: &[LibBootReceipt],
    ) -> Result<Vec<LibBootReceipt>> {
        receipts
            .iter()
            .map(|expected| {
                let actual = self.load_and_register_from_specs(
                    cx,
                    expected.requested_source.clone(),
                    expected.resolved_source.clone(),
                )?;
                if &actual == expected {
                    Ok(actual)
                } else {
                    Err(Error::Lib(format!(
                        "boot receipt replay mismatch for {}",
                        expected.manifest.id
                    )))
                }
            })
            .collect()
    }

    /// Replays a registry boot state and returns the receipts produced by the
    /// replay.
    pub fn replay_boot_state(
        &self,
        cx: &mut Cx,
        state: &RegistryBootState,
    ) -> Result<Vec<LibBootReceipt>> {
        self.replay_boot_receipts(cx, &state.receipts)
    }

    /// Inspects a source's manifest without loading it, resolving catalog
    /// symbols first.
    pub fn inspect_manifest(&self, cx: &mut Cx, source: LibSource) -> Result<LibManifest> {
        if let LibSource::Symbol(symbol) = &source
            && let Some(resolved) = self.sources.get(symbol).cloned()
        {
            return self.inspect_manifest(cx, resolved.into());
        }
        for loader in &self.loaders {
            if loader.can_load(&source) {
                if let Some(manifest) = loader.inspect_manifest(cx, &source)? {
                    return Ok(manifest);
                }
                return Err(Error::HostError(
                    "loader does not support manifest inspection before load".to_owned(),
                ));
            }
        }
        match source {
            LibSource::Symbol(symbol) => Err(Error::HostError(format!(
                "no loader accepted lib source symbol {}",
                symbol
            ))),
            _ => Err(Error::HostError(
                "no loader accepted lib source for inspection".to_owned(),
            )),
        }
    }

    fn load_and_register_from_specs(
        &self,
        cx: &mut Cx,
        requested: LibSourceSpec,
        resolved: LibSourceSpec,
    ) -> Result<LibBootReceipt> {
        let lib = self.load_lib(cx, resolved.clone().into())?;
        let lib_id = cx.load_lib(lib.as_ref())?;
        cx.registry()
            .boot_receipt(lib_id, requested, resolved)
            .ok_or_else(|| Error::Lib(format!("loaded lib id {lib_id:?} is not registered")))
    }
}
