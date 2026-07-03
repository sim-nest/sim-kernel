use std::collections::BTreeMap;

use crate::{Error, Expr, Result, Symbol};

use super::{
    CatalogRow, CatalogStore, CatalogTableSpec,
    snapshot_expr::{snapshot_from_expr, snapshot_to_expr, unresolved_live_expr},
};

/// A deterministic, point-in-time copy of a [`CatalogStore`]'s data.
///
/// Snapshots carry table specs, row data, sequences, and the catalog epoch. Live
/// host payloads (runtime values, tests) are not serialized; their fields become
/// `catalog/unresolved-live` markers. See the README section "Snapshots and
/// deltas".
///
/// # Examples
///
/// ```
/// # use sim_kernel::catalog::{CatalogSnapshot, CatalogStore};
/// let store = CatalogStore::new();
/// let snapshot = CatalogSnapshot::from_store(&store);
/// // Snapshots round-trip through a deterministic Expr form.
/// let restored = CatalogSnapshot::from_expr(snapshot.to_expr()).unwrap();
/// assert_eq!(restored, snapshot);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSnapshot {
    /// Installed table specs by name.
    pub tables: BTreeMap<Symbol, CatalogTableSpec>,
    /// Snapshot rows by table and key.
    pub rows: BTreeMap<Symbol, BTreeMap<Symbol, CatalogSnapshotRow>>,
    /// Sequence values by name.
    pub sequences: BTreeMap<Symbol, u64>,
    /// Catalog epoch the snapshot was taken at.
    pub epoch: u64,
}

impl CatalogSnapshot {
    /// Captures the visible data of `store`, replacing live payloads with
    /// unresolved-live markers.
    pub fn from_store(store: &CatalogStore) -> Self {
        let rows = visible_rows(store)
            .iter()
            .map(|(table, rows)| {
                let rows = rows
                    .iter()
                    .map(|(key, row)| {
                        (
                            key.clone(),
                            CatalogSnapshotRow {
                                table: row.table.clone(),
                                key: row.key.clone(),
                                epoch: row.epoch,
                                data: snapshot_row_data(row),
                            },
                        )
                    })
                    .collect();
                (table.clone(), rows)
            })
            .collect();

        Self {
            tables: store.tables.clone(),
            rows,
            sequences: visible_sequences(store).clone(),
            epoch: store.epoch(),
        }
    }

    /// Emits the deterministic [`Expr`] encoding of the snapshot.
    pub fn to_expr(&self) -> Expr {
        snapshot_to_expr(self)
    }

    /// Parses a snapshot from the [`Expr`] shape produced by [`Self::to_expr`].
    pub fn from_expr(expr: Expr) -> Result<Self> {
        snapshot_from_expr(expr)
    }

    /// Returns the snapshot rows of `table` by key, if any.
    pub fn rows(&self, table: &Symbol) -> Option<&BTreeMap<Symbol, CatalogSnapshotRow>> {
        self.rows.get(table)
    }
}

/// One row within a [`CatalogSnapshot`]: data-only, with live payloads already
/// projected to markers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSnapshotRow {
    /// Table the row belongs to.
    pub table: Symbol,
    /// Key identifying the row.
    pub key: Symbol,
    /// Epoch at which the row was last written.
    pub epoch: u64,
    /// Serializable field data.
    pub data: BTreeMap<Symbol, Expr>,
}

impl CatalogStore {
    /// Restores a data-only store from `snapshot`, validating table keys, row
    /// keys, required fields, and row epochs.
    pub fn from_snapshot(snapshot: CatalogSnapshot) -> Result<Self> {
        validate_table_keys(&snapshot.tables)?;
        let mut store = CatalogStore {
            tables: snapshot.tables,
            sequences: snapshot.sequences,
            epoch: snapshot.epoch,
            ..Self::default()
        };

        for (table, rows) in snapshot.rows {
            let spec = store
                .table(&table)
                .cloned()
                .ok_or_else(|| Error::CatalogSchema {
                    table: table.clone(),
                    message: "unknown catalog table".to_owned(),
                })?;
            for (key, snapshot_row) in rows {
                validate_snapshot_row(&table, &key, &snapshot_row, &spec, store.epoch)?;
                let mut row = CatalogRow::new(snapshot_row.table, snapshot_row.key);
                row.data = snapshot_row.data;
                row.set_epoch(snapshot_row.epoch);
                store
                    .rows
                    .entry(table.clone())
                    .or_default()
                    .insert(key, row);
            }
        }

        Ok(store)
    }
}

fn visible_rows(store: &CatalogStore) -> &BTreeMap<Symbol, BTreeMap<Symbol, CatalogRow>> {
    store
        .overlay
        .as_ref()
        .map_or(&store.rows, |overlay| overlay.all_rows())
}

fn visible_sequences(store: &CatalogStore) -> &BTreeMap<Symbol, u64> {
    store
        .overlay
        .as_ref()
        .map_or(&store.sequences, |overlay| overlay.all_sequences())
}

fn snapshot_row_data(row: &CatalogRow) -> BTreeMap<Symbol, Expr> {
    let mut data = row.data.clone();
    for field in row.live.keys() {
        data.entry(field.clone())
            .or_insert_with(|| unresolved_live_expr(row, field));
    }
    data
}

fn validate_table_keys(tables: &BTreeMap<Symbol, CatalogTableSpec>) -> Result<()> {
    for (name, spec) in tables {
        if name != &spec.name {
            return Err(Error::CatalogSchema {
                table: name.clone(),
                message: "table spec key does not match table name".to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_snapshot_row(
    table: &Symbol,
    key: &Symbol,
    row: &CatalogSnapshotRow,
    spec: &CatalogTableSpec,
    snapshot_epoch: u64,
) -> Result<()> {
    if &row.table != table || &row.key != key {
        return Err(Error::CatalogSchema {
            table: table.clone(),
            message: "snapshot row key does not match row data".to_owned(),
        });
    }
    if row.epoch > snapshot_epoch {
        return Err(Error::CatalogSchema {
            table: table.clone(),
            message: "row epoch is newer than snapshot epoch".to_owned(),
        });
    }
    for field in &spec.required_fields {
        if !row.data.contains_key(field) {
            return Err(Error::CatalogSchema {
                table: table.clone(),
                message: format!("missing required catalog field {field}"),
            });
        }
    }
    Ok(())
}
