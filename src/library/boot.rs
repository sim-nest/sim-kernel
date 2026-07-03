use std::path::PathBuf;

use crate::{ExportRecord, LibId, LibManifest, Symbol};

/// A data-only library source suitable for boot receipts.
///
/// This mirrors the replayable variants of
/// [`LibSource`](crate::library::LibSource). Host-constructed library objects
/// are intentionally excluded because they contain live Rust behavior, not
/// stable boot data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibSourceSpec {
    /// A catalog-resolved library symbol.
    Symbol(Symbol),
    /// A filesystem path.
    Path(PathBuf),
    /// A URL.
    Url(String),
    /// In-memory bytes.
    Bytes(Vec<u8>),
}

/// A loaded dependency edge recorded in a boot receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibBootDependency {
    /// Stable id of the loaded dependency in the recorded boot session.
    pub lib_id: LibId,
    /// Manifest symbol of the loaded dependency.
    pub symbol: Symbol,
}

/// The data-only receipt for one loaded library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibBootReceipt {
    /// Stable id assigned to the library during the recorded boot session.
    pub lib_id: LibId,
    /// Source requested by the caller.
    pub requested_source: LibSourceSpec,
    /// Source actually handed to the loader after catalog resolution.
    pub resolved_source: LibSourceSpec,
    /// Manifest loaded from the resolved source.
    pub manifest: LibManifest,
    /// Loaded dependency edges for this library.
    pub dependencies: Vec<LibBootDependency>,
    /// Committed export records produced by the load.
    pub exports: Vec<ExportRecord>,
}

/// A data-only boot state for replaying a loaded registry surface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryBootState {
    /// Receipts in load order.
    pub receipts: Vec<LibBootReceipt>,
}

impl RegistryBootState {
    /// Builds a boot state from load-order receipts.
    pub fn new(receipts: Vec<LibBootReceipt>) -> Self {
        Self { receipts }
    }
}
