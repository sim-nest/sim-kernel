use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    Args, Callable, CapabilityName, ClassRef, Cx, Datum, Error, Expr, HandleId, HandleSeed,
    MatchScore, Object, Op, OpKey, OpSpec, Ref, Result, Shape, ShapeDoc, ShapeMatch, Step, Symbol,
    Value, core_any_ref, core_call_op_key, core_shape_check_value_op_key, datum_store::DatumStore,
    handle_store::HandleStore, invoke_op,
};

fn qsym(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

struct EchoCallable {
    calls: Arc<AtomicUsize>,
}

impl Object for EchoCallable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<echo-callable>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EchoCallable {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_FUNCTION_CLASS_ID, qsym("core", "Function"))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for EchoCallable {
    fn call(&self, _cx: &mut Cx, args: Args) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        args.values()
            .first()
            .cloned()
            .ok_or_else(|| Error::Eval("echo callable expects at least one argument".to_owned()))
    }
}

struct CountingOp {
    spec: OpSpec,
    calls: Arc<AtomicUsize>,
}

impl Op for CountingOp {
    fn spec(&self) -> &OpSpec {
        &self.spec
    }

    fn invoke_authorized(&self, _cx: &mut Cx, input: Value) -> Result<Step> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Value(input))
    }
}

struct OpObject {
    op: CountingOp,
}

impl Object for OpObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<op-object>".to_owned())
    }

    fn op(&self, key: &OpKey) -> Option<&dyn Op> {
        (key == &self.op.spec.key).then_some(&self.op as &dyn Op)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for OpObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_CLASS_CLASS_ID, qsym("core", "Object"))
    }
}

#[derive(Default)]
struct AcceptShape {
    value_checks: AtomicUsize,
}

impl Shape for AcceptShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(qsym("test", "AcceptShape"))
    }

    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        self.value_checks.fetch_add(1, Ordering::SeqCst);
        Ok(ShapeMatch::accept(MatchScore::exact(10)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(5)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("accept"))
    }
}

struct RejectShape;

impl Shape for RejectShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(ShapeMatch::reject("rejected by test shape"))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::reject("value-only shape"))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("reject"))
    }
}

#[test]
fn current_callable_invokes_through_invoke_op() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let target = cx
        .factory()
        .opaque(Arc::new(EchoCallable {
            calls: calls.clone(),
        }))
        .unwrap();
    let expected = cx.factory().string("ok".to_owned()).unwrap();
    let input = cx.factory().list(vec![expected.clone()]).unwrap();

    let step = invoke_op(&mut cx, target, &core_call_op_key(), input).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(step, Step::Value(value) if value == expected));
}

#[test]
fn missing_capability_prevents_op_implementation_from_running() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let spec = OpSpec::new(
        key.clone(),
        Ref::Symbol(qsym("test", "counter")),
        core_any_ref(),
        core_any_ref(),
    )
    .requiring(CapabilityName::new("test.required"));
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec,
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let err = invoke_op(&mut cx, target, &key, input).unwrap_err();

    assert!(
        matches!(err, Error::CapabilityDenied { capability } if capability.as_str() == "test.required")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn missing_args_shape_prevents_op_implementation_from_running() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let missing = qsym("test", "MissingShape");
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec: OpSpec::new(
                    key.clone(),
                    core_any_ref(),
                    Ref::Symbol(missing.clone()),
                    core_any_ref(),
                ),
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let err = invoke_op(&mut cx, target, &key, input).unwrap_err();

    assert!(matches!(err, Error::UnknownSymbol { symbol } if symbol == missing));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn non_shape_args_shape_prevents_op_implementation_from_running() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let non_shape = qsym("test", "NonShape");
    let value = cx.factory().bool(true).unwrap();
    cx.registry_mut()
        .register_shape_value(non_shape.clone(), value)
        .unwrap();
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec: OpSpec::new(
                    key.clone(),
                    core_any_ref(),
                    Ref::Symbol(non_shape),
                    core_any_ref(),
                ),
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let err = invoke_op(&mut cx, target, &key, input).unwrap_err();

    assert!(matches!(
        err,
        Error::TypeMismatch {
            expected: "shape",
            found: "non-shape"
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn handle_args_shape_ref_is_checked_before_op_runs() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let shape = cx.factory().opaque(Arc::new(RejectShape)).unwrap();
    let shape_ref = Ref::Handle(cx.handles_mut().intern(shape));
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec: OpSpec::new(key.clone(), core_any_ref(), shape_ref, core_any_ref()),
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let err = invoke_op(&mut cx, target, &key, input).unwrap_err();

    assert!(matches!(err, Error::WrongShape { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn content_args_shape_ref_must_resolve_to_a_shape() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let content = cx.datum_store_mut().intern(Datum::Bool(true)).unwrap();
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec: OpSpec::new(
                    key.clone(),
                    core_any_ref(),
                    Ref::Content(content),
                    core_any_ref(),
                ),
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let err = invoke_op(&mut cx, target, &key, input).unwrap_err();

    assert!(matches!(
        err,
        Error::TypeMismatch {
            expected: "shape",
            found: "non-shape"
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn unresolved_args_shape_ref_fails_closed() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let missing = Ref::Handle(HandleId::from_seed_and_sequence(HandleSeed::new(7), 99));
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec: OpSpec::new(key.clone(), core_any_ref(), missing.clone(), core_any_ref()),
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let err = invoke_op(&mut cx, target, &key, input).unwrap_err();

    assert!(matches!(
        err,
        Error::UnresolvedShapeRef { reference } if *reference == missing
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn core_any_shape_args_shape_allows_op_implementation_to_run() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = OpKey::new(Symbol::new("test"), Symbol::new("count"), 1);
    let target = cx
        .factory()
        .opaque(Arc::new(OpObject {
            op: CountingOp {
                spec: OpSpec::new(
                    key.clone(),
                    core_any_ref(),
                    Ref::Symbol(qsym("core", "AnyShape")),
                    core_any_ref(),
                ),
                calls: calls.clone(),
            },
        }))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let step = invoke_op(&mut cx, target, &key, input.clone()).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(step, Step::Value(value) if value == input));
}

#[test]
fn current_shape_checks_through_invoke_op() {
    let mut cx = Cx::stub();
    let target = cx
        .factory()
        .opaque(Arc::new(AcceptShape::default()))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();

    let step = invoke_op(&mut cx, target, &core_shape_check_value_op_key(), input).unwrap();

    let Step::Value(value) = step else {
        panic!("shape check should return a value step");
    };
    let matched = value
        .object()
        .downcast_ref::<crate::ShapeMatchObject>()
        .unwrap()
        .matched();
    assert!(matched.accepted);
    assert_eq!(matched.score.value(), 10);
}

#[test]
fn old_direct_call_paths_still_work() {
    let mut cx = Cx::stub();
    let calls = Arc::new(AtomicUsize::new(0));
    let target = cx
        .factory()
        .opaque(Arc::new(EchoCallable {
            calls: calls.clone(),
        }))
        .unwrap();
    let expected = cx.factory().string("direct".to_owned()).unwrap();

    let actual = cx
        .call_value(target, Args::new(vec![expected.clone()]))
        .unwrap();

    assert_eq!(actual, expected);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn old_direct_shape_path_still_works() {
    let mut cx = Cx::stub();
    let shape_value = cx
        .factory()
        .opaque(Arc::new(AcceptShape::default()))
        .unwrap();
    let input = cx.factory().bool(true).unwrap();
    let shape = shape_value.object().as_shape().unwrap();

    let matched = shape.check_value(&mut cx, input).unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.score.value(), 10);
}
