use std::collections::{BTreeMap, BTreeSet};

use crate::Symbol;

use super::{CatalogEvent, CatalogOp, CatalogRow, CatalogStore, CatalogTx};

/// Buffered, uncommitted catalog edits layered over a base [`CatalogStore`].
///
/// An overlay snapshots the parent's rows, sequences, and journal, accumulates
/// local puts, deletes, and sequence bumps, and is either committed back as a
/// single transaction or discarded.
#[derive(Clone, Debug)]
pub struct CatalogOverlay {
    parent_epoch: u64,
    local_rows: BTreeMap<Symbol, BTreeMap<Symbol, CatalogRow>>,
    local_deletes: BTreeMap<Symbol, BTreeSet<Symbol>>,
    local_sequences: BTreeMap<Symbol, u64>,
    rows: BTreeMap<Symbol, BTreeMap<Symbol, CatalogRow>>,
    sequences: BTreeMap<Symbol, u64>,
    journal: Vec<CatalogEvent>,
    epoch: u64,
}

impl CatalogOverlay {
    pub(crate) fn from_parent(store: &CatalogStore) -> Self {
        Self {
            parent_epoch: store.epoch,
            local_rows: BTreeMap::new(),
            local_deletes: BTreeMap::new(),
            local_sequences: BTreeMap::new(),
            rows: store.rows.clone(),
            sequences: store.sequences.clone(),
            journal: store.journal.clone(),
            epoch: store.epoch,
        }
    }

    /// Returns the parent store epoch the overlay was created from.
    pub fn parent_epoch(&self) -> u64 {
        self.parent_epoch
    }

    pub(crate) fn row(&self, table: &Symbol, key: &Symbol) -> Option<&CatalogRow> {
        self.rows.get(table).and_then(|rows| rows.get(key))
    }

    pub(crate) fn rows(&self, table: &Symbol) -> Option<&BTreeMap<Symbol, CatalogRow>> {
        self.rows.get(table)
    }

    pub(crate) fn all_rows(&self) -> &BTreeMap<Symbol, BTreeMap<Symbol, CatalogRow>> {
        &self.rows
    }

    pub(crate) fn all_sequences(&self) -> &BTreeMap<Symbol, u64> {
        &self.sequences
    }

    pub(crate) fn sequence(&self, name: &Symbol) -> Option<u64> {
        self.sequences.get(name).copied()
    }

    pub(crate) fn journal(&self) -> &[CatalogEvent] {
        &self.journal
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn bump_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    pub(crate) fn put_row(&mut self, row: CatalogRow) {
        let table = row.table.clone();
        let key = row.key.clone();
        if let Some(deleted) = self.local_deletes.get_mut(&table) {
            deleted.remove(&key);
        }
        if self
            .local_deletes
            .get(&table)
            .is_some_and(BTreeSet::is_empty)
        {
            self.local_deletes.remove(&table);
        }
        self.rows
            .entry(table.clone())
            .or_default()
            .insert(key.clone(), row.clone());
        self.local_rows.entry(table).or_default().insert(key, row);
    }

    pub(crate) fn delete_row(&mut self, table: &Symbol, key: &Symbol) {
        if let Some(rows) = self.rows.get_mut(table) {
            rows.remove(key);
            if rows.is_empty() {
                self.rows.remove(table);
            }
        }
        if let Some(rows) = self.local_rows.get_mut(table) {
            rows.remove(key);
            if rows.is_empty() {
                self.local_rows.remove(table);
            }
        }
        self.local_deletes
            .entry(table.clone())
            .or_default()
            .insert(key.clone());
    }

    pub(crate) fn bump_sequence(&mut self, name: Symbol, reserved: u64) -> u64 {
        let visible = self.sequences.entry(name.clone()).or_default();
        *visible = (*visible).max(reserved);
        let local = self.local_sequences.entry(name).or_default();
        *local = (*local).max(*visible);
        *visible
    }

    pub(crate) fn push_event(&mut self, event: CatalogEvent) {
        self.journal.push(event);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.local_rows.is_empty()
            && self.local_deletes.is_empty()
            && self.local_sequences.is_empty()
    }

    pub(crate) fn into_tx(self) -> CatalogTx {
        let mut tx = CatalogTx::new();
        for (table, keys) in self.local_deletes {
            for key in keys {
                tx.push(CatalogOp::DeleteRow {
                    table: table.clone(),
                    key,
                });
            }
        }
        for rows in self.local_rows.into_values() {
            for row in rows.into_values() {
                tx.put_row(row);
            }
        }
        for (name, reserved) in self.local_sequences {
            tx.bump_sequence(name, reserved);
        }
        tx
    }
}
