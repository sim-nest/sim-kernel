use std::collections::BTreeMap;

use crate::{Error, Result, Symbol};

use super::{CatalogSnapshot, CatalogSnapshotRow, CatalogStore, CatalogTableSpec};

/// A replayable log of catalog events plus the table specs and epoch needed to
/// rebuild a [`CatalogStore`] deterministically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogReplay {
    /// Table specs the replay installs.
    pub tables: BTreeMap<Symbol, CatalogTableSpec>,
    /// Ordered events to replay.
    pub events: Vec<CatalogReplayEvent>,
    /// Final catalog epoch after replay.
    pub epoch: u64,
}

/// A single event within a [`CatalogReplay`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogReplayEvent {
    /// Insert or replace a row.
    PutRow(CatalogSnapshotRow),
    /// Delete a row.
    DeleteRow {
        /// Epoch of the deletion.
        epoch: u64,
        /// Table the row was in.
        table: Symbol,
        /// Key of the deleted row.
        key: Symbol,
    },
    /// Set a sequence value.
    Sequence {
        /// Epoch of the change.
        epoch: u64,
        /// Sequence name.
        name: Symbol,
        /// New sequence value.
        next: u64,
    },
}

impl CatalogReplay {
    /// Builds a replay log from `snapshot`'s rows and sequences.
    pub fn from_snapshot(snapshot: &CatalogSnapshot) -> Self {
        let row_events = snapshot
            .rows
            .values()
            .flat_map(BTreeMap::values)
            .cloned()
            .map(CatalogReplayEvent::PutRow);
        let sequence_events =
            snapshot
                .sequences
                .iter()
                .map(|(name, next)| CatalogReplayEvent::Sequence {
                    epoch: snapshot.epoch,
                    name: name.clone(),
                    next: *next,
                });

        Self {
            tables: snapshot.tables.clone(),
            events: row_events.chain(sequence_events).collect(),
            epoch: snapshot.epoch,
        }
    }
}

impl CatalogStore {
    /// Rebuilds a store by replaying `replay`'s events in order, validating each
    /// event epoch against the replay epoch.
    pub fn replay(replay: CatalogReplay) -> Result<Self> {
        let mut snapshot = CatalogSnapshot {
            tables: replay.tables,
            rows: BTreeMap::new(),
            sequences: BTreeMap::new(),
            epoch: replay.epoch,
        };

        for event in replay.events {
            match event {
                CatalogReplayEvent::PutRow(row) => {
                    validate_event_epoch(row.epoch, snapshot.epoch, &row.table)?;
                    snapshot
                        .rows
                        .entry(row.table.clone())
                        .or_default()
                        .insert(row.key.clone(), row);
                }
                CatalogReplayEvent::DeleteRow { epoch, table, key } => {
                    validate_event_epoch(epoch, snapshot.epoch, &table)?;
                    if let Some(rows) = snapshot.rows.get_mut(&table) {
                        rows.remove(&key);
                        if rows.is_empty() {
                            snapshot.rows.remove(&table);
                        }
                    }
                }
                CatalogReplayEvent::Sequence { epoch, name, next } => {
                    validate_event_epoch(epoch, snapshot.epoch, &name)?;
                    snapshot.sequences.insert(name, next);
                }
            }
        }

        Self::from_snapshot(snapshot)
    }
}

fn validate_event_epoch(epoch: u64, replay_epoch: u64, table: &Symbol) -> Result<()> {
    if epoch <= replay_epoch {
        Ok(())
    } else {
        Err(Error::CatalogSchema {
            table: table.clone(),
            message: "replay event epoch is newer than replay epoch".to_owned(),
        })
    }
}
