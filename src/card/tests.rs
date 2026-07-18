use std::sync::Arc;

use crate::{
    Claim, ClaimPattern, DefaultFactory, Expr, NoopEvalPolicy, Ref, Symbol,
    card::card_fixed_predicates, card::card_for_ref, card::card_for_ref_with_fallback,
    card::card_for_value, card::minimal_card,
};

fn cx() -> crate::Cx {
    crate::Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
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

fn table_keys(expr: &Expr) -> Vec<Symbol> {
    let Expr::Map(entries) = expr else {
        panic!("expected map expression");
    };
    entries
        .iter()
        .map(|(entry_key, _)| match entry_key {
            Expr::Symbol(symbol) => symbol.clone(),
            _ => panic!("expected symbol key"),
        })
        .collect()
}

#[test]
fn card_fixed_field_order_is_stable() {
    let mut cx = cx();
    let subject = Ref::Symbol(Symbol::qualified("test", "ordered"));

    let card = card_for_ref(&mut cx, subject).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();
    let keys = table_keys(&expr);

    assert_eq!(
        &keys[..10],
        &[
            Symbol::new("subject"),
            Symbol::new("kind"),
            Symbol::new("help"),
            Symbol::new("args"),
            Symbol::new("result"),
            Symbol::new("tests"),
            Symbol::new("ops"),
            Symbol::new("requires"),
            Symbol::new("see-also"),
            Symbol::new("shape-known"),
        ]
    );
    assert_eq!(
        card_fixed_predicates(),
        vec![
            Symbol::qualified("core", "subject"),
            Symbol::qualified("core", "kind"),
            Symbol::qualified("core", "help"),
            Symbol::qualified("core", "args"),
            Symbol::qualified("core", "result"),
            Symbol::qualified("core", "tests"),
            Symbol::qualified("core", "ops"),
            Symbol::qualified("core", "requires"),
            Symbol::qualified("core", "see-also"),
            Symbol::qualified("core", "shape-known"),
        ]
    );
}

#[test]
fn unknown_ref_produces_minimal_card() {
    let mut cx = cx();
    let subject = Ref::Symbol(Symbol::qualified("missing", "thing"));

    let card = card_for_ref(&mut cx, subject).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(
        table_value(&expr, "subject"),
        Some(&Expr::Symbol(Symbol::qualified("missing", "thing")))
    );
    assert_eq!(
        table_value(&expr, "kind"),
        Some(&Expr::Symbol(Symbol::qualified("core", "unknown")))
    );
    assert_eq!(
        table_value(&expr, "help"),
        Some(&Expr::String(String::new()))
    );
    assert_eq!(
        table_value(&expr, "args"),
        Some(&Expr::Symbol(Symbol::qualified("core", "Any")))
    );
    assert_eq!(
        table_value(&expr, "result"),
        Some(&Expr::Symbol(Symbol::qualified("core", "Any")))
    );
    assert_eq!(table_value(&expr, "tests"), Some(&Expr::List(Vec::new())));
    assert_eq!(table_value(&expr, "ops"), Some(&Expr::List(Vec::new())));
    assert_eq!(
        table_value(&expr, "requires"),
        Some(&Expr::List(Vec::new()))
    );
    assert_eq!(
        table_value(&expr, "see-also"),
        Some(&Expr::List(Vec::new()))
    );
    assert_eq!(table_value(&expr, "shape-known"), Some(&Expr::Bool(false)));
}

#[test]
fn every_value_produces_a_minimal_card() {
    let mut cx = cx();
    let value = cx.factory().string("hello".to_owned()).unwrap();

    let card = card_for_value(&mut cx, value).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert!(table_value(&expr, "subject").is_some());
    assert_eq!(
        table_value(&expr, "kind"),
        Some(&Expr::Symbol(Symbol::qualified("core", "unknown")))
    );
    assert_eq!(
        table_value(&expr, "args"),
        Some(&Expr::Symbol(Symbol::qualified("core", "Any")))
    );
    assert_eq!(
        table_value(&expr, "result"),
        Some(&Expr::Symbol(Symbol::qualified("core", "Any")))
    );
    assert_eq!(table_value(&expr, "shape-known"), Some(&Expr::Bool(false)));
}

#[test]
fn visible_claims_override_minimal_card_defaults() {
    let mut cx = cx();
    let subject = Ref::Symbol(Symbol::qualified("test", "callable"));
    cx.insert_fact(Claim::public(
        subject.clone(),
        Symbol::qualified("core", "kind"),
        Ref::Symbol(Symbol::qualified("core", "function")),
    ))
    .unwrap();
    let help = crate::Claim::content_object(
        cx.datum_store_mut(),
        subject.clone(),
        Symbol::qualified("core", "help"),
        crate::Datum::String("callable help".to_owned()),
    )
    .unwrap();
    cx.insert_fact(help).unwrap();
    cx.insert_fact(Claim::public(
        subject.clone(),
        Symbol::qualified("core", "tests"),
        Ref::Symbol(Symbol::qualified("test", "callable-test")),
    ))
    .unwrap();

    let card = card_for_ref(&mut cx, subject).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(
        table_value(&expr, "kind"),
        Some(&Expr::Symbol(Symbol::qualified("core", "function")))
    );
    assert_eq!(
        table_value(&expr, "help"),
        Some(&Expr::String("callable help".to_owned()))
    );
    let Some(Expr::List(tests)) = table_value(&expr, "tests") else {
        panic!("tests should be a list");
    };
    assert_eq!(
        tests,
        &[Expr::Symbol(Symbol::qualified("test", "callable-test"))]
    );
}

#[test]
fn fallback_table_preserves_existing_metadata_fields() {
    let mut cx = cx();
    let subject = Ref::Symbol(Symbol::qualified("test", "old-function"));
    let fallback = cx
        .factory()
        .table(vec![
            (
                Symbol::new("symbol"),
                cx.factory().string("test/old-function".to_owned()).unwrap(),
            ),
            (
                Symbol::new("case-count"),
                cx.factory()
                    .number_literal(Symbol::qualified("numbers", "f64"), "2".to_owned())
                    .unwrap(),
            ),
        ])
        .unwrap();

    let card = card_for_ref_with_fallback(
        &mut cx,
        subject,
        Some(fallback),
        Some(Symbol::qualified("core", "function")),
    )
    .unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(
        table_value(&expr, "kind"),
        Some(&Expr::Symbol(Symbol::qualified("core", "function")))
    );
    assert_eq!(
        table_value(&expr, "symbol"),
        Some(&Expr::String("test/old-function".to_owned()))
    );
    assert!(table_value(&expr, "case-count").is_some());

    let claims = cx
        .query_facts(ClaimPattern {
            subject: Some(Ref::Symbol(Symbol::qualified("test", "old-function"))),
            predicate: Some(Symbol::qualified("core", "kind")),
            object: Some(Ref::Symbol(Symbol::qualified("core", "function"))),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);
}

#[test]
fn symbolic_subject_is_card_identity() {
    let mut cx = cx();
    let subject = Ref::Symbol(Symbol::qualified("core", "help"));

    let card = minimal_card(&mut cx, subject).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(
        table_value(&expr, "subject"),
        Some(&Expr::Symbol(Symbol::qualified("core", "help")))
    );
    assert!(table_value(&expr, "class-id").is_none());
}
