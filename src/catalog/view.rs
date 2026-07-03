use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    Cx, Error, Expr, Result, Symbol, Value,
    capability::registry_catalog_read_capability,
    id::CORE_TABLE_CLASS_ID,
    object::{ClassRef, Object},
    table::{Dir, Table},
};

use super::{CatalogSnapshot, CatalogSnapshotRow};

// sim-non-citizen(reason = "read-only registry catalog view; descriptor data is the catalog snapshot table path", kind = "handle", descriptor = "core/Table")
/// Read-only directory view over a [`CatalogSnapshot`], navigating table names
/// as a nested [`Dir`]/[`Table`] tree.
///
/// All reads require the `registry.catalog.read` capability; writes fail closed
/// with [`Error::CatalogReadOnly`]. See the README section "Read-only table
/// views".
#[derive(Clone, Debug)]
pub struct CatalogDirView {
    snapshot: CatalogSnapshot,
    path: Vec<Symbol>,
}

impl CatalogDirView {
    /// Creates a view rooted at the top of `snapshot`.
    pub fn new(snapshot: CatalogSnapshot) -> Self {
        Self {
            snapshot,
            path: Vec::new(),
        }
    }

    fn at_path(snapshot: CatalogSnapshot, path: Vec<Symbol>) -> Self {
        Self { snapshot, path }
    }

    /// Returns the snapshot this view reads from.
    pub fn snapshot(&self) -> &CatalogSnapshot {
        &self.snapshot
    }

    fn table_for_path(&self, path: &[Symbol]) -> Option<Symbol> {
        table_symbol_for_path(path).filter(|table| self.snapshot.tables.contains_key(table))
    }

    fn has_namespace_for_path(&self, path: &[Symbol]) -> bool {
        self.snapshot
            .tables
            .keys()
            .map(table_path)
            .any(|table_path| path_is_prefix(path, &table_path) && path.len() < table_path.len())
    }

    fn child_path(&self, name: &Symbol) -> Vec<Symbol> {
        self.path.iter().cloned().chain(symbol_path(name)).collect()
    }

    fn child_names(&self) -> Vec<Symbol> {
        let names = self
            .snapshot
            .tables
            .keys()
            .map(table_path)
            .filter_map(|table_path| {
                path_is_prefix(&self.path, &table_path)
                    .then(|| table_path.get(self.path.len()).cloned())
                    .flatten()
            })
            .collect::<BTreeSet<_>>();
        names.into_iter().collect()
    }

    fn child_value(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        let path = self.child_path(&name);
        if let Some(table) = self.table_for_path(&path) {
            return cx
                .factory()
                .opaque(Arc::new(CatalogTableView {
                    snapshot: self.snapshot.clone(),
                    table,
                }))
                .map(Some);
        }
        if self.has_namespace_for_path(&path) {
            return cx
                .factory()
                .opaque(Arc::new(Self::at_path(self.snapshot.clone(), path)))
                .map(Some);
        }
        Ok(None)
    }

    fn read_only_error(&self) -> Error {
        Error::CatalogReadOnly {
            table: Symbol::new("registry-catalog-view"),
        }
    }
}

impl Object for CatalogDirView {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("catalog-dir[{}]", path_display(&self.path)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for CatalogDirView {
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

    fn truth(&self, cx: &mut Cx) -> Result<bool> {
        Ok(!self.is_empty(cx)?)
    }

    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }

    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for CatalogDirView {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("catalog", "dir-view")
    }

    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        cx.require(&registry_catalog_read_capability())?;
        match self.child_value(cx, key)? {
            Some(value) => Ok(value),
            None => cx.factory().nil(),
        }
    }

    fn set(&self, _cx: &mut Cx, _key: Symbol, _value: Value) -> Result<()> {
        Err(self.read_only_error())
    }

    fn has(&self, cx: &mut Cx, key: Symbol) -> Result<bool> {
        cx.require(&registry_catalog_read_capability())?;
        let path = self.child_path(&key);
        Ok(self.table_for_path(&path).is_some() || self.has_namespace_for_path(&path))
    }

    fn del(&self, _cx: &mut Cx, _key: Symbol) -> Result<Value> {
        Err(self.read_only_error())
    }

    fn keys(&self, cx: &mut Cx) -> Result<Vec<Symbol>> {
        cx.require(&registry_catalog_read_capability())?;
        Ok(self.child_names())
    }

    fn entries(&self, cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        cx.require(&registry_catalog_read_capability())?;
        self.child_names()
            .into_iter()
            .filter_map(|key| {
                self.child_value(cx, key.clone())
                    .transpose()
                    .map(|value| value.map(|value| (key, value)))
            })
            .collect()
    }

    fn len(&self, cx: &mut Cx) -> Result<usize> {
        cx.require(&registry_catalog_read_capability())?;
        Ok(self.child_names().len())
    }

    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        Err(self.read_only_error())
    }
}

impl Dir for CatalogDirView {
    fn mkdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
        Err(self.read_only_error())
    }

    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        cx.require(&registry_catalog_read_capability())?;
        self.child_value(cx, name)
    }

    fn rmdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
        Err(self.read_only_error())
    }

    fn is_dir(&self, cx: &mut Cx, name: Symbol) -> Result<bool> {
        cx.require(&registry_catalog_read_capability())?;
        self.has(cx, name)
    }
}

