use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
    Cx, Error, Expr, Result, Symbol, Value,
    id::CORE_TABLE_CLASS_ID,
    object::{ClassRef, Object},
    table::{Table, TableBackend},
};

use super::{
    CatalogRow, CatalogSnapshot, CatalogStore, CatalogTableSpec, CatalogTx, CatalogWritePolicy,
};

// sim-non-citizen(reason = "mutable registry catalog table handle; descriptor data is exposed through catalog view rows", kind = "handle", descriptor = "core/Table")
/// Mutable runtime [`Table`] object wrapping a private [`CatalogStore`].
///
/// This is the opt-in catalog semantics for ordinary runtime users who want a
/// transactional table without registry authority; it is a library-style
/// backend, not kernel registry behavior.
pub struct CatalogTable {
    store: RwLock<CatalogStore>,
}

impl CatalogTable {
    /// Creates an empty catalog-backed table.
    pub fn new() -> Result<Self> {
        Ok(Self {
            store: RwLock::new(new_store()?),
        })
    }

    /// Creates a catalog-backed table seeded with `entries`.
    pub fn with_entries(entries: Vec<(Symbol, Value)>) -> Result<Self> {
        let table = Self::new()?;
        for (key, value) in entries {
            table.put_value(key, value)?;
        }
        Ok(table)
    }

    /// Returns a snapshot of the table's underlying catalog store.
    pub fn snapshot(&self) -> Result<CatalogSnapshot> {
        self.read_store()
            .map(|store| CatalogSnapshot::from_store(&store))
    }

    fn put_value(&self, key: Symbol, value: Value) -> Result<()> {
        let mut tx = CatalogTx::new();
        tx.put_row(row_for_value(key, value));
        let mut store = self.write_store()?;
        tx.commit(&mut store).map(|_| ())
    }

    fn read_store(&self) -> Result<RwLockReadGuard<'_, CatalogStore>> {
        self.store
            .read()
            .map_err(|_| Error::PoisonedLock("catalog table"))
    }

    fn write_store(&self) -> Result<RwLockWriteGuard<'_, CatalogStore>> {
        self.store
            .write()
            .map_err(|_| Error::PoisonedLock("catalog table"))
    }
}

impl Object for CatalogTable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        let len = self
            .read_store()?
            .rows(&entries_table())
            .map_or(0, |rows| rows.len());
        Ok(format!("catalog-table[{len}]"))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for CatalogTable {
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

impl Table for CatalogTable {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("table", "catalog")
    }

    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        match self
            .read_store()?
            .row(&entries_table(), &key)
            .map(|row| row_value(cx, row))
            .transpose()?
        {
            Some(value) => Ok(value),
            None => cx.factory().nil(),
        }
    }

    fn set(&self, _cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
        self.put_value(key, value)
    }

    fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(self.read_store()?.row(&entries_table(), &key).is_some())
    }

    fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        let mut store = self.write_store()?;
        let Some(value) = store
            .row(&entries_table(), &key)
            .map(|row| row_value(cx, row))
            .transpose()?
        else {
            return cx.factory().nil();
        };

        let mut tx = CatalogTx::new();
        tx.delete_row(entries_table(), key);
        tx.commit(&mut store)?;
        Ok(value)
    }

    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(self
            .read_store()?
            .rows(&entries_table())
            .map(|rows| rows.keys().cloned().collect())
            .unwrap_or_default())
    }

    fn entries(&self, cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        self.read_store()?
            .rows(&entries_table())
            .into_iter()
            .flat_map(|rows| rows.iter())
            .map(|(key, row)| row_value(cx, row).map(|value| (key.clone(), value)))
            .collect()
    }

    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(self
            .read_store()?
            .rows(&entries_table())
            .map_or(0, |rows| rows.len()))
    }

    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        let keys = self
            .read_store()?
            .rows(&entries_table())
            .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        if keys.is_empty() {
            return Ok(());
        }

        let mut tx = CatalogTx::new();
        for key in keys {
            tx.delete_row(entries_table(), key);
        }
        let mut store = self.write_store()?;
        tx.commit(&mut store).map(|_| ())
    }
}

/// [`TableBackend`] that constructs [`CatalogTable`]s under the
/// `table/catalog` backend name.
pub struct CatalogBackend;

impl TableBackend for CatalogBackend {
    fn name(&self) -> &str {
        "table/catalog"
    }

    fn new_table(&self, cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
        cx.factory()
            .opaque(Arc::new(CatalogTable::with_entries(entries)?))
    }
}

fn new_store() -> Result<CatalogStore> {
    let mut store = CatalogStore::new();
    store.install_table(
        CatalogTableSpec::new(entries_table(), CatalogWritePolicy::Mutable)
            .with_required_fields(vec![field("key"), field("value")]),
    )?;
    Ok(store)
}

fn row_for_value(key: Symbol, value: Value) -> CatalogRow {
    let mut row =
        CatalogRow::new(entries_table(), key.clone()).with_data(field("key"), Expr::Symbol(key));
    row.insert_live_value(field("value"), value);
    row
}

fn row_value(cx: &mut Cx, row: &CatalogRow) -> Result<Value> {
    if let Some(value) = row.live_value(&field("value")) {
        Ok(value.clone())
    } else if let Some(expr) = row.data.get(&field("value")) {
        cx.factory().expr(expr.clone())
    } else {
        cx.factory().nil()
    }
}

fn entries_table() -> Symbol {
    Symbol::qualified("table", "entries")
}

fn field(name: &'static str) -> Symbol {
    Symbol::new(name)
}
