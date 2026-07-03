//! The table contract: the pluggable [`Table`] and [`Dir`] backend protocol.
//!
//! The kernel defines the table/directory protocols and a backend registry;
//! concrete table representations are libs loaded against it, with `AssocTable`
//! provided as a baseline backend rather than kernel-fixed behavior.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use crate::{
    catalog::CatalogBackend,
    env::Cx,
    error::{Error, Result},
    expr::Expr,
    id::{CORE_TABLE_CLASS_ID, Symbol},
    object::{ClassRef, Object},
    value::{RuntimeObject, Value},
};

/// Universal map surface. Keys are symbols; values are runtime values.
pub trait Table: RuntimeObject {
    /// Symbol identifying the backend representation.
    fn backend_symbol(&self) -> Symbol;

    /// Looks up `key`, returning nil when absent.
    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value>;

    /// Inserts or replaces the value for `key`.
    fn set(&self, cx: &mut Cx, key: Symbol, value: Value) -> Result<()>;

    /// Whether `key` is present.
    fn has(&self, cx: &mut Cx, key: Symbol) -> Result<bool>;

    /// Removes `key`, returning its prior value or nil.
    fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value>;

    /// All keys, in backend order.
    fn keys(&self, cx: &mut Cx) -> Result<Vec<Symbol>>;

    /// All key/value pairs, in backend order.
    fn entries(&self, cx: &mut Cx) -> Result<Vec<(Symbol, Value)>>;

    /// Number of entries.
    fn len(&self, cx: &mut Cx) -> Result<usize>;

    /// Whether the table has no entries.
    fn is_empty(&self, cx: &mut Cx) -> Result<bool> {
        Ok(self.len(cx)? == 0)
    }

    /// Removes all entries.
    fn clear(&self, cx: &mut Cx) -> Result<()>;

    /// Projects the table to an [`Expr::Map`].
    fn as_table_expr(&self, cx: &mut Cx) -> Result<Expr> {
        let entries = self.entries(cx)?;
        let mut pairs = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            pairs.push((Expr::Symbol(key), value.object().as_expr(cx)?));
        }
        Ok(Expr::Map(pairs))
    }

    /// Order-insensitive equality against another table's entries.
    fn table_eq(&self, cx: &mut Cx, other: &dyn Table) -> Result<bool> {
        let mut left = self.entries(cx)?;
        let mut right = other.entries(cx)?;
        left.sort_by(|a, b| a.0.cmp(&b.0));
        right.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(left == right)
    }
}

/// Hierarchical table surface for backends that support nested subtables.
pub trait Dir: Table {
    /// Creates a nested subtable under `name`, returning it.
    fn mkdir(&self, cx: &mut Cx, name: Symbol) -> Result<Value>;

    /// Opens the subtable at `name`, or `Ok(None)` when absent.
    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>>;

    /// Removes the subtable at `name`, returning it.
    fn rmdir(&self, cx: &mut Cx, name: Symbol) -> Result<Value>;

    /// Whether `name` resolves to a subtable.
    fn is_dir(&self, cx: &mut Cx, name: Symbol) -> Result<bool>;
}

/// Baseline table backend backed by an association list under a lock.
// sim-non-citizen(reason = "kernel table backing object; canonical form is native table entries", kind = "private", descriptor = "")
// FUTURE CLEANUP: this is the kernel's built-in REFERENCE table backend (the
// bootstrap implementation). Once a default-loaded distribution lib can supply
// a `TableBackend` before any user code runs, move `AssocTable` out of the
// kernel and keep only the `Table`/`TableBackend` contracts here.
pub struct AssocTable {
    entries: RwLock<Vec<(Symbol, Value)>>,
}

impl AssocTable {
    /// Builds an empty table.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
        }
    }

    /// Builds a table seeded with the given entries.
    pub fn with_entries(entries: Vec<(Symbol, Value)>) -> Self {
        Self {
            entries: RwLock::new(entries),
        }
    }

    fn read_entries(&self) -> Result<RwLockReadGuard<'_, Vec<(Symbol, Value)>>> {
        self.entries.read().map_err(|_| poisoned_table_error())
    }

    fn write_entries(&self) -> Result<RwLockWriteGuard<'_, Vec<(Symbol, Value)>>> {
        self.entries.write().map_err(|_| poisoned_table_error())
    }
}

