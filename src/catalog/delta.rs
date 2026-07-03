use std::collections::BTreeMap;

use crate::{Error, Result, Symbol};

use super::{
    CatalogEvent, CatalogEventOp, CatalogRow, CatalogSnapshot, CatalogSnapshotRow, CatalogStore,
    CatalogTableSpec, CatalogWritePolicy,
};

/// The change set between two catalog epochs: table specs, changed and deleted
/// rows, and sequence changes.
///
/// Produced by [`CatalogStore::delta_since`] and applied by
/// [`CatalogStore::apply_delta`]. See the README section "Snapshots and deltas".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogDelta {
    /// Epoch the delta starts from (exclusive, or 0 for a full delta).
    pub from_epoch: u64,
    /// Epoch the delta brings the store to.
    pub to_epoch: u64,
    /// Table specs that must exist or match in the target store.
    pub table_specs: BTreeMap<Symbol, CatalogTableSpec>,
    /// Rows inserted or replaced, by table and key.
    pub rows_changed: BTreeMap<Symbol, BTreeMap<Symbol, CatalogSnapshotRow>>,
    /// Rows deleted, by table and key.
    pub rows_deleted: BTreeMap<Symbol, BTreeMap<Symbol, CatalogDeletedRow>>,
    /// Sequence advances, by sequence name.
    pub sequence_changes: BTreeMap<Symbol, CatalogSequenceChange>,
}

/// A row removed by a [`CatalogDelta`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogDeletedRow {
    /// Epoch at which the deletion occurred.
    pub epoch: u64,
    /// Table the row was deleted from.
    pub table: Symbol,
    /// Key of the deleted row.
    pub key: Symbol,
}

/// A sequence advance carried by a [`CatalogDelta`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSequenceChange {
    /// Epoch at which the sequence advanced.
    pub epoch: u64,
    /// Sequence name.
    pub name: Symbol,
    /// New sequence value.
    pub next: u64,
}

impl CatalogStore {
    /// Computes the delta carrying this store from `from_epoch` to its current
    /// epoch; `from_epoch == 0` yields a full delta.
    pub fn delta_since(&self, from_epoch: u64) -> Result<CatalogDelta> {
        let to_epoch = self.epoch();
        if from_epoch > to_epoch {
            return Err(delta_error(
                "delta source epoch is newer than catalog epoch",
            ));
        }

        let snapshot = CatalogSnapshot::from_store(self);
        let mut rows_changed = BTreeMap::new();
        let mut rows_deleted = BTreeMap::new();
        let mut sequence_changes = BTreeMap::new();

        if from_epoch == 0 {
            rows_changed = snapshot.rows.clone();
            for (name, next) in &snapshot.sequences {
                sequence_changes.insert(
                    name.clone(),
                    CatalogSequenceChange {
                        epoch: to_epoch,
                        name: name.clone(),
                        next: *next,
                    },
                );
            }
        }

        for event in self
            .journal()
            .iter()
            .filter(|event| event.epoch > from_epoch)
        {
            match &event.op {
                CatalogEventOp::PutRow { table, key } => {
                    remove_deleted_row(&mut rows_deleted, table, key);
                    if let Some(row) = snapshot.rows(table).and_then(|rows| rows.get(key)) {
                        rows_changed
                            .entry(table.clone())
                            .or_default()
                            .insert(key.clone(), row.clone());
                    }
                }
                CatalogEventOp::DeleteRow { table, key } => {
                    remove_changed_row(&mut rows_changed, table, key);
                    rows_deleted.entry(table.clone()).or_default().insert(
                        key.clone(),
                        CatalogDeletedRow {
                            epoch: event.epoch,
                            table: table.clone(),
                            key: key.clone(),
                        },
                    );
                }
                CatalogEventOp::Sequence { name, next } => {
                    sequence_changes.insert(
                        name.clone(),
                        CatalogSequenceChange {
                            epoch: event.epoch,
                            name: name.clone(),
                            next: *next,
                        },
                    );
                }
            }
        }

        Ok(CatalogDelta {
            from_epoch,
            to_epoch,
            table_specs: snapshot.tables,
            rows_changed,
            rows_deleted,
            sequence_changes,
        })
    }

    /// Applies `delta` atomically after validating source epoch, table
    /// compatibility, sealed-row conflicts, change epochs, and target epoch;
    /// the store is left unchanged on error.
    pub fn apply_delta(&mut self, delta: CatalogDelta) -> Result<()> {
        let mut next = self.clone();
        next.apply_delta_in_place(delta)?;
        *self = next;
        Ok(())
    }

    fn apply_delta_in_place(&mut self, delta: CatalogDelta) -> Result<()> {
        if self.overlay.is_some() {
            return Err(delta_error(
                "cannot apply catalog delta while overlay is active",
            ));
        }
        if self.epoch != delta.from_epoch {
            return Err(delta_error("catalog delta source epoch mismatch"));
        }
        if delta.to_epoch < delta.from_epoch {
            return Err(delta_error(
                "catalog delta target epoch precedes source epoch",
            ));
        }

        for spec in delta.table_specs.values() {
            self.install_or_validate_delta_table(spec)?;
        }
        self.validate_delta_rows(&delta)?;
        self.validate_delta_deletes(&delta)?;
        validate_delta_sequences(&delta)?;

        for rows in delta.rows_changed.values() {
            for row in rows.values() {
                self.apply_delta_row(row.clone());
            }
        }
        for rows in delta.rows_deleted.values() {
            for deleted in rows.values() {
                self.apply_delta_delete(deleted);
            }
        }
        for change in delta.sequence_changes.values() {
            self.sequences.insert(change.name.clone(), change.next);
            self.journal.push(CatalogEvent {
                epoch: change.epoch,
                op: CatalogEventOp::Sequence {
                    name: change.name.clone(),
                    next: change.next,
                },
            });
        }

        self.epoch = delta.to_epoch;
        if self.epoch == delta.to_epoch {
            Ok(())
        } else {
            Err(delta_error("catalog delta target epoch was not reached"))
        }
    }

