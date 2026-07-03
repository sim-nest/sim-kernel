use crate::{
    Symbol,
    env::Cx,
    error::{Error, Result},
    eval::{Demand, EvalPolicy, PreparedArgs},
    expr::Expr,
    object::RawArgs,
    value::Value,
};

use super::protocol::Phase;

/// Wraps any [`EvalPolicy`] so unbound names fail with precise errors.
///
/// The wrapped policy still controls argument strategy, forcing, macro
/// expansion, and expression evaluation. `StrictNames` changes only unbound
/// symbol and unbound call resolution.
pub struct StrictNames<P>(
    /// Policy that supplies all behavior except unbound-name resolution.
    pub P,
);

impl<P: EvalPolicy> EvalPolicy for StrictNames<P> {
    fn name(&self) -> &'static str {
        "strict-names"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        self.0.allow_macro_expansion(phase)
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        self.0.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        self.0.force(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        self.0.eval_expr(cx, expr)
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
