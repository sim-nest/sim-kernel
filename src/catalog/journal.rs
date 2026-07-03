use crate::Symbol;

/// One append-only audit entry recording a committed catalog operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEvent {
    /// Catalog epoch at which the operation committed.
    pub epoch: u64,
    /// The committed operation.
    pub op: CatalogEventOp,
}

/// The committed operation recorded by a [`CatalogEvent`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogEventOp {
    /// A row was inserted or replaced.
    PutRow {
        /// Table holding the row.
        table: Symbol,
        /// Key of the affected row.
        key: Symbol,
    },
    /// A row was deleted.
    DeleteRow {
        /// Table that held the row.
        table: Symbol,
        /// Key of the deleted row.
        key: Symbol,
    },
    /// A sequence advanced to a new value.
    Sequence {
        /// Sequence that advanced.
        name: Symbol,
        /// Next value of the sequence after the bump.
        next: u64,
    },
}
