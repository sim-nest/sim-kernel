use std::sync::Arc;

use crate::{
    Symbol,
    env::Cx,
    error::{Error, Result},
    eval::{Demand, LazyThunkObject, PreparedArgs, ThunkObject},
    expr::Expr,
    object::RawArgs,
    value::Value,
};

use super::protocol::Phase;

/// The eval-policy contract: how a call site evaluates its arguments and body.
///
/// An eval policy decides argument strategy (eager, lazy, by-need, or strict
/// by [`Demand`]), whether macro expansion is permitted in a given
/// [`Phase`], how a value is [`force`](EvalPolicy::force)d to satisfy a
/// demand, and how a bare [`Expr`] is evaluated. The kernel ships several
/// reference policies ([`EagerPolicy`], [`LazyPolicy`], [`NeedPolicy`],
/// [`StrictByShapePolicy`], [`HybridPolicy`], [`NoopEvalPolicy`]); libraries
/// may supply their own. Concrete language semantics stay out of the kernel.
pub trait EvalPolicy: Send + Sync {
    /// A stable, human-readable name for this policy.
    fn name(&self) -> &'static str;
    /// Whether macro expansion is permitted in the given [`Phase`].
    ///
    /// Defaults to `false` (no expansion in any phase).
    fn allow_macro_expansion(&self, _phase: Phase) -> bool {
        false
    }
    /// Prepares raw call arguments into [`PreparedArgs`] per the demands.
    ///
    /// The per-position `demands` slice states how strongly each argument
    /// must be forced; positions beyond its length take a policy default.
    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs>;
    /// Forces `value` far enough to satisfy `demand`.
    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value>;
    /// Evaluates a bare expression to a value under this policy.
    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value>;
    /// Resolve a symbol that bound to nothing in value position.
    ///
    /// The default keeps it as data, which makes eval total and fits the
    /// symbolic substrate. A strict language policy can override this to fail.
    fn resolve_unbound_symbol(&self, cx: &mut Cx, symbol: Symbol) -> Result<Value> {
        cx.factory().symbol(symbol)
    }
    /// Resolve a call whose operator bound to nothing.
    ///
    /// The default keeps the unevaluated application as data, matching the
    /// symbolic substrate. A strict language policy can override this to fail.
    fn resolve_unbound_call(
        &self,
        cx: &mut Cx,
        operator: Symbol,
        args: Vec<Expr>,
    ) -> Result<Value> {
        cx.factory().expr(Expr::Call {
            operator: Box::new(Expr::Symbol(operator)),
            args,
        })
    }
}

/// A shared, reference-counted handle to an [`EvalPolicy`].
pub type EvalPolicyRef = Arc<dyn EvalPolicy>;

/// A policy that quotes every argument and refuses to evaluate expressions.
///
/// Useful as a placeholder where no real evaluation strategy is installed:
/// arguments are passed through as quoted expression values and
/// [`eval_expr`](EvalPolicy::eval_expr) returns an error.
pub struct NoopEvalPolicy;

impl EvalPolicy for NoopEvalPolicy {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        _demands: &[Demand],
    ) -> Result<PreparedArgs> {
        let values = raw
            .into_exprs()
            .into_iter()
            .map(|expr| cx.factory().expr(expr))
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedArgs::new(values))
    }

    fn force(&self, _cx: &mut Cx, value: Value, _demand: Demand) -> Result<Value> {
        Ok(value)
    }

    fn eval_expr(&self, _cx: &mut Cx, _expr: Expr) -> Result<Value> {
        Err(Error::Eval(
            "noop eval policy cannot evaluate expressions".to_owned(),
        ))
    }
}

/// A policy that fully evaluates every argument before the call.
pub struct EagerPolicy;

impl EvalPolicy for EagerPolicy {
    fn name(&self) -> &'static str {
        "eager"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        matches!(
            phase,
            Phase::Read | Phase::Expand | Phase::Compile | Phase::Eval
        )
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        _demands: &[Demand],
    ) -> Result<PreparedArgs> {
        let values = raw
            .into_exprs()
            .into_iter()
            .map(|expr| cx.eval_expr(expr))
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedArgs::new(values))
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        crate::eval::force_default(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        crate::eval::eval_expr_default(cx, expr)
    }
}

