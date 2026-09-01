use crate::{
    datum_store::BTreeDatumStore, error::Result, fact_store::BTreeFactStore, library::Registry,
};

use super::{Cx, load_ledger::LibLoadLedger};

pub(crate) struct LibraryState {
    pub(crate) registry: Registry,
    datum_store: BTreeDatumStore,
    facts: BTreeFactStore,
    lib_load_ledger: LibLoadLedger,
}

impl Cx {
    pub(crate) fn library_state(&self) -> LibraryState {
        LibraryState {
            registry: self.registry.clone(),
            datum_store: self.datum_store.clone(),
            facts: self.facts.clone(),
            lib_load_ledger: self.lib_load_ledger.clone(),
        }
    }

    pub(crate) fn commit_library_state(&mut self, state: LibraryState) {
        self.registry = state.registry;
        self.datum_store = state.datum_store;
        self.facts = state.facts;
        self.lib_load_ledger = state.lib_load_ledger;
    }

    pub(crate) fn with_library_state<T>(
        &mut self,
        state: &mut LibraryState,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        std::mem::swap(&mut self.registry, &mut state.registry);
        std::mem::swap(&mut self.datum_store, &mut state.datum_store);
        std::mem::swap(&mut self.facts, &mut state.facts);
        std::mem::swap(&mut self.lib_load_ledger, &mut state.lib_load_ledger);
        let result = f(self);
        std::mem::swap(&mut self.registry, &mut state.registry);
        std::mem::swap(&mut self.datum_store, &mut state.datum_store);
        std::mem::swap(&mut self.facts, &mut state.facts);
        std::mem::swap(&mut self.lib_load_ledger, &mut state.lib_load_ledger);
        result
    }
}
