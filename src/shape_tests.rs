use std::sync::Arc;

use crate::{
    Callable, DefaultFactory, Error, NoopEvalPolicy, Value,
    object::{Args, RawArgs},
    shape::{
        ExprKind, MatchScore, Shape, ShapeBindings, ShapeCallTarget, ShapeDoc, ShapeMatch,
        call_shape,
    },
};

struct AnyKernelShape;

impl Shape for AnyKernelShape {
    fn check_value(&self, _cx: &mut crate::Cx, _value: Value) -> crate::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(7)))
    }

    fn check_expr(&self, _cx: &mut crate::Cx, _expr: &crate::Expr) -> crate::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(5)))
    }

    fn describe(&self, _cx: &mut crate::Cx) -> crate::Result<ShapeDoc> {
        Ok(ShapeDoc::new("kernel-any"))
    }
}

fn cx() -> crate::Cx {
    crate::Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

#[test]
fn expr_kind_names_and_matches_live_in_kernel() {
    assert!(ExprKind::String.matches(&crate::Expr::String("ok".to_owned())));
    assert_eq!(ExprKind::String.name(), "string");
}

#[test]
fn shape_bindings_can_populate_env_from_kernel() {
    let mut cx = cx();
    let mut bindings = ShapeBindings::new();
    bindings.bind_expr(
        crate::Symbol::new("s"),
        crate::Expr::String("hello".to_owned()),
    );
    bindings.bind_value(crate::Symbol::new("n"), cx.factory().bool(true).unwrap());
    bindings.into_env(&mut cx).unwrap();
    assert!(cx.env().get(&crate::Symbol::new("s")).is_some());
    assert!(cx.env().get(&crate::Symbol::new("n")).is_some());
}

#[test]
fn kernel_shape_call_returns_match_table() {
    let mut cx = cx();
    let value = cx.factory().string("ok".to_owned()).unwrap();
    let matched = call_shape(&mut cx, &AnyKernelShape, ShapeCallTarget::Value(value)).unwrap();
    let table = matched.object().as_table(&mut cx).unwrap();
    let table = table.object().as_table_impl().unwrap();
    let _ = table.get(&mut cx, crate::Symbol::new("accepted")).unwrap();
    let _ = table.get(&mut cx, crate::Symbol::new("score")).unwrap();
}

#[test]
fn summed_rejects_stay_below_accept() {
    let mut score = MatchScore::reject();
    for _ in 0..64 {
        score += MatchScore::reject();
    }
    assert!(score < MatchScore::exact(0));
}

#[test]
fn shape_callable_accepts_exactly_one_argument() {
    let mut cx = cx();
    let shape = AnyKernelShape;
    let value = cx.factory().string("ok".to_owned()).unwrap();

    let matched = shape
        .call(&mut cx, Args::new(vec![value.clone()]))
        .expect("one evaluated value should match");
    assert!(matched.object().truth(&mut cx).unwrap());

    let error = shape.call(&mut cx, Args::new(Vec::new())).unwrap_err();
    assert!(matches!(error, Error::Eval(message) if message == "shape call expects 1 argument"));

    let error = shape
        .call(&mut cx, Args::new(vec![value.clone(), value]))
        .unwrap_err();
    assert!(matches!(error, Error::Eval(message) if message == "shape call expects 1 argument"));

    let error = shape
        .call_exprs(&mut cx, RawArgs::new(Vec::new()))
        .unwrap_err();
    assert!(matches!(error, Error::Eval(message) if message == "shape call expects 1 expression"));
}
