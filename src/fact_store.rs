//! The [`FactStore`] contract: storing and querying claims under visibility.
//!
//! This is protocol the libraries implement; the kernel ships a `BTreeMap`-
//! backed store and defines the capability-gated read rules, not the
//! persistence strategy.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use crate::{
    capability::{CapabilityName, CapabilitySet, fact_private_capability},
    claim::{Claim, ClaimKind, ClaimPattern, Visibility},
    datum::Datum,
    datum_store::{BTreeDatumStore, DatumStore},
    env::Cx,
    error::{Error, Result},
    id::Symbol,
    ref_id::{ContentId, Ref},
};

/// Contract for storing and querying [`Claim`]s under visibility rules.
///
/// This is protocol the libraries implement; the kernel ships a [`BTreeMap`]-
/// backed [`BTreeFactStore`] and defines the capability-gated read rules, not
/// the persistence strategy.
pub trait FactStore {
    /// Inserts `claim` after checking the caller's `capabilities`, returning its
    /// content [`Ref`]; private claims require the fact-private capability.
    fn insert_authorized(
        &mut self,
        capabilities: &CapabilitySet,
        data: &mut dyn DatumStore,
        claim: Claim,
    ) -> Result<Ref>;

    /// Returns the claims matching `pattern` that are visible under `cx`'s
    /// capabilities.
    fn query_authorized(&self, cx: &Cx, pattern: ClaimPattern) -> Result<Vec<Claim>>;
}

/// In-memory [`FactStore`] backed by [`BTreeMap`]s with a triple index; the
/// kernel default.
#[derive(Clone, Debug, Default)]
pub struct BTreeFactStore {
    claims: BTreeMap<ContentId, Claim>,
    index: BTreeMap<ClaimIndexKey, BTreeSet<ContentId>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ClaimIndexKey {
    subject: Option<Ref>,
    predicate: Option<Symbol>,
    object: Option<Ref>,
}

impl BTreeFactStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of stored claims.
    pub fn len(&self) -> usize {
        self.claims.len()
    }

    /// Returns whether the store holds no claims.
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    /// Returns the stored claim with content id `id`, if present.
    pub fn get(&self, id: &ContentId) -> Option<&Claim> {
        self.claims.get(id)
    }

    /// Removes the claim with content id `id`, updating every query index.
    pub fn remove(&mut self, id: &ContentId) -> Option<Claim> {
        let claim = self.claims.remove(id)?;
        for key in index_keys_for_claim(&claim) {
            let should_remove_key = if let Some(ids) = self.index.get_mut(&key) {
                ids.remove(id);
                ids.is_empty()
            } else {
                false
            };
            if should_remove_key {
                self.index.remove(&key);
            }
        }
        Some(claim)
    }

    /// Seeds the fixed core boot claims (the kernel class `kind` facts) into
    /// this store and their datums into `data`.
    pub fn insert_boot_claims(&mut self, data: &mut BTreeDatumStore) {
        for record in boot_claim_records() {
            data.insert_known(record.id.clone(), record.datum.clone());
            self.insert_indexed(record.claim.clone());
        }
    }

    fn insert_indexed(&mut self, claim: Claim) {
        let Some(id) = claim.id.clone() else {
            return;
        };

        if !self.claims.contains_key(&id) {
            for key in index_keys_for_claim(&claim) {
                self.index.entry(key).or_default().insert(id.clone());
            }
            self.claims.insert(id, claim);
        }
    }
}

impl FactStore for BTreeFactStore {
    fn insert_authorized(
        &mut self,
        capabilities: &CapabilitySet,
        data: &mut dyn DatumStore,
        mut claim: Claim,
    ) -> Result<Ref> {
        authorize_insert(capabilities, &claim)?;

        let id = claim.content_id(data)?;
        claim.id = Some(id.clone());

        self.insert_indexed(claim);

        Ok(Ref::Content(id))
    }

    fn query_authorized(&self, cx: &Cx, pattern: ClaimPattern) -> Result<Vec<Claim>> {
        let key = ClaimIndexKey {
            subject: pattern.subject,
            predicate: pattern.predicate,
            object: pattern.object,
        };

        let Some(ids) = self.index.get(&key) else {
            return Ok(Vec::new());
        };

        Ok(ids
            .iter()
            .filter_map(|id| self.claims.get(id))
            .filter(|claim| visible_to(cx, claim, pattern.include_revoked))
            .cloned()
            .collect())
    }
}

fn authorize_insert(capabilities: &CapabilitySet, claim: &Claim) -> Result<()> {
    if claim.visibility == Visibility::Private {
        require_capability(capabilities, &fact_private_capability())?;
    }
    Ok(())
}

fn visible_to(cx: &Cx, claim: &Claim, include_revoked: bool) -> bool {
    if claim.kind == ClaimKind::Revoked && !include_revoked {
        return false;
    }

    match claim.visibility {
        Visibility::Public => true,
        Visibility::CapabilityGated => has_all_requirements(cx.capabilities(), claim),
        Visibility::Private => {
            cx.capabilities().contains(&fact_private_capability())
                && has_all_requirements(cx.capabilities(), claim)
        }
    }
}

fn has_all_requirements(capabilities: &CapabilitySet, claim: &Claim) -> bool {
    claim
        .requires
        .iter()
        .all(|capability| capabilities.contains(capability))
}

fn require_capability(capabilities: &CapabilitySet, capability: &CapabilityName) -> Result<()> {
    if capabilities.contains(capability) {
        Ok(())
    } else {
        Err(Error::CapabilityDenied {
            capability: capability.clone(),
        })
    }
}

fn index_keys_for_claim(claim: &Claim) -> Vec<ClaimIndexKey> {
    let subjects = [None, Some(claim.subject.clone())];
    let predicates = [None, Some(claim.predicate.clone())];
    let objects = [None, Some(claim.object.clone())];
    let mut keys = Vec::with_capacity(8);

    for subject in subjects {
        for predicate in predicates.clone() {
            for object in objects.clone() {
                keys.push(ClaimIndexKey {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    object,
                });
            }
        }
    }

    keys
}

fn core_boot_claims() -> Vec<Claim> {
    core_class_names()
        .iter()
        .map(|name| {
            Claim::public(
                Ref::Symbol(core_symbol(name)),
                core_symbol("kind"),
                Ref::Symbol(core_symbol("class")),
            )
        })
        .collect()
}

#[derive(Clone)]
struct BootClaimRecord {
    id: ContentId,
    datum: Datum,
    claim: Claim,
}

fn boot_claim_records() -> &'static [BootClaimRecord] {
    static RECORDS: OnceLock<Vec<BootClaimRecord>> = OnceLock::new();
    RECORDS.get_or_init(|| {
        core_boot_claims()
            .into_iter()
            .map(|mut claim| {
                let datum = claim.canonical_datum();
                let id = datum.content_id().expect("core boot claim datum is valid");
                claim.id = Some(id.clone());
                BootClaimRecord { id, datum, claim }
            })
            .collect()
    })
}

fn core_class_names() -> &'static [&'static str] {
    &[
        "Class",
        "Nil",
        "Bool",
        "Number",
        "Symbol",
        "String",
        "Bytes",
        "List",
        "Table",
        "Expr",
        "Function",
        "Shape",
        "Thunk",
        "EvalRequest",
        "EvalReply",
        "Macro",
        "ShapeMatch",
        "Codec",
        "Help",
        "Test",
        "NumberDomain",
        "LocalEvalFabric",
        "Card",
    ]
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}
