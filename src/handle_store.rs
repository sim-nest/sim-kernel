//! The [`HandleStore`] contract: interning values behind opaque handle ids.
//!
//! This is protocol the libraries implement; the kernel ships a default store
//! and defines the handle-id identity, not the lifetime policy.

use std::collections::{BTreeMap, HashMap};

use crate::{
    ref_id::{HandleId, HandleSeed, HandleSequence},
    value::Value,
};

/// Contract for interning runtime [`Value`]s behind opaque [`HandleId`]s.
///
/// This is protocol the libraries implement; the kernel ships a default
/// [`BTreeHandleStore`] and defines the handle-id identity, not the lifetime
/// policy.
pub trait HandleStore {
    /// Interns `value`, returning its handle; equal values reuse one handle.
    fn intern(&mut self, value: Value) -> HandleId;
    /// Resolves the value for `id`, or `None` when it is not stored here.
    fn get(&self, id: &HandleId) -> Option<&Value>;
    /// Returns whether `id` is interned in this store.
    fn contains(&self, id: &HandleId) -> bool;
    /// Returns the existing handle for `value`, if one is interned.
    fn handle_for_value(&self, value: &Value) -> Option<HandleId>;
}

/// In-memory [`HandleStore`] keyed by [`HandleId`] with a reverse value index;
/// the kernel default.
#[derive(Clone, Debug)]
pub struct BTreeHandleStore {
    sequence: HandleSequence,
    values: BTreeMap<HandleId, Value>,
    handles_by_value: HashMap<Value, HandleId>,
}

impl BTreeHandleStore {
    /// Creates an empty store.
    pub fn new(seed: HandleSeed) -> Self {
        Self {
            sequence: seed.sequence(),
            values: BTreeMap::new(),
            handles_by_value: HashMap::new(),
        }
    }

    /// Returns the number of interned values.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the store holds no values.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Allocates an identity without interning a value.
    pub fn fresh_handle(&mut self) -> HandleId {
        self.sequence.next_handle()
    }
}

impl HandleStore for BTreeHandleStore {
    fn intern(&mut self, value: Value) -> HandleId {
        if let Some(handle) = self.handles_by_value.get(&value) {
            return *handle;
        }

        let handle = self.fresh_handle();
        self.handles_by_value.insert(value.clone(), handle);
        self.values.insert(handle, value);
        handle
    }

    fn get(&self, id: &HandleId) -> Option<&Value> {
        self.values.get(id)
    }

    fn contains(&self, id: &HandleId) -> bool {
        self.values.contains_key(id)
    }

    fn handle_for_value(&self, value: &Value) -> Option<HandleId> {
        self.handles_by_value.get(value).copied()
    }
}

#[cfg(test)]
mod tests {
    use crate::{DefaultFactory, Factory};

    use super::*;

    fn factory() -> DefaultFactory {
        DefaultFactory
    }

    #[test]
    fn handle_store_intern_followed_by_get_returns_original_value() {
        let mut store = BTreeHandleStore::new(HandleSeed::new(7));
        let value = factory().string("stored".to_owned()).unwrap();

        let handle = store.intern(value.clone());

        assert_eq!(store.get(&handle), Some(&value));
        assert!(store.contains(&handle));
    }

    #[test]
    fn handle_store_reuses_handle_for_same_value() {
        let mut store = BTreeHandleStore::new(HandleSeed::new(7));
        let value = factory().bool(true).unwrap();

        let first = store.intern(value.clone());
        let second = store.intern(value);

        assert_eq!(first, second);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn handle_store_distinguishes_distinct_values() {
        let mut store = BTreeHandleStore::new(HandleSeed::new(7));
        let first_value = factory().bool(true).unwrap();
        let second_value = factory().bool(true).unwrap();

        let first = store.intern(first_value);
        let second = store.intern(second_value);

        assert_ne!(first, second);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn handle_store_reuses_handle_for_cloned_value() {
        let mut store = BTreeHandleStore::new(HandleSeed::new(7));
        let left = factory().string("same".to_owned()).unwrap();
        let right = left.clone();

        assert_eq!(store.intern(left), store.intern(right));
    }
}
