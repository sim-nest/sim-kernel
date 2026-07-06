use std::sync::Arc;

use crate::{
    CapabilityName, CapabilitySet, Claim, ClaimKind, ClaimPattern, Datum, DatumStore,
    DefaultFactory, Error, FactStore, NoopEvalPolicy, Ref, Symbol, Visibility,
    fact_private_capability,
};

fn cx() -> crate::Cx {
    crate::Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn sym(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn ref_sym(namespace: &str, name: &str) -> Ref {
    Ref::Symbol(sym(namespace, name))
}

fn public_claim(name: &str) -> Claim {
    Claim::public(
        ref_sym("test", name),
        sym("test", "predicate"),
        ref_sym("test", "object"),
    )
}

fn exact_for(claim: &Claim) -> ClaimPattern {
    ClaimPattern::exact(
        claim.subject.clone(),
        claim.predicate.clone(),
        claim.object.clone(),
    )
}

#[test]
fn public_claim_inserts_and_queries() {
    let mut cx = cx();
    let claim = public_claim("public");

    let reference = cx.insert_fact(claim.clone()).unwrap();

    let Ref::Content(id) = reference else {
        panic!("expected claim insertion to return a content ref");
    };
    assert!(cx.datum_store().contains(&id));

    let results = cx.query_facts(exact_for(&claim)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, Some(id));
    assert_eq!(results[0].subject, claim.subject);
    assert_eq!(results[0].predicate, claim.predicate);
    assert_eq!(results[0].object, claim.object);
}

#[test]
fn private_claim_requires_private_fact_capability_and_is_hidden_normally() {
    let mut cx = cx();
    let claim = public_claim("private").with_visibility(Visibility::Private);

    let denied = cx.insert_fact(claim.clone()).unwrap_err();
    assert!(matches!(denied, Error::CapabilityDenied { .. }));

    cx.grant_from_host(fact_private_capability());
    cx.insert_fact(claim.clone()).unwrap();
    assert_eq!(cx.query_facts(exact_for(&claim)).unwrap().len(), 1);

    cx.with_capabilities(CapabilitySet::default(), |cx| {
        assert!(cx.query_facts(exact_for(&claim))?.is_empty());
        Ok(())
    })
    .unwrap();
}

#[test]
fn capability_gated_claim_is_hidden_without_capability() {
    let mut cx = cx();
    let capability = CapabilityName::new("test.secret");
    let claim = public_claim("gated")
        .with_visibility(Visibility::CapabilityGated)
        .requiring(capability.clone());

    cx.insert_fact(claim.clone()).unwrap();

    assert!(cx.query_facts(exact_for(&claim)).unwrap().is_empty());

    cx.grant_from_host(capability);
    assert_eq!(cx.query_facts(exact_for(&claim)).unwrap().len(), 1);
}

#[test]
fn revoked_claim_is_hidden_unless_requested() {
    let mut cx = cx();
    let claim = public_claim("revoked").with_kind(ClaimKind::Revoked);

    cx.insert_fact(claim.clone()).unwrap();

    assert!(cx.query_facts(exact_for(&claim)).unwrap().is_empty());
    assert_eq!(
        cx.query_facts(exact_for(&claim).include_revoked())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn claim_object_literals_are_content_refs() {
    let mut cx = cx();
    let object = Datum::String("literal object".to_owned());
    let claim = Claim::content_object(
        cx.datum_store_mut(),
        ref_sym("test", "literal-subject"),
        sym("test", "literal-predicate"),
        object.clone(),
    )
    .unwrap();

    let Ref::Content(object_id) = &claim.object else {
        panic!("expected literal object to be interned as content");
    };
    assert_eq!(cx.datum_store().get(object_id).unwrap(), Some(&object));

    cx.insert_fact(claim.clone()).unwrap();
    let results = cx.query_facts(exact_for(&claim)).unwrap();
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].object, Ref::Content(_)));
}

#[test]
fn cx_installs_minimal_core_boot_claims() {
    let cx = cx();
    let results = cx
        .query_facts(ClaimPattern::exact(
            ref_sym("core", "Bool"),
            sym("core", "kind"),
            ref_sym("core", "class"),
        ))
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[test]
fn fact_insertion_does_not_change_registry_behavior() {
    let mut cx = cx();
    let subject = sym("test", "fact-only");
    let claim = Claim::public(
        Ref::Symbol(subject.clone()),
        sym("test", "predicate"),
        ref_sym("test", "object"),
    );

    cx.insert_fact(claim).unwrap();

    assert!(matches!(
        cx.resolve_value(&subject),
        Err(Error::UnknownSymbol { .. })
    ));
}

#[test]
fn direct_fact_store_queries_match_cx_helpers() {
    let mut cx = cx();
    let claim = public_claim("direct-store");
    cx.insert_fact(claim.clone()).unwrap();

    let direct = cx.facts().query_authorized(&cx, exact_for(&claim)).unwrap();
    let helper = cx.query_facts(exact_for(&claim)).unwrap();

    assert_eq!(direct, helper);
}
