use std::collections::BTreeMap;

use crate::{ContentId, fact_store::BTreeFactStore, id::LibId};

#[derive(Default)]
pub(super) struct LibLoadLedger {
    claims_by_lib: BTreeMap<LibId, Vec<ContentId>>,
}

impl LibLoadLedger {
    pub(super) fn record_claims(&mut self, lib_id: LibId, claim_ids: Vec<ContentId>) {
        if claim_ids.is_empty() {
            return;
        }
        self.claims_by_lib
            .entry(lib_id)
            .or_default()
            .extend(claim_ids);
    }

    pub(super) fn remove_claims(&mut self, lib_ids: &[LibId], facts: &mut BTreeFactStore) {
        for lib_id in lib_ids {
            if let Some(claim_ids) = self.claims_by_lib.remove(lib_id) {
                for claim_id in claim_ids {
                    facts.remove(&claim_id);
                }
            }
        }
    }
}
