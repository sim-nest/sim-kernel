//! The registry catalog substrate: transactional, `Cx`-free table storage.
//!
//! The catalog is the authoritative storage behind the
//! [`Registry`](crate::Registry); see the README section "Contract: registry
//! catalog substrate". The kernel defines the catalog contracts -- schema, rows,
//! transactions, the append-only journal, snapshots, deltas, overlays,
//! read-only views, and replay -- and ships a default [`store::CatalogStore`];
//! libraries supply higher-level behavior over them.

/// Snapshot-to-snapshot change sets and their application.
pub mod delta;
/// Append-only audit journal of committed catalog operations.
pub mod journal;
/// Composite `table/key` string keys used by catalog views and overlays.
pub mod key;
/// Buffered, uncommitted catalog edits layered over a base store.
pub mod overlay;
/// Deterministic replay of journaled catalog events.
pub mod replay;
/// Catalog row data: deterministic [`Expr`](crate::Expr) plus live payloads.
pub mod row;
/// Table specifications and write policies.
pub mod schema;
/// Deterministic point-in-time snapshots of catalog data.
pub mod snapshot;
mod snapshot_expr;
/// The [`store::CatalogStore`]: lock-free transactional table storage.
pub mod store;
/// Mutable catalog-backed table object for ordinary runtime use.
pub mod table_backend;
/// Atomic catalog transactions and their constituent operations.
pub mod tx;
/// Read-only catalog directory and table views.
pub mod view;

pub use delta::{CatalogDeletedRow, CatalogDelta, CatalogSequenceChange};
pub use journal::{CatalogEvent, CatalogEventOp};
pub use key::{catalog_key, split_catalog_key};
pub use overlay::CatalogOverlay;
pub use replay::{CatalogReplay, CatalogReplayEvent};
pub use row::CatalogRow;
pub use schema::{CatalogTableSpec, CatalogWritePolicy};
pub use snapshot::{CatalogSnapshot, CatalogSnapshotRow};
pub use store::CatalogStore;
pub use table_backend::{CatalogBackend, CatalogTable};
pub use tx::{CatalogOp, CatalogTx};
pub use view::{CatalogDirView, CatalogTableView, registry_catalog_view};

#[cfg(test)]
mod delta_tests;
#[cfg(test)]
mod overlay_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod view_tests;
