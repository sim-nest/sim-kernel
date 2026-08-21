use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    Demand, EagerPolicy, EvalPolicy, MacroExpander, NoopEvalPolicy, Phase, PreparedArgs, Result,
    StrictNames, Symbol, Thunk, ThunkObject,
    callable::Callable,
    capability::macro_expansion_capability_for_phase,
    env::Cx,
    error::Error,
    expr::Expr,
    factory::DefaultFactory,
    id::{CORE_EVAL_REQUEST_CLASS_ID, CORE_FUNCTION_CLASS_ID, CORE_SHAPE_CLASS_ID, ShapeId},
    object::{Args, Object, RawArgs},
    shape::{MatchScore, Shape, ShapeDoc, ShapeMatch},
    value::Value,
};

const TEST_TICK_CALLABLE_CLASS_ID: crate::ClassId = CORE_FUNCTION_CLASS_ID;
const TEST_RECURSIVE_FORCE_CALLABLE_CLASS_ID: crate::ClassId = CORE_SHAPE_CLASS_ID;
const TEST_FAIL_THEN_SUCCEED_CALLABLE_CLASS_ID: crate::ClassId = CORE_EVAL_REQUEST_CLASS_ID;

struct TickCallable {
    counter: Arc<AtomicUsize>,
}

impl Callable for TickCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        cx.factory().bool(true)
    }
}

impl Object for TickCallable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<tick-callable>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for TickCallable {
    fn class(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().class_stub(
            TEST_TICK_CALLABLE_CLASS_ID,
            Symbol::qualified("test", "TickCallable"),
        )
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

struct RecursiveForceCallable {
    thunk: Arc<Mutex<Option<Value>>>,
}

impl Callable for RecursiveForceCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        let thunk = self
            .thunk
            .lock()
            .map_err(|_| Error::PoisonedLock("recursive thunk handle"))?
            .clone()
            .ok_or_else(|| Error::Eval("missing recursive thunk".to_owned()))?;
        cx.force(thunk, Demand::Value)
    }
}

impl Object for RecursiveForceCallable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<recursive-force-callable>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for RecursiveForceCallable {
    fn class(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().class_stub(
            TEST_RECURSIVE_FORCE_CALLABLE_CLASS_ID,
            Symbol::qualified("test", "RecursiveForceCallable"),
        )
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

struct FailThenSucceedCallable {
    attempts: Arc<AtomicUsize>,
}

impl Callable for FailThenSucceedCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            Err(Error::Eval("boom".to_owned()))
        } else {
            cx.factory().bool(true)
        }
    }
}

impl Object for FailThenSucceedCallable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<fail-then-succeed-callable>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for FailThenSucceedCallable {
    fn class(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().class_stub(
            TEST_FAIL_THEN_SUCCEED_CALLABLE_CLASS_ID,
            Symbol::qualified("test", "FailThenSucceedCallable"),
        )
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

struct FixedShape {
    accepted: bool,
}

impl Shape for FixedShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        if self.accepted {
            Ok(ShapeMatch::accept(MatchScore::exact(1)))
        } else {
            Ok(ShapeMatch::reject("fixed rejection"))
        }
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::reject("value-only shape"))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("fixed"))
    }
}

struct RejectUnboundNamesPolicy;

impl EvalPolicy for RejectUnboundNamesPolicy {
    fn name(&self) -> &'static str {
        "reject-unbound-names-test"
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        EagerPolicy.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        EagerPolicy.force(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        EagerPolicy.eval_expr(cx, expr)
    }

    fn resolve_unbound_symbol(&self, _cx: &mut Cx, symbol: Symbol) -> Result<Value> {
        Err(Error::UnknownSymbol { symbol })
    }

    fn resolve_unbound_call(
        &self,
        _cx: &mut Cx,
        operator: Symbol,
        _args: Vec<Expr>,
    ) -> Result<Value> {
        Err(Error::UnknownFunction { function: operator })
    }
}

struct CountingMacroExpander {
    calls: Arc<AtomicUsize>,
}

impl MacroExpander for CountingMacroExpander {
    fn expand_expr(&self, _cx: &mut Cx, _phase: Phase, _expr: Expr) -> Result<Expr> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(expanded_macro_expr())
    }
}