/// A policy that wraps every argument in a re-forcing lazy thunk.
///
/// Each argument becomes a [`LazyThunkObject`] that re-evaluates whenever it
/// is forced; results are not cached between forces.
pub struct LazyPolicy;

impl EvalPolicy for LazyPolicy {
    fn name(&self) -> &'static str {
        "lazy"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        matches!(
            phase,
            Phase::Read | Phase::Expand | Phase::Compile | Phase::Eval
        )
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        _demands: &[Demand],
    ) -> Result<PreparedArgs> {
        lazy_args(cx, raw)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        crate::eval::force_default(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        crate::eval::eval_expr_default(cx, expr)
    }
}

/// A call-by-need policy: arguments are thunked and forced at most once.
///
/// Each argument becomes a memoizing [`ThunkObject`] whose value is computed
/// on first force and cached for later uses.
pub struct NeedPolicy;

impl EvalPolicy for NeedPolicy {
    fn name(&self) -> &'static str {
        "lazy-by-need"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        matches!(
            phase,
            Phase::Read | Phase::Expand | Phase::Compile | Phase::Eval
        )
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        _demands: &[Demand],
    ) -> Result<PreparedArgs> {
        need_args(cx, raw)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        crate::eval::force_default(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        crate::eval::eval_expr_default(cx, expr)
    }
}

/// A policy that forces or defers each argument by its [`Demand`].
///
/// Positions demanding a value are evaluated eagerly; positions demanding an
/// expression (or nothing) become lazy thunks.
pub struct StrictByShapePolicy;

impl EvalPolicy for StrictByShapePolicy {
    fn name(&self) -> &'static str {
        "strict-by-shape"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        matches!(
            phase,
            Phase::Read | Phase::Expand | Phase::Compile | Phase::Eval
        )
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        let env = cx.env().clone();
        let values = raw
            .into_exprs()
            .into_iter()
            .enumerate()
            .map(|(index, expr)| {
                let demand = demands.get(index).copied().unwrap_or(Demand::Value);
                match demand {
                    Demand::Never | Demand::Expr => Ok(Value::from_arc(Arc::new(
                        LazyThunkObject::new(expr, env.clone()),
                    ))),
                    Demand::Value | Demand::Bool | Demand::Class(_) | Demand::Shape(_) => {
                        cx.eval_expr(expr)
                    }
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedArgs::new(values))
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        crate::eval::force_default(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        crate::eval::eval_expr_default(cx, expr)
    }
}

/// A policy that mixes eager and quoted arguments by their [`Demand`].
///
/// Like [`StrictByShapePolicy`], but expression-demanding positions are kept
/// as quoted expression values rather than lazy thunks.
pub struct HybridPolicy;

impl EvalPolicy for HybridPolicy {
    fn name(&self) -> &'static str {
        "hybrid"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        matches!(
            phase,
            Phase::Read | Phase::Expand | Phase::Compile | Phase::Eval
        )
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        let values = raw
            .into_exprs()
            .into_iter()
            .enumerate()
            .map(|(index, expr)| {
                let demand = demands.get(index).copied().unwrap_or(Demand::Value);
                match demand {
                    Demand::Never | Demand::Expr => cx.factory().expr(expr),
                    Demand::Value | Demand::Bool | Demand::Class(_) | Demand::Shape(_) => {
                        cx.eval_expr(expr)
                    }
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedArgs::new(values))
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        crate::eval::force_default(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        crate::eval::eval_expr_default(cx, expr)
    }
}

fn lazy_args(cx: &mut Cx, raw: RawArgs) -> Result<PreparedArgs> {
    let env = cx.env().clone();
    let values = raw
        .into_exprs()
        .into_iter()
        .map(|expr| {
            Ok(Value::from_arc(Arc::new(LazyThunkObject::new(
                expr,
                env.clone(),
            ))))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PreparedArgs::new(values))
}

fn need_args(cx: &mut Cx, raw: RawArgs) -> Result<PreparedArgs> {
    let env = cx.env().clone();
    let values = raw
        .into_exprs()
        .into_iter()
        .map(|expr| {
            Ok(Value::from_arc(Arc::new(ThunkObject::new(
                expr,
                env.clone(),
            ))))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PreparedArgs::new(values))
}