    fn install_or_validate_delta_table(&mut self, spec: &CatalogTableSpec) -> Result<()> {
        match self.tables.get(&spec.name) {
            Some(existing) if existing == spec => Ok(()),
            Some(_) => Err(Error::CatalogSchema {
                table: spec.name.clone(),
                message: "incompatible catalog table spec".to_owned(),
            }),
            None => {
                self.tables.insert(spec.name.clone(), spec.clone());
                Ok(())
            }
        }
    }

    fn validate_delta_rows(&self, delta: &CatalogDelta) -> Result<()> {
        for (table, rows) in &delta.rows_changed {
            let spec = self.table(table).ok_or_else(|| Error::CatalogSchema {
                table: table.clone(),
                message: "unknown catalog table".to_owned(),
            })?;
            for (key, row) in rows {
                validate_row_key(table, key, row)?;
                validate_change_epoch(row.epoch, delta, table)?;
                validate_required_fields(spec, row)?;
                if spec.policy == CatalogWritePolicy::Sealed && self.row(table, key).is_some() {
                    return Err(Error::CatalogConflict {
                        table: table.clone(),
                        key: key.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_delta_deletes(&self, delta: &CatalogDelta) -> Result<()> {
        for (table, rows) in &delta.rows_deleted {
            if self.table(table).is_none() {
                return Err(Error::CatalogSchema {
                    table: table.clone(),
                    message: "unknown catalog table".to_owned(),
                });
            }
            for (key, deleted) in rows {
                if &deleted.table != table || &deleted.key != key {
                    return Err(Error::CatalogSchema {
                        table: table.clone(),
                        message: "deleted row key does not match row data".to_owned(),
                    });
                }
                validate_change_epoch(deleted.epoch, delta, table)?;
            }
        }
        Ok(())
    }

    fn apply_delta_row(&mut self, snapshot_row: CatalogSnapshotRow) {
        let mut row = CatalogRow::new(snapshot_row.table.clone(), snapshot_row.key.clone());
        row.data = snapshot_row.data;
        row.set_epoch(snapshot_row.epoch);
        self.rows
            .entry(row.table.clone())
            .or_default()
            .insert(row.key.clone(), row);
        self.journal.push(CatalogEvent {
            epoch: snapshot_row.epoch,
            op: CatalogEventOp::PutRow {
                table: snapshot_row.table,
                key: snapshot_row.key,
            },
        });
    }

    fn apply_delta_delete(&mut self, deleted: &CatalogDeletedRow) {
        if let Some(rows) = self.rows.get_mut(&deleted.table) {
            rows.remove(&deleted.key);
            if rows.is_empty() {
                self.rows.remove(&deleted.table);
            }
        }
        self.journal.push(CatalogEvent {
            epoch: deleted.epoch,
            op: CatalogEventOp::DeleteRow {
                table: deleted.table.clone(),
                key: deleted.key.clone(),
            },
        });
    }
}

fn validate_row_key(table: &Symbol, key: &Symbol, row: &CatalogSnapshotRow) -> Result<()> {
    if &row.table == table && &row.key == key {
        Ok(())
    } else {
        Err(Error::CatalogSchema {
            table: table.clone(),
            message: "changed row key does not match row data".to_owned(),
        })
    }
}

fn validate_required_fields(spec: &CatalogTableSpec, row: &CatalogSnapshotRow) -> Result<()> {
    for field in &spec.required_fields {
        if !row.data.contains_key(field) {
            return Err(Error::CatalogSchema {
                table: row.table.clone(),
                message: format!("missing required catalog field {field}"),
            });
        }
    }
    Ok(())
}

fn validate_delta_sequences(delta: &CatalogDelta) -> Result<()> {
    for change in delta.sequence_changes.values() {
        validate_change_epoch(change.epoch, delta, &change.name)?;
    }
    Ok(())
}

fn validate_change_epoch(epoch: u64, delta: &CatalogDelta, table: &Symbol) -> Result<()> {
    if epoch <= delta.to_epoch && (epoch > delta.from_epoch || delta.from_epoch == 0) {
        Ok(())
    } else {
        Err(Error::CatalogSchema {
            table: table.clone(),
            message: "catalog delta change epoch is outside delta bounds".to_owned(),
        })
    }
}

fn remove_changed_row(
    rows_changed: &mut BTreeMap<Symbol, BTreeMap<Symbol, CatalogSnapshotRow>>,
    table: &Symbol,
    key: &Symbol,
) {
    if let Some(rows) = rows_changed.get_mut(table) {
        rows.remove(key);
        if rows.is_empty() {
            rows_changed.remove(table);
        }
    }
}

fn remove_deleted_row(
    rows_deleted: &mut BTreeMap<Symbol, BTreeMap<Symbol, CatalogDeletedRow>>,
    table: &Symbol,
    key: &Symbol,
) {
    if let Some(rows) = rows_deleted.get_mut(table) {
        rows.remove(key);
        if rows.is_empty() {
            rows_deleted.remove(table);
        }
    }
}

fn delta_error(message: &'static str) -> Error {
    Error::CatalogSchema {
        table: Symbol::qualified("catalog", "delta"),
        message: message.to_owned(),
    }
}
