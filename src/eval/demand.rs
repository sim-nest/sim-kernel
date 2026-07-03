use crate::{
    env::Cx,
    expr::Expr,
    id::{ClassId, ShapeId},
    value::Value,
};

/// How strongly an argument position demands its value be forced.
///
/// A [`Demand`] is the contract an [`EvalPolicy`](crate::EvalPolicy) consults
/// when preparing call arguments: it states how far a value must be driven
/// before the callable sees it, from "leave it as an unevaluated expression"
/// to "force it to a value of a specific class or shape". The kernel only
/// defines the demand vocabulary; libraries decide how to satisfy each variant.
///
/// # Examples
///
/// ```
/// use sim_kernel::eval::Demand;
///
/// // The vocabulary is plain, copyable data.
/// let demand = Demand::Value;
/// assert_eq!(demand, Demand::Value);
/// assert_ne!(Demand::Value, Demand::Expr);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demand {
    /// Never force; the argument is passed through untouched.
    Never,
    /// Force to an expression form (leave it quoted, do not evaluate).
    Expr,
    /// Force to a fully evaluated value.
    Value,
    /// Force to a value and coerce to its boolean truth.
    Bool,
    /// Force to a value required to be of the given class.
    Class(ClassId),
    /// Force to a value required to match the given shape.
    Shape(ShapeId),
}

/// A positional tuple of prepared call arguments.
///
/// Produced by [`EvalPolicy::prepare_call_args`](crate::EvalPolicy::prepare_call_args),
/// it carries the values (eager, thunked, or quoted, per the policy and the
/// per-position [`Demand`]) that a callable receives.
///
/// # Examples
///
/// ```
/// use sim_kernel::eval::PreparedArgs;
///
/// let args = PreparedArgs::new(Vec::new());
/// assert!(args.is_empty());
/// assert_eq!(args.len(), 0);
/// assert!(args.get(0).is_none());
/// ```
#[derive(Clone)]
pub struct PreparedArgs {
    values: Vec<Value>,
}

impl PreparedArgs {
    /// Wraps an ordered list of prepared argument values.
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Returns the prepared values in positional order.
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// Returns the value at `index`, or `None` if out of range.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    /// Returns the number of prepared arguments.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` when there are no prepared arguments.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Reconstructs the arguments as a single list [`Expr`] of their forms.
    pub fn as_expr_tuple(&self, cx: &mut Cx) -> crate::error::Result<Expr> {
        Ok(Expr::List(
            self.values
                .iter()
                .map(|value| value.object().as_expr(cx))
                .collect::<crate::error::Result<Vec<_>>>()?,
        ))
    }
}
