//! Reference identity: the [`Ref`] contract and its content/handle ids.
//!
//! Defines the stable reference types -- content ids, handle ids, and
//! coordinates -- that name data and objects across the substrate; libraries
//! resolve refs to values.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use crate::id::Symbol;

/// A stable reference to data or an object across the substrate.
///
/// The kernel defines the reference contract; libraries resolve a [`Ref`] to a
/// concrete value. A ref names its target by symbol, by content id, by a
/// process-local handle, or by a ranked coordinate.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ref {
    /// A named reference resolved through the registry.
    Symbol(Symbol),
    /// A content-addressed reference to an immutable datum.
    Content(ContentId),
    /// A process-local handle to a live object.
    Handle(HandleId),
    /// A ranked coordinate within a named space.
    Coord(Coordinate),
}

/// Content-addressed identity: a hash algorithm plus its 32-byte digest.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentId {
    /// The hash algorithm that produced the digest (for example `core/sha256`).
    pub algorithm: Symbol,
    /// The 32-byte content digest.
    pub bytes: [u8; 32],
}

impl ContentId {
    /// Builds a content id from an algorithm symbol and a 32-byte digest.
    pub fn from_bytes(algorithm: Symbol, bytes: [u8; 32]) -> Self {
        Self { algorithm, bytes }
    }
}

/// Process-local handle to a live object, unique within this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandleId(pub u128);

impl HandleId {
    /// Allocates a fresh handle, distinct from every other in this process.
    pub fn fresh() -> Self {
        let salt = u128::from(*PROCESS_SALT.get_or_init(make_process_salt));
        let counter = u128::from(NEXT_HANDLE.fetch_add(1, Ordering::Relaxed));
        Self((salt << 64) | counter)
    }
}

/// A ranked coordinate: an ordinal position within a named space.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Coordinate {
    /// The named coordinate space.
    pub space: Symbol,
    /// The position within the space, keyed by content id.
    pub ordinal: ContentId,
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static PROCESS_SALT: OnceLock<u64> = OnceLock::new();

fn make_process_salt() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let folded = (nanos ^ (nanos >> 64)) & u128::from(u64::MAX);
    let time_salt = u64::try_from(folded).unwrap_or(0);
    time_salt ^ (u64::from(std::process::id()) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_id(byte: u8) -> ContentId {
        ContentId::from_bytes(Symbol::qualified("core", "sha256"), [byte; 32])
    }

    #[test]
    fn ref_id_equal_symbols_compare_equal() {
        let left = Ref::Symbol(Symbol::qualified("core", "Bool"));
        let right = Ref::Symbol(Symbol::qualified("core", "Bool"));

        assert_eq!(left, right);
    }

    #[test]
    fn ref_id_equal_content_ids_compare_equal() {
        let left = content_id(1);
        let right = content_id(1);

        assert_eq!(left, right);
    }

    #[test]
    fn ref_id_equal_coordinates_compare_equal() {
        let left = Coordinate {
            space: Symbol::qualified("rank", "natural"),
            ordinal: content_id(2),
        };
        let right = Coordinate {
            space: Symbol::qualified("rank", "natural"),
            ordinal: content_id(2),
        };

        assert_eq!(left, right);
    }

    #[test]
    fn ref_id_handle_never_equals_content_id() {
        let handle = Ref::Handle(HandleId::fresh());
        let content = Ref::Content(content_id(3));

        assert_ne!(handle, content);
    }

    #[test]
    fn ref_id_fresh_handles_are_distinct() {
        assert_ne!(HandleId::fresh(), HandleId::fresh());
    }
}
