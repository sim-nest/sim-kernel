use std::{any::Any, sync::Arc};

use super::{check_value_report, satisfies_shape_predicate, shape_report_from_match};
use crate::{
    ClaimPattern, ClassRef, Cx, Datum, Expr, MatchScore, NumberLiteral, Object, QuoteMode, Ref,
    Result, Shape, ShapeBindings, ShapeDoc, ShapeMatch, Symbol, Value, Visibility,
    datum_store::DatumStore, fact_private_capability,
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

fn sym(name: &str) -> Symbol {
    Symbol::new(name)
}

fn qsym(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn number(value: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: qsym("numbers", "f64"),
        canonical: value.to_owned(),
    })
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

#[test]
fn executable_expr_captures_are_structural_terms() {
    let mut cx = cx();
    let mut captures = ShapeBindings::new();
    captures.bind_expr(
        sym("call"),
        Expr::Call {
            operator: Box::new(Expr::Symbol(sym("f"))),
            args: vec![number("1")],
        },
    );
    captures.bind_expr(
        sym("infix"),
        Expr::Infix {
            operator: sym("+"),
            left: Box::new(number("1")),
            right: Box::new(number("2")),
        },
    );
    captures.bind_expr(
        sym("quote"),
        Expr::Quote {
            mode: QuoteMode::Quote,
            expr: Box::new(Expr::List(vec![Expr::Symbol(sym("x"))])),
        },
    );
    captures.bind_expr(
        sym("annotated"),
        Expr::Annotated {
            expr: Box::new(Expr::Local(sym("x"))),
            annotations: vec![(sym("doc"), Expr::String("value".to_owned()))],
        },
    );
    let matched = ShapeMatch {
        accepted: true,
        captures,
        score: MatchScore::exact(1),
        diagnostics: Vec::new(),
    };

    let report = shape_report_from_match(
        &mut cx,
        Ref::Symbol(qsym("test", "shape")),
        Ref::Symbol(qsym("test", "target")),
        matched,
    )
    .unwrap();
    let Ref::Content(captures_id) = report.captures else {
        panic!("captures should be interned content");
    };
    let captures_datum = cx
        .datum_store()
        .get(&captures_id)
        .unwrap()
        .expect("captures datum should exist");

    assert!(!contains_debug_record(captures_datum));
    assert_eq!(
        expr_binding_term_kinds(captures_datum),
        vec!["call", "call", "quote", "annotated"]
    );
}

fn expr_binding_term_kinds(datum: &Datum) -> Vec<&str> {
    let Datum::Node { fields, .. } = datum else {
        panic!("captures should be a node");
    };
    let Datum::Vector(exprs) = field(fields, "exprs") else {
        panic!("exprs should be a vector");
    };
    exprs
        .iter()
        .map(|binding| {
            let Datum::Node { fields, .. } = binding else {
                panic!("binding should be a node");
            };
            let Datum::Node {
                fields: value_fields,
                ..
            } = field(fields, "value")
            else {
                panic!("binding value should be a term node");
            };
            let Datum::Symbol(kind) = field(value_fields, "kind") else {
                panic!("term should carry a kind");
            };
            kind.name.as_ref()
        })
        .collect()
}

fn contains_debug_record(datum: &Datum) -> bool {
    match datum {
        Datum::Node { tag, fields } => {
            tag == &qsym("core", "expr-canonical-key")
                || fields.iter().any(|(name, value)| {
                    name.name.as_ref() == "debug" || contains_debug_record(value)
                })
        }
        Datum::List(items) | Datum::Vector(items) | Datum::Set(items) => {
            items.iter().any(contains_debug_record)
        }
        Datum::Map(entries) => entries
            .iter()
            .any(|(key, value)| contains_debug_record(key) || contains_debug_record(value)),
        _ => false,
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a Datum {
    fields
        .iter()
        .find_map(|(field, value)| (field.name.as_ref() == name).then_some(value))
        .unwrap_or_else(|| panic!("missing field {name}"))
}