fn test_cx() -> Cx {
    Cx::new(
        Arc::new(EagerPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(7),
    )
}

fn strict_names_cx() -> Cx {
    Cx::new(
        Arc::new(StrictNames(EagerPolicy)),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(7),
    )
}

fn registered_shape(cx: &mut Cx, name: &str, accepted: bool) -> ShapeId {
    let shape = cx
        .factory()
        .opaque(Arc::new(FixedShape { accepted }))
        .unwrap();
    cx.registry_mut()
        .register_shape_value(Symbol::qualified("test", name), shape)
        .unwrap()
}

fn macro_input_expr() -> Expr {
    Expr::Symbol(Symbol::qualified("macro-test", "input"))
}

fn expanded_macro_expr() -> Expr {
    Expr::Symbol(Symbol::qualified("macro-test", "expanded"))
}

#[test]
fn noop_policy_denies_macro_expansion_before_expander_runs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut cx = Cx::new(
        Arc::new(NoopEvalPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(7),
    );
    cx.set_macro_expander(Arc::new(CountingMacroExpander {
        calls: calls.clone(),
    }));

    let err = cx
        .expand_macros(Phase::Expand, macro_input_expr())
        .unwrap_err();

    assert!(
        matches!(err, Error::Eval(message) if message.contains("macro expansion denied by eval policy noop"))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn policy_allowed_macro_expansion_requires_phase_capability() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut cx = test_cx();
    cx.set_macro_expander(Arc::new(CountingMacroExpander {
        calls: calls.clone(),
    }));

    let err = cx
        .expand_macros(Phase::Compile, macro_input_expr())
        .unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability }
            if capability == macro_expansion_capability_for_phase(Phase::Compile)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn macro_expansion_runs_with_policy_and_capability() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut cx, seat) = Cx::new_seated(
        Arc::new(EagerPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(7),
    );
    seat.grant(&mut cx, macro_expansion_capability_for_phase(Phase::Eval))
        .unwrap();
    cx.set_macro_expander(Arc::new(CountingMacroExpander {
        calls: calls.clone(),
    }));

    let expanded = cx.expand_macros(Phase::Eval, macro_input_expr()).unwrap();

    assert_eq!(expanded, expanded_macro_expr());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn default_policy_self_quotes_unbound_value_symbol() {
    let mut cx = test_cx();
    let symbol = Symbol::qualified("test", "free");

    let value = cx.eval_expr(Expr::Symbol(symbol.clone())).unwrap();

    assert_eq!(
        value.object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(symbol)
    );
}

#[test]
fn eval_policy_can_reject_unbound_value_symbol() {
    let mut cx = Cx::new(
        Arc::new(RejectUnboundNamesPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(7),
    );
    let symbol = Symbol::qualified("test", "free");

    let err = cx.eval_expr(Expr::Symbol(symbol.clone())).unwrap_err();

    assert!(matches!(err, Error::UnknownSymbol { symbol: found } if found == symbol));
}

#[test]
fn default_policy_turns_unbound_operator_call_into_symbolic_call() {
    let mut cx = test_cx();
    let operator = Symbol::qualified("test", "free-call");
    let argument = Symbol::qualified("test", "free-arg");
    let expr = Expr::Call {
        operator: Box::new(Expr::Symbol(operator)),
        args: vec![Expr::Symbol(argument)],
    };

    let value = cx.eval_expr(expr.clone()).unwrap();

    assert_eq!(value.object().as_expr(&mut cx).unwrap(), expr);
}

#[test]
fn eval_policy_can_reject_unbound_operator_call() {
    let mut cx = Cx::new(
        Arc::new(RejectUnboundNamesPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(7),
    );
    let operator = Symbol::qualified("test", "free-call");
    let expr = Expr::Call {
        operator: Box::new(Expr::Symbol(operator.clone())),
        args: Vec::new(),
    };

    let err = cx.eval_expr(expr).unwrap_err();

    assert!(matches!(
        err,
        Error::UnknownFunction { function } if function == operator
    ));
}

#[test]
fn demand_shape_accepts_matching_value() {
    let mut cx = test_cx();
    let shape = registered_shape(&mut cx, "accepting-shape", true);
    let value = cx.factory().string("ok".to_owned()).unwrap();

    let forced = cx.force(value.clone(), Demand::Shape(shape)).unwrap();

    assert_eq!(forced, value);
}

#[test]
fn demand_shape_rejects_mismatched_value() {
    let mut cx = test_cx();
    let shape = registered_shape(&mut cx, "rejecting-shape", false);
    let value = cx.factory().string("bad".to_owned()).unwrap();

    let err = cx.force(value, Demand::Shape(shape)).unwrap_err();

    assert!(matches!(
        err,
        Error::WrongShape {
            expected,
            diagnostics
        } if expected == shape && diagnostics.len() == 1
    ));
}

#[test]
fn demand_shape_requires_registered_shape_id() {
    let mut cx = test_cx();
    let missing = ShapeId(99_999);
    let value = cx.factory().string("unchecked".to_owned()).unwrap();

    let err = cx.force(value, Demand::Shape(missing)).unwrap_err();

    assert!(matches!(err, Error::MissingShape(found) if found == missing));
}

#[test]
fn bound_non_callable_operator_still_rejects_as_non_callable() {
    let mut cx = test_cx();
    let operator = Symbol::qualified("test", "not-callable");
    let non_callable = cx.factory().bool(true).unwrap();
    cx.env_mut().define(operator.clone(), non_callable);

    let err = cx
        .eval_expr(Expr::Call {
            operator: Box::new(Expr::Symbol(operator)),
            args: Vec::new(),
        })
        .unwrap_err();

    assert!(matches!(
        err,
        Error::TypeMismatch {
            expected: "callable",
            found: "non-callable"
        }
    ));
}

#[test]
fn strict_names_rejects_bare_unbound_symbol() {
    let mut cx = strict_names_cx();
    let symbol = Symbol::qualified("test", "free");

    let err = cx.eval_expr(Expr::Symbol(symbol.clone())).unwrap_err();

    assert!(matches!(err, Error::UnknownSymbol { symbol: found } if found == symbol));
}

#[test]
fn strict_names_rejects_unbound_operator_call() {
    let mut cx = strict_names_cx();
    let operator = Symbol::qualified("test", "free-call");
    let expr = Expr::Call {
        operator: Box::new(Expr::Symbol(operator.clone())),
        args: vec![Expr::Bool(true)],
    };

    let err = cx.eval_expr(expr).unwrap_err();

    assert!(matches!(
        err,
        Error::UnknownFunction { function } if function == operator
    ));
}

#[test]
fn strict_names_keeps_inner_policy_for_normal_calls() {
    let mut cx = strict_names_cx();
    let counter = Arc::new(AtomicUsize::new(0));
    let symbol = Symbol::qualified("test", "tick");
    let callable = Value::from_arc(Arc::new(TickCallable {
        counter: counter.clone(),
    }));
    cx.env_mut().define(symbol.clone(), callable);

    let value = cx
        .eval_expr(Expr::Call {
            operator: Box::new(Expr::Symbol(symbol)),
            args: vec![Expr::Bool(true)],
        })
        .unwrap();

    assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn thunk_force_memoizes_successful_evaluation() {
    let mut cx = test_cx();
    let counter = Arc::new(AtomicUsize::new(0));
    let symbol = Symbol::qualified("test", "tick");
    let callable = Value::from_arc(Arc::new(TickCallable {
        counter: counter.clone(),
    }));
    cx.env_mut().define(symbol.clone(), callable);

    let thunk = ThunkObject::new(
        Expr::Call {
            operator: Box::new(Expr::Symbol(symbol)),
            args: Vec::new(),
        },
        cx.env().clone(),
    );

    let first = thunk.force(&mut cx, Demand::Value).unwrap();
    let second = thunk.force(&mut cx, Demand::Value).unwrap();

    assert_eq!(first.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
    assert_eq!(second.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn thunk_force_rejects_recursive_self_force() {
    let mut cx = test_cx();
    let thunk_slot = Arc::new(Mutex::new(None));
    let symbol = Symbol::qualified("test", "recurse");
    let callable = Value::from_arc(Arc::new(RecursiveForceCallable {
        thunk: thunk_slot.clone(),
    }));
    cx.env_mut().define(symbol.clone(), callable);

    let thunk_value = Value::from_arc(Arc::new(ThunkObject::new(
        Expr::Call {
            operator: Box::new(Expr::Symbol(symbol)),
            args: Vec::new(),
        },
        cx.env().clone(),
    )));
    *thunk_slot.lock().unwrap() = Some(thunk_value.clone());

    let err = cx.force(thunk_value, Demand::Value).unwrap_err();
    assert!(matches!(err, Error::RecursiveThunkForce));
}

#[test]
fn thunk_force_restores_pending_state_after_error() {
    let mut cx = test_cx();
    let attempts = Arc::new(AtomicUsize::new(0));
    let symbol = Symbol::qualified("test", "retry");
    let callable = Value::from_arc(Arc::new(FailThenSucceedCallable {
        attempts: attempts.clone(),
    }));
    cx.env_mut().define(symbol.clone(), callable);

    let thunk = ThunkObject::new(
        Expr::Call {
            operator: Box::new(Expr::Symbol(symbol)),
            args: Vec::new(),
        },
        cx.env().clone(),
    );

    let first = thunk.force(&mut cx, Demand::Value);
    assert!(matches!(first, Err(Error::Eval(message)) if message == "boom"));

    let second = thunk.force(&mut cx, Demand::Value).unwrap();
    assert_eq!(second.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}
