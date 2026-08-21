//! Standard-distribution metadata: the profile and organ claim vocabulary.
//!
//! The kernel defines the predicate keys and operation keys by which the
//! standard distribution describes itself; the standard distribution itself is
//! a lib loaded by default, not kernel behavior.

use crate::{
    card::{card_kind_predicate, card_ops_predicate, card_tests_predicate},
    claim::{Claim, ClaimKind, ClaimPattern},
    env::Cx,
    error::Result,
    id::{LibId, Symbol},
    ref_id::Ref,
    term::OpKey,
};

/// The card-kind symbol for a standard-distribution profile.
pub fn standard_profile_kind() -> Symbol {
    standard_symbol("profile")
}

/// The card-kind symbol for a standard-distribution organ.
pub fn standard_organ_kind() -> Symbol {
    standard_symbol("organ")
}

/// The claim predicate naming a profile.
pub fn standard_profile_predicate() -> Symbol {
    standard_symbol("profile")
}

/// The claim predicate relating a profile to one of its organs.
pub fn standard_organ_predicate() -> Symbol {
    standard_symbol("organ")
}

/// The claim predicate carrying a fidelity badge.
pub fn standard_fidelity_predicate() -> Symbol {
    standard_symbol("fidelity")
}

/// The claim predicate carrying fidelity evidence.
pub fn standard_evidence_predicate() -> Symbol {
    standard_symbol("evidence")
}

/// Builds a `control`-namespace operation key for the given name at version 1.
pub fn standard_control_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("control"), Symbol::new(name), 1)
}

/// Publishes the claims describing a profile: its kind, organs, and tests.
///
/// Each claim is inserted once; re-running is idempotent.
pub fn publish_profile_claims(
    cx: &mut Cx,
    profile: Symbol,
    organs: impl IntoIterator<Item = Symbol>,
    conformance_tests: impl IntoIterator<Item = Symbol>,
) -> Result<()> {
    publish_profile_claims_with_owner(cx, None, profile, organs, conformance_tests)
}

/// Publishes profile claims and records newly inserted facts under `lib_id`.
pub fn publish_profile_claims_for_lib(
    cx: &mut Cx,
    lib_id: LibId,
    profile: Symbol,
    organs: impl IntoIterator<Item = Symbol>,
    conformance_tests: impl IntoIterator<Item = Symbol>,
) -> Result<()> {
    publish_profile_claims_with_owner(cx, Some(lib_id), profile, organs, conformance_tests)
}

fn publish_profile_claims_with_owner(
    cx: &mut Cx,
    owner: Option<LibId>,
    profile: Symbol,
    organs: impl IntoIterator<Item = Symbol>,
    conformance_tests: impl IntoIterator<Item = Symbol>,
) -> Result<()> {
    let subject = Ref::Symbol(profile.clone());
    insert_once(
        cx,
        owner,
        subject.clone(),
        card_kind_predicate(),
        Ref::Symbol(standard_profile_kind()),
    )?;
    for organ in organs {
        insert_once(
            cx,
            owner,
            subject.clone(),
            standard_organ_predicate(),
            Ref::Symbol(organ),
        )?;
    }
    for test in conformance_tests {
        insert_once(
            cx,
            owner,
            subject.clone(),
            card_tests_predicate(),
            Ref::Symbol(test),
        )?;
    }
    insert_once(
        cx,
        owner,
        subject,
        standard_profile_predicate(),
        Ref::Symbol(profile),
    )
}

/// Publishes the claims describing an organ: its kind and the ops it provides.
///
/// Each claim is inserted once; re-running is idempotent.
pub fn publish_organ_claims(
    cx: &mut Cx,
    organ: Symbol,
    ops: impl IntoIterator<Item = OpKey>,
) -> Result<()> {
    publish_organ_claims_with_owner(cx, None, organ, ops)
}

/// Publishes organ claims and records newly inserted facts under `lib_id`.
pub fn publish_organ_claims_for_lib(
    cx: &mut Cx,
    lib_id: LibId,
    organ: Symbol,
    ops: impl IntoIterator<Item = OpKey>,
) -> Result<()> {
    publish_organ_claims_with_owner(cx, Some(lib_id), organ, ops)
}

fn publish_organ_claims_with_owner(
    cx: &mut Cx,
    owner: Option<LibId>,
    organ: Symbol,
    ops: impl IntoIterator<Item = OpKey>,
) -> Result<()> {
    let subject = Ref::Symbol(organ);
    insert_once(
        cx,
        owner,
        subject.clone(),
        card_kind_predicate(),
        Ref::Symbol(standard_organ_kind()),
    )?;
    for op in ops {
        insert_once(
            cx,
            owner,
            subject.clone(),
            card_ops_predicate(),
            Ref::Symbol(op_symbol(&op)),
        )?;
    }
    Ok(())
}

/// Publishes a fidelity badge for a subject together with its evidence.
///
/// The badge and the observed evidence claim are each inserted once.
pub fn publish_fidelity_badge(
    cx: &mut Cx,
    subject: Ref,
    badge: Symbol,
    evidence: Ref,
) -> Result<()> {
    publish_fidelity_badge_with_owner(cx, None, subject, badge, evidence)
}

/// Publishes a fidelity badge and records newly inserted facts under `lib_id`.
pub fn publish_fidelity_badge_for_lib(
    cx: &mut Cx,
    lib_id: LibId,
    subject: Ref,
    badge: Symbol,
    evidence: Ref,
) -> Result<()> {
    publish_fidelity_badge_with_owner(cx, Some(lib_id), subject, badge, evidence)
}