impl Default for AssocTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Object for AssocTable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("table[{}]", self.read_entries()?.len()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for AssocTable {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = Symbol::qualified("core", "Table");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory().class_stub(CORE_TABLE_CLASS_ID, symbol)
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table_expr(cx)
    }
    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(!self.read_entries()?.is_empty())
    }
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
}

impl Table for AssocTable {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("core", "Table")
    }

    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        let guard = self.read_entries()?;
        match guard.iter().find(|(candidate, _)| *candidate == key) {
            Some((_, value)) => Ok(value.clone()),
            None => cx.factory().nil(),
        }
    }

    fn set(&self, _cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
        let mut guard = self.write_entries()?;
        if let Some((_, slot)) = guard.iter_mut().find(|(candidate, _)| *candidate == key) {
            *slot = value;
        } else {
            guard.push((key, value));
        }
        Ok(())
    }

    fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(self
            .read_entries()?
            .iter()
            .any(|(candidate, _)| *candidate == key))
    }

    fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        let mut guard = self.write_entries()?;
        if let Some(index) = guard.iter().position(|(candidate, _)| *candidate == key) {
            Ok(guard.remove(index).1)
        } else {
            cx.factory().nil()
        }
    }

    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(self
            .read_entries()?
            .iter()
            .map(|(key, _)| key.clone())
            .collect())
    }

    fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        Ok(self.read_entries()?.clone())
    }

    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(self.read_entries()?.len())
    }

    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        self.write_entries()?.clear();
        Ok(())
    }
}

fn poisoned_table_error() -> Error {
    Error::Eval("assoc table lock poisoned".to_owned())
}

/// Factory protocol for constructing tables in a particular representation.
pub trait TableBackend: Send + Sync {
    /// Stable name the backend is registered and selected under.
    fn name(&self) -> &str;

    /// Builds a table from an initial set of entries.
    fn new_table(&self, cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value>;
}

/// Registry of named table backends with one active default.
pub struct TableRegistry {
    backends: BTreeMap<String, Arc<dyn TableBackend>>,
    active: String,
}

impl TableRegistry {
    /// Builds a registry preloaded with the `assoc` and catalog backends.
    pub fn new() -> Self {
        let mut registry = Self {
            backends: BTreeMap::new(),
            active: "assoc".to_owned(),
        };
        registry.register(Arc::new(AssocBackend));
        registry.register(Arc::new(CatalogBackend));
        registry
    }

    /// Registers a backend under its own name, replacing any prior one.
    pub fn register(&mut self, backend: Arc<dyn TableBackend>) {
        self.backends.insert(backend.name().to_owned(), backend);
    }

    /// Selects the active backend by name, erroring if it is unknown.
    pub fn set_active(&mut self, name: &str) -> Result<()> {
        if self.backends.contains_key(name) {
            self.active = name.to_owned();
            Ok(())
        } else {
            Err(Error::Eval(format!("unknown table backend: {name}")))
        }
    }

    /// Name of the currently active backend.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Builds a table using the active backend.
    pub fn new_table(&self, cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
        self.backend()?.new_table(cx, entries)
    }

    fn backend(&self) -> Result<&Arc<dyn TableBackend>> {
        self.backends
            .get(&self.active)
            .ok_or_else(|| Error::Eval("active table backend missing".to_owned()))
    }
}

impl Default for TableRegistry {
    fn default() -> Self {
        Self::new()
    }
}

struct AssocBackend;

impl TableBackend for AssocBackend {
    fn name(&self) -> &str {
        "assoc"
    }

    fn new_table(&self, cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
        cx.factory()
            .opaque(Arc::new(AssocTable::with_entries(entries)))
    }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod table_tests;