// sim-non-citizen(reason = "read-only registry catalog table view; descriptor data is the catalog snapshot table symbol", kind = "handle", descriptor = "core/Table")
/// Read-only [`Table`] view over the rows of one catalog table in a snapshot.
///
/// Reads require `registry.catalog.read`; missing `get` returns `nil`, keys and
/// entries are sorted by [`Symbol`], and writes fail closed with
/// [`Error::CatalogReadOnly`].
#[derive(Clone, Debug)]
pub struct CatalogTableView {
    snapshot: CatalogSnapshot,
    table: Symbol,
}

impl CatalogTableView {
    /// Creates a view over `table` within `snapshot`.
    pub fn new(snapshot: CatalogSnapshot, table: Symbol) -> Self {
        Self { snapshot, table }
    }

    /// Returns the table name this view exposes.
    pub fn table(&self) -> &Symbol {
        &self.table
    }

    fn rows(&self) -> impl Iterator<Item = (&Symbol, &CatalogSnapshotRow)> {
        self.snapshot
            .rows(&self.table)
            .into_iter()
            .flat_map(BTreeMap::iter)
    }
}

impl Object for CatalogTableView {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("catalog-table[{}]", self.table))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for CatalogTableView {
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

    fn truth(&self, cx: &mut Cx) -> Result<bool> {
        Ok(!self.is_empty(cx)?)
    }

    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
}

impl Table for CatalogTableView {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("catalog", "table-view")
    }

    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        cx.require(&registry_catalog_read_capability())?;
        match self
            .snapshot
            .rows(&self.table)
            .and_then(|rows| rows.get(&key))
        {
            Some(row) => row_value(cx, row),
            None => cx.factory().nil(),
        }
    }

    fn set(&self, _cx: &mut Cx, _key: Symbol, _value: Value) -> Result<()> {
        Err(Error::CatalogReadOnly {
            table: self.table.clone(),
        })
    }

    fn has(&self, cx: &mut Cx, key: Symbol) -> Result<bool> {
        cx.require(&registry_catalog_read_capability())?;
        Ok(self
            .snapshot
            .rows(&self.table)
            .is_some_and(|rows| rows.contains_key(&key)))
    }

    fn del(&self, _cx: &mut Cx, _key: Symbol) -> Result<Value> {
        Err(Error::CatalogReadOnly {
            table: self.table.clone(),
        })
    }

    fn keys(&self, cx: &mut Cx) -> Result<Vec<Symbol>> {
        cx.require(&registry_catalog_read_capability())?;
        Ok(self.rows().map(|(key, _)| key.clone()).collect())
    }

    fn entries(&self, cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        cx.require(&registry_catalog_read_capability())?;
        self.rows()
            .map(|(key, row)| row_value(cx, row).map(|value| (key.clone(), value)))
            .collect()
    }

    fn len(&self, cx: &mut Cx) -> Result<usize> {
        cx.require(&registry_catalog_read_capability())?;
        Ok(self.rows().count())
    }

    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        Err(Error::CatalogReadOnly {
            table: self.table.clone(),
        })
    }
}

/// Returns a [`CatalogDirView`] over the registry's catalog snapshot, gated by
/// the `registry.catalog.read` capability.
pub fn registry_catalog_view(cx: &mut Cx) -> Result<Value> {
    cx.require(&registry_catalog_read_capability())?;
    let snapshot = cx.registry().catalog_snapshot();
    cx.factory().opaque(Arc::new(CatalogDirView::new(snapshot)))
}

fn row_value(cx: &mut Cx, row: &CatalogSnapshotRow) -> Result<Value> {
    let entries = row
        .data
        .iter()
        .map(|(field, value)| {
            cx.factory()
                .expr(value.clone())
                .map(|value| (field.clone(), value))
        })
        .collect::<Result<Vec<_>>>()?;
    cx.factory().table(entries)
}

fn symbol_path(symbol: &Symbol) -> impl Iterator<Item = Symbol> {
    symbol
        .as_qualified_str()
        .split('/')
        .map(|part| Symbol::new(part.to_owned()))
        .collect::<Vec<_>>()
        .into_iter()
}

fn table_path(table: &Symbol) -> Vec<Symbol> {
    symbol_path(table).collect()
}

fn table_symbol_for_path(path: &[Symbol]) -> Option<Symbol> {
    match path {
        [name] => Some(Symbol::new(name.as_qualified_str())),
        [namespace, name] => Some(Symbol::qualified(
            namespace.as_qualified_str(),
            name.as_qualified_str(),
        )),
        _ => None,
    }
}

fn path_is_prefix(prefix: &[Symbol], path: &[Symbol]) -> bool {
    prefix.len() <= path.len() && prefix.iter().zip(path).all(|(left, right)| left == right)
}

fn path_display(path: &[Symbol]) -> String {
    if path.is_empty() {
        "/".to_owned()
    } else {
        path.iter()
            .map(Symbol::as_qualified_str)
            .collect::<Vec<_>>()
            .join("/")
    }
}