fn publish_fidelity_badge_with_owner(
    cx: &mut Cx,
    owner: Option<LibId>,
    subject: Ref,
    badge: Symbol,
    evidence: Ref,
) -> Result<()> {
    insert_once(
        cx,
        owner,
        subject.clone(),
        standard_fidelity_predicate(),
        Ref::Symbol(badge),
    )?;
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            standard_evidence_predicate(),
            evidence.clone(),
        ))?
        .is_empty();
    if !exists {
        insert_claim(
            cx,
            owner,
            Claim::public(subject, standard_evidence_predicate(), evidence)
                .with_kind(ClaimKind::Observed),
        )?;
    }
    Ok(())
}

fn insert_once(
    cx: &mut Cx,
    owner: Option<LibId>,
    subject: Ref,
    predicate: Symbol,
    object: Ref,
) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        insert_claim(cx, owner, Claim::public(subject, predicate, object))?;
    }
    Ok(())
}

fn insert_claim(cx: &mut Cx, owner: Option<LibId>, claim: Claim) -> Result<()> {
    match owner {
        Some(lib_id) => {
            cx.insert_fact_for_lib(lib_id, claim)?;
        }
        None => {
            cx.insert_fact(claim)?;
        }
    }
    Ok(())
}

fn op_symbol(op: &OpKey) -> Symbol {
    Symbol::qualified(
        op.namespace.to_string(),
        format!("{}.v{}", op.name, op.version),
    )
}

fn standard_symbol(name: &str) -> Symbol {
    Symbol::qualified("standard", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DefaultFactory, Expr, NoopEvalPolicy, card::card_for_ref};
    use std::sync::Arc;

    #[test]
    fn profile_organs_tests_and_fidelity_are_claims() {
        let mut cx = Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let profile = Symbol::qualified("standard", "rust-clean");
        let organ = Symbol::qualified("standard", "reader");
        let test = Symbol::qualified("test", "reader-roundtrip");

        publish_profile_claims(&mut cx, profile.clone(), [organ.clone()], [test.clone()]).unwrap();
        publish_organ_claims(&mut cx, organ.clone(), [standard_control_op_key("prompt")]).unwrap();
        publish_fidelity_badge(
            &mut cx,
            Ref::Symbol(profile.clone()),
            Symbol::qualified("standard", "full"),
            Ref::Symbol(test.clone()),
        )
        .unwrap();

        assert_has_claim(
            &cx,
            Ref::Symbol(profile.clone()),
            standard_organ_predicate(),
            Ref::Symbol(organ),
        );
        assert_has_claim(
            &cx,
            Ref::Symbol(profile),
            card_tests_predicate(),
            Ref::Symbol(test),
        );
    }

    #[test]
    fn profile_and_organ_claims_project_to_cards() {
        let mut cx = Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let profile = Symbol::qualified("standard", "rust-clean");
        let organ = Symbol::qualified("standard", "reader");
        let test = Symbol::qualified("test", "reader-roundtrip");

        publish_profile_claims(&mut cx, profile.clone(), [organ.clone()], [test.clone()]).unwrap();
        publish_organ_claims(&mut cx, organ.clone(), [standard_control_op_key("prompt")]).unwrap();
        publish_fidelity_badge(
            &mut cx,
            Ref::Symbol(profile.clone()),
            Symbol::qualified("standard", "full"),
            Ref::Symbol(test.clone()),
        )
        .unwrap();

        let profile_card = card_expr(&mut cx, Ref::Symbol(profile));
        assert_eq!(
            table_value(&profile_card, "kind"),
            Some(&Expr::Symbol(standard_profile_kind()))
        );
        assert_list_contains_symbol(table_value(&profile_card, "tests").unwrap(), test);

        let organ_card = card_expr(&mut cx, Ref::Symbol(organ));
        assert_eq!(
            table_value(&organ_card, "kind"),
            Some(&Expr::Symbol(standard_organ_kind()))
        );
        assert_list_contains_symbol(
            table_value(&organ_card, "ops").unwrap(),
            Symbol::qualified("control", "prompt.v1"),
        );
    }

    fn assert_has_claim(cx: &Cx, subject: Ref, predicate: Symbol, object: Ref) {
        let claims = cx
            .query_facts(ClaimPattern::exact(subject, predicate, object))
            .unwrap();
        assert_eq!(claims.len(), 1);
    }

    fn card_expr(cx: &mut Cx, subject: Ref) -> Expr {
        card_for_ref(cx, subject)
            .unwrap()
            .object()
            .as_expr(cx)
            .unwrap()
    }

    fn table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a Expr> {
        let Expr::Map(entries) = expr else {
            return None;
        };
        entries.iter().find_map(|(entry_key, entry_value)| {
            let Expr::Symbol(entry_key) = entry_key else {
                return None;
            };
            (entry_key == &Symbol::new(key)).then_some(entry_value)
        })
    }

    fn assert_list_contains_symbol(expr: &Expr, expected: Symbol) {
        assert!(matches!(expr, Expr::List(_)), "expected list");
        let Expr::List(items) = expr else {
            return;
        };
        assert!(
            items
                .iter()
                .any(|item| item == &Expr::Symbol(expected.clone())),
            "expected list to contain {expected}"
        );
    }
}
