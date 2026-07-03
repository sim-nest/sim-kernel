//! The [`DatumStore`] contract: interning and resolving content-addressed data.
//!
//! This is protocol the libraries implement; the kernel ships a simple
//! `BTreeMap`-backed store but defines no persistent storage policy.

use std::collections::BTreeMap;

use crate::{datum::Datum, error::Result, ref_id::ContentId};

/// Contract for interning and resolving content-addressed [`Datum`]s.
///
/// This is protocol the libraries implement; the kernel ships a simple
/// [`BTreeDatumStore`] but defines no persistent storage policy.
pub trait DatumStore {
    /// Interns `datum`, returning its [`ContentId`]; re-interning equal data is
    /// idempotent.
    fn intern(&mut self, datum: Datum) -> Result<ContentId>;
    /// Resolves the datum for `id`, or `None` when it is not stored here.
    fn get(&self, id: &ContentId) -> Result<Option<&Datum>>;
    /// Returns whether `id` is interned in this store.
    fn contains(&self, id: &ContentId) -> bool;
}

/// In-memory [`DatumStore`] backed by a [`BTreeMap`]; the kernel default.
#[derive(Clone, Debug, Default)]
pub struct BTreeDatumStore {
    data: BTreeMap<ContentId, Datum>,
}

impl BTreeDatumStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert_known(&mut self, id: ContentId, datum: Datum) -> Option<Datum> {
        self.data.insert(id, datum)
    }

    /// Returns the number of interned datums.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns whether the store holds no datums.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl DatumStore for BTreeDatumStore {
    fn intern(&mut self, datum: Datum) -> Result<ContentId> {
        let id = datum.content_id()?;
        self.data.entry(id.clone()).or_insert(datum);
        Ok(id)
    }

    fn get(&self, id: &ContentId) -> Result<Option<&Datum>> {
        Ok(self.data.get(id))
    }

    fn contains(&self, id: &ContentId) -> bool {
        self.data.contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Datum, Symbol, datum_content_algorithm};

    use super::*;

    #[test]
    fn datum_store_intern_followed_by_get_returns_original_datum() {
        let mut store = BTreeDatumStore::new();
        let datum = Datum::String("stored".to_owned());

        let id = store.intern(datum.clone()).unwrap();

        assert_eq!(store.get(&id).unwrap(), Some(&datum));
        assert!(store.contains(&id));
    }

    #[test]
    fn datum_store_unresolved_external_content_ids_return_none() {
        let store = BTreeDatumStore::new();
        let id = ContentId::from_bytes(datum_content_algorithm(), [9; 32]);

        assert_eq!(store.get(&id).unwrap(), None);
        assert!(!store.contains(&id));
    }

    #[test]
    fn datum_store_intern_reuses_equal_content_id() {
        let mut store = BTreeDatumStore::new();
        let left = Datum::Symbol(Symbol::new("same"));
        let right = Datum::Symbol(Symbol::new("same"));

        let left_id = store.intern(left).unwrap();
        let right_id = store.intern(right).unwrap();

        assert_eq!(left_id, right_id);
        assert_eq!(store.len(), 1);
    }
}
