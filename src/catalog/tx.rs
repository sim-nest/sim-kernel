use std::collections::BTreeSet;

use crate::{Error, Result, Symbol};

use super::{
    CatalogEvent, CatalogEventOp, CatalogRow, CatalogStore, CatalogTableSpec, CatalogWritePolicy,
};

/// An atomic catalog transaction: an ordered batch of [`CatalogOp`]s applied
/// together against a [`CatalogStore`].
///
/// # Examples
///
/// ```
/// # use sim_kernel::catalog::{CatalogRow, CatalogTx};
/// # use sim_kernel::Symbol;
/// let mut tx = CatalogTx::new();
/// tx.put_row(CatalogRow::new(Symbol::new("registry/libs"), Symbol::new("demo")));
/// assert_eq!(tx.ops().len(), 1);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogTx {
    ops: Vec<CatalogOp>,
}

impl CatalogTx {
    /// Creates an empty transaction.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one operation to the transaction.
    pub fn push(&mut self, op: CatalogOp) {
        self.ops.push(op);
    }

    /// Queues a row insert or replace.
    pub fn put_row(&mut self, row: CatalogRow) {
        self.push(CatalogOp::PutRow(row));
    }

    /// Queues deletion of the row at `table`/`key`.
    pub fn delete_row(&mut self, table: Symbol, key: Symbol) {
        self.push(CatalogOp::DeleteRow { table, key });
    }

    /// Queues reserving `reserved` ids from the named sequence.
    pub fn bump_sequence(&mut self, name: Symbol, reserved: u64) {
        self.push(CatalogOp::BumpSequence { name, reserved });
    }

    /// Returns the queued operations in order.
    pub fn ops(&self) -> &[CatalogOp] {
        &self.ops
    }

    /// Checks the transaction against `store` without mutating it, enforcing
    /// table existence, write policies, required fields, and per-row uniqueness.
    pub fn validate(&self, store: &CatalogStore) -> Result<()> {
        let mut touched_rows = BTreeSet::new();
        for op in &self.ops {
            match op {
                CatalogOp::PutRow(row) => {
                    validate_unique_row_op(&mut touched_rows, &row.table, &row.key)?;
                    let spec = table_spec(store, &row.table)?;
                    validate_put(store, spec, row)?;
                }
                CatalogOp::DeleteRow { table, key } => {
                    validate_unique_row_op(&mut touched_rows, table, key)?;
                    let spec = table_spec(store, table)?;
                    validate_delete(spec)?;
                }
                CatalogOp::BumpSequence { .. } => {}
            }
        }
        Ok(())
    }

    /// Validates and applies the transaction, returning the new catalog epoch
    /// and recording an audit event per operation.
    pub fn commit(self, store: &mut CatalogStore) -> Result<u64> {
        self.validate(store)?;
        let epoch = store.bump_epoch();
        for op in self.ops {
            match op {
                CatalogOp::PutRow(mut row) => {
                    row.set_epoch(epoch);
                    let table = row.table.clone();
                    let key = row.key.clone();
                    store.put_row(row);
                    store.push_event(CatalogEvent {
                        epoch,
                        op: CatalogEventOp::PutRow { table, key },
                    });
                }
                CatalogOp::DeleteRow { table, key } => {
                    store.delete_row(&table, &key);
                    store.push_event(CatalogEvent {
                        epoch,
                        op: CatalogEventOp::DeleteRow { table, key },
                    });
                }
                CatalogOp::BumpSequence { name, reserved } => {
                    let next = store.bump_sequence(name.clone(), reserved);
                    store.push_event(CatalogEvent {
                        epoch,
                        op: CatalogEventOp::Sequence { name, next },
                    });
                }
            }
        }
        Ok(epoch)
    }
}

/// A single operation within a [`CatalogTx`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogOp {
    /// Insert or replace a row.
    PutRow(CatalogRow),
    /// Delete the row at the given table and key.
    DeleteRow {
        /// Table holding the row.
        table: Symbol,
        /// Key of the row to delete.
        key: Symbol,
    },
    /// Reserve ids from a named sequence.
    BumpSequence {
        /// Sequence to advance.
        name: Symbol,
        /// Number of ids to reserve.
        reserved: u64,
    },
}

fn validate_unique_row_op(
    touched_rows: &mut BTreeSet<(Symbol, Symbol)>,
    table: &Symbol,
    key: &Symbol,
) -> Result<()> {
    if touched_rows.insert((table.clone(), key.clone())) {
        Ok(())
    } else {
        Err(Error::CatalogConflict {
            table: table.clone(),
            key: key.clone(),
        })
    }
}

fn table_spec<'a>(store: &'a CatalogStore, table: &Symbol) -> Result<&'a CatalogTableSpec> {
    store.table(table).ok_or_else(|| Error::CatalogSchema {
        table: table.clone(),
        message: "unknown catalog table".to_owned(),
    })
}

fn validate_put(store: &CatalogStore, spec: &CatalogTableSpec, row: &CatalogRow) -> Result<()> {
    match spec.policy {
        CatalogWritePolicy::Derived => {
            return Err(Error::CatalogReadOnly {
                table: spec.name.clone(),
            });
        }
        CatalogWritePolicy::Sealed | CatalogWritePolicy::AppendOnly
            if store.row(&row.table, &row.key).is_some() =>
        {
            return Err(Error::CatalogConflict {
                table: row.table.clone(),
                key: row.key.clone(),
            });
        }
        CatalogWritePolicy::Mutable
        | CatalogWritePolicy::Sealed
        | CatalogWritePolicy::AppendOnly => {}
    }

    for field in &spec.required_fields {
        if !row.has_field(field) {
            return Err(Error::CatalogSchema {
                table: row.table.clone(),
                message: format!("missing required catalog field {field}"),
            });
        }
    }
    Ok(())
}

fn validate_delete(spec: &CatalogTableSpec) -> Result<()> {
    match spec.policy {
        CatalogWritePolicy::Mutable => Ok(()),
        CatalogWritePolicy::Sealed
        | CatalogWritePolicy::AppendOnly
        | CatalogWritePolicy::Derived => Err(Error::CatalogReadOnly {
            table: spec.name.clone(),
        }),
    }
}
