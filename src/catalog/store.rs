use std::collections::BTreeMap;

use crate::{Error, Result, Symbol};

use super::{CatalogEvent, CatalogOverlay, CatalogRow, CatalogTableSpec};

/// `Cx`-free, lock-free transactional table storage owned by the registry.
///
/// The store holds table specs, rows by table and key, named sequences, the
/// append-only journal, and the current epoch. It is the authoritative catalog
/// storage described in the README section "Contract: registry catalog
/// substrate"; mutation flows through [`CatalogTx`](super::CatalogTx) commits,
/// optionally buffered through an overlay.
///
/// # Examples
///
/// ```
/// # use sim_kernel::catalog::{CatalogRow, CatalogStore, CatalogTableSpec, CatalogTx, CatalogWritePolicy};
/// # use sim_kernel::Symbol;
/// let mut store = CatalogStore::new();
/// let table = Symbol::new("registry/libs");
/// store.install_table(CatalogTableSpec::new(table.clone(), CatalogWritePolicy::Mutable)).unwrap();
///
/// let mut tx = CatalogTx::new();
/// tx.put_row(CatalogRow::new(table.clone(), Symbol::new("demo")));
/// let epoch = tx.commit(&mut store).unwrap();
///
/// assert_eq!(store.epoch(), epoch);
/// assert!(store.row(&table, &Symbol::new("demo")).is_some());
/// ```
#[derive(Clone, Debug, Default)]
pub struct CatalogStore {
    pub(crate) tables: BTreeMap<Symbol, CatalogTableSpec>,
    pub(crate) rows: BTreeMap<Symbol, BTreeMap<Symbol, CatalogRow>>,
    pub(crate) sequences: BTreeMap<Symbol, u64>,
    pub(crate) journal: Vec<CatalogEvent>,
    pub(crate) epoch: u64,
    pub(crate) overlay: Option<CatalogOverlay>,
}

impl CatalogStore {
    /// Creates an empty store with no tables, rows, or sequences.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a table spec, erroring if a table of that name already exists.
    pub fn install_table(&mut self, spec: CatalogTableSpec) -> Result<()> {
        if self.tables.contains_key(&spec.name) {
            return Err(Error::CatalogConflict {
                table: spec.name.clone(),
                key: spec.name,
            });
        }
        self.tables.insert(spec.name.clone(), spec);
        Ok(())
    }

    /// Returns the spec for `name`, if the table is installed.
    pub fn table(&self, name: &Symbol) -> Option<&CatalogTableSpec> {
        self.tables.get(name)
    }

    /// Returns all installed table specs by name.
    pub fn tables(&self) -> &BTreeMap<Symbol, CatalogTableSpec> {
        &self.tables
    }

    /// Returns the row at `table`/`key`, reading through any active overlay.
    pub fn row(&self, table: &Symbol, key: &Symbol) -> Option<&CatalogRow> {
        if let Some(overlay) = &self.overlay {
            return overlay.row(table, key);
        }
        self.rows.get(table).and_then(|rows| rows.get(key))
    }

    /// Returns all rows of `table` by key, reading through any active overlay.
    pub fn rows(&self, table: &Symbol) -> Option<&BTreeMap<Symbol, CatalogRow>> {
        if let Some(overlay) = &self.overlay {
            return overlay.rows(table);
        }
        self.rows.get(table)
    }

    /// Returns the current value of sequence `name`, if any.
    pub fn sequence(&self, name: &Symbol) -> Option<u64> {
        if let Some(overlay) = &self.overlay {
            return overlay.sequence(name);
        }
        self.sequences.get(name).copied()
    }

    /// Returns the append-only audit journal, reading through any active overlay.
    pub fn journal(&self) -> &[CatalogEvent] {
        if let Some(overlay) = &self.overlay {
            return overlay.journal();
        }
        &self.journal
    }

    /// Returns the current catalog epoch, reading through any active overlay.
    pub fn epoch(&self) -> u64 {
        if let Some(overlay) = &self.overlay {
            return overlay.epoch();
        }
        self.epoch
    }

    /// Runs `f` against a buffered overlay, committing its edits on success and
    /// rolling them back on error.
    pub fn with_overlay<F, R>(&mut self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Self) -> Result<R>,
    {
        self.begin_overlay()?;
        match f(self) {
            Ok(value) => {
                self.commit_overlay()?;
                Ok(value)
            }
            Err(err) => {
                self.rollback_overlay();
                Err(err)
            }
        }
    }

    pub(crate) fn begin_overlay(&mut self) -> Result<()> {
        if self.overlay.is_some() {
            return Err(overlay_error("nested catalog overlays are not supported"));
        }
        self.overlay = Some(CatalogOverlay::from_parent(self));
        Ok(())
    }

    pub(crate) fn rollback_overlay(&mut self) {
        self.overlay = None;
    }

    pub(crate) fn commit_overlay(&mut self) -> Result<()> {
        let Some(overlay) = self.overlay.take() else {
            return Err(overlay_error("no catalog overlay is active"));
        };
        if overlay.parent_epoch() != self.epoch {
            return Err(overlay_error("catalog overlay parent epoch changed"));
        }
        if overlay.is_empty() {
            return Ok(());
        }
        overlay.into_tx().commit(self).map(|_| ())
    }

    pub(crate) fn bump_epoch(&mut self) -> u64 {
        if let Some(overlay) = &mut self.overlay {
            return overlay.bump_epoch();
        }
        self.epoch += 1;
        self.epoch
    }

    pub(crate) fn put_row(&mut self, row: CatalogRow) {
        if let Some(overlay) = &mut self.overlay {
            overlay.put_row(row);
            return;
        }
        self.rows
            .entry(row.table.clone())
            .or_default()
            .insert(row.key.clone(), row);
    }

    pub(crate) fn delete_row(&mut self, table: &Symbol, key: &Symbol) {
        if let Some(overlay) = &mut self.overlay {
            overlay.delete_row(table, key);
            return;
        }
        if let Some(rows) = self.rows.get_mut(table) {
            rows.remove(key);
            if rows.is_empty() {
                self.rows.remove(table);
            }
        }
    }

    pub(crate) fn bump_sequence(&mut self, name: Symbol, reserved: u64) -> u64 {
        if let Some(overlay) = &mut self.overlay {
            return overlay.bump_sequence(name, reserved);
        }
        let slot = self.sequences.entry(name).or_default();
        *slot = (*slot).max(reserved);
        *slot
    }

    pub(crate) fn push_event(&mut self, event: CatalogEvent) {
        if let Some(overlay) = &mut self.overlay {
            overlay.push_event(event);
            return;
        }
        self.journal.push(event);
    }
}

fn overlay_error(message: &'static str) -> Error {
    Error::CatalogSchema {
        table: Symbol::qualified("catalog", "overlay"),
        message: message.to_owned(),
    }
}
