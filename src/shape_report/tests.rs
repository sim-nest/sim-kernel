use std::{any::Any, sync::Arc};

use super::{check_value_report, satisfies_shape_predicate};
use crate::{
    ClaimPattern, ClassRef, Cx, MatchScore, Object, Ref, Result, Shape, ShapeDoc, ShapeMatch,
    Symbol, Value, Visibility, fact_private_capability,
};

struct TestShape {
    accepted: bool,
}

impl Shape for TestShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("test", "shape-report-shape"))
    }

    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        if self.accepted {
            Ok(ShapeMatch::accept(MatchScore::exact(9)))
        } else {
            Ok(ShapeMatch::reject("shape report rejected value"))
        }
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &crate::Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(3)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("test shape"))
    }
}

struct OpaqueTarget {
    publish: bool,
}

impl Object for OpaqueTarget {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<opaque-target>".to_owned())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl crate::ObjectCompat for OpaqueTarget {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().nil()
    }
    fn publish_shape_satisfaction_claims(&self, _cx: &mut Cx, _shape: &Ref) -> Result<bool> {
        Ok(self.publish)
    }
}

use crate::testing::bare_cx as cx;

fn shape_value(cx: &mut Cx, accepted: bool) -> Value {
    cx.factory()
        .opaque(Arc::new(TestShape { accepted }))
        .unwrap()
}

fn opaque_value(cx: &mut Cx, publish: bool) -> Value {
    cx.factory()
        .opaque(Arc::new(OpaqueTarget { publish }))
        .unwrap()
}

#[test]
fn successful_check_produces_report() {
    let mut cx = cx();
    let shape = shape_value(&mut cx, true);
    let value = cx.factory().string("stable".to_owned()).unwrap();

    let report = check_value_report(&mut cx, &shape, value).unwrap();

    assert!(report.accepted);
    assert_eq!(report.score, MatchScore::exact(9));
    assert_eq!(
        report.shape,
        Ref::Symbol(Symbol::qualified("test", "shape-report-shape"))
    );
    assert!(matches!(report.target, Ref::Content(_)));
    assert!(matches!(report.captures, Ref::Content(_)));
    assert!(matches!(report.id, Ref::Content(_)));
}

#[test]
fn successful_stable_target_inserts_satisfaction_claim() {
    let mut cx = cx();
    let shape = shape_value(&mut cx, true);
    let value = cx.factory().string("stable".to_owned()).unwrap();

    let report = check_value_report(&mut cx, &shape, value).unwrap();
    let claims = cx
        .query_facts(ClaimPattern::exact(
            report.target.clone(),
            satisfies_shape_predicate(),
            report.shape.clone(),
        ))
        .unwrap();

    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].evidence, vec![report.id.clone()]);
    assert_eq!(claims[0].visibility, Visibility::Public);
}

#[test]
fn failed_check_produces_report_without_satisfaction_claim() {
    let mut cx = cx();
    let shape = shape_value(&mut cx, false);
    let value = cx.factory().string("stable".to_owned()).unwrap();

    let report = check_value_report(&mut cx, &shape, value).unwrap();
    let claims = cx
        .query_facts(ClaimPattern::exact(
            report.target.clone(),
            satisfies_shape_predicate(),
            report.shape.clone(),
        ))
        .unwrap();

    assert!(!report.accepted);
    assert!(!report.diagnostics.is_empty());
    assert!(claims.is_empty());
}

#[test]
fn live_handle_satisfaction_claim_is_private_by_default() {
    let mut cx = cx();
    let shape = shape_value(&mut cx, true);
    let value = opaque_value(&mut cx, false);

    let report = check_value_report(&mut cx, &shape, value).unwrap();
    let pattern = ClaimPattern::exact(
        report.target.clone(),
        satisfies_shape_predicate(),
        report.shape.clone(),
    );

    assert!(matches!(report.target, Ref::Handle(_)));
    assert!(cx.query_facts(pattern.clone()).unwrap().is_empty());
    cx.grant_from_host(fact_private_capability());
    let claims = cx.query_facts(pattern).unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].visibility, Visibility::Private);
}

#[test]
fn live_handle_owner_can_publish_satisfaction_claim() {
    let mut cx = cx();
    let shape = shape_value(&mut cx, true);
    let value = opaque_value(&mut cx, true);

    let report = check_value_report(&mut cx, &shape, value).unwrap();
    let claims = cx
        .query_facts(ClaimPattern::exact(
            report.target.clone(),
            satisfies_shape_predicate(),
            report.shape.clone(),
        ))
        .unwrap();

    assert!(matches!(report.target, Ref::Handle(_)));
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].visibility, Visibility::Public);
}
