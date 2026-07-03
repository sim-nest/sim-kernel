//! The callable contract: objects that can be invoked with checked arguments.
//!
//! This is protocol the libraries implement; the kernel defines [`Callable`]
//! and the invocation surface, not concrete callable behavior.

use crate::{
    env::Cx,
    error::Result,
    object::{Args, Object, RawArgs, ShapeRef},
    value::Value,
};

/// Runtime protocol for values that can be called.
///
/// Functions, constructors, macros that accept raw expressions, and host or
/// guest function stubs all share this protocol. A callable is always a
/// runtime object, so it also has a class, display form, reflection table, and
/// optional protocol views through `Object`.
pub trait Callable: Object {
    /// Invoke the callable with already-evaluated, checked [`Args`].
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value>;

    /// Optional shape describing the accepted argument list, for browsing.
    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(None)
    }

    /// Optional shape describing the call result, for browsing.
    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(None)
    }

    /// Invoke the callable on raw, unevaluated argument expressions.
    ///
    /// The default evaluates each expression in `cx` and forwards to
    /// [`Callable::call`]; macro-like callables override to inspect raw forms.
    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let values = args
            .into_exprs()
            .into_iter()
            .map(|expr| cx.eval_expr(expr))
            .collect::<Result<Vec<_>>>()?;
        self.call(cx, Args::new(values))
    }
}
