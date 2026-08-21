//! Reference identity: the [`Ref`] contract and its content/handle ids.
//!
//! Defines the stable reference types -- content ids, handle ids, and
//! coordinates -- that name data and objects across the substrate; libraries
//! resolve refs to values.

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
    /// Builds a handle from a supplied seed and sequence number.
    pub const fn from_seed_and_sequence(seed: HandleSeed, sequence: u64) -> Self {
        Self(((seed.0 as u128) << 64) | sequence as u128)
    }
}

/// Caller-supplied namespace for a deterministic sequence of live handles.
///
/// A runtime boundary obtains this plain value from its bootstrap input. The
/// kernel never manufactures one from ambient process state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandleSeed(pub u64);

impl HandleSeed {
    /// Creates a handle seed from caller-owned data.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Starts a deterministic handle sequence in this seed's namespace.
    pub const fn sequence(self) -> HandleSequence {
        HandleSequence {
            seed: self,
            next: 1,
        }
    }
}

/// Deterministic allocator for handles in one caller-supplied namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleSequence {
    seed: HandleSeed,
    next: u64,
}

impl HandleSequence {
    /// Allocates the next handle in the sequence.
    ///
    /// # Panics
    ///
    /// Panics after exhausting all nonzero `u64` sequence numbers instead of
    /// wrapping into an existing handle.
    pub fn next_handle(&mut self) -> HandleId {
        let sequence = self.next;
        self.next = self.next.checked_add(1).expect("handle sequence exhausted");
        HandleId::from_seed_and_sequence(self.seed, sequence)
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
        let handle = Ref::Handle(HandleSeed::new(7).sequence().next_handle());
        let content = Ref::Content(content_id(3));

        assert_ne!(handle, content);
    }

    #[test]
    fn equal_seeds_produce_identical_handle_sequences() {
        let mut left = HandleSeed::new(7).sequence();
        let mut right = HandleSeed::new(7).sequence();

        assert_eq!(left.next_handle(), right.next_handle());
        assert_eq!(left.next_handle(), right.next_handle());
    }

    #[test]
    fn distinct_seeds_separate_handle_sequences() {
        let mut left = HandleSeed::new(7).sequence();
        let mut right = HandleSeed::new(8).sequence();

        assert_ne!(left.next_handle(), right.next_handle());
        assert_ne!(left.next_handle(), right.next_handle());
    }
}
