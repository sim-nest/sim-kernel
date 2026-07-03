//! Evaluation policy and the macro-expander contract.
//!
//! The kernel defines the eval-policy, demand, thunk, and macro-expander
//! contracts plus the [`EvalFabric`] surface for location-transparent eval;
//! libraries supply the concrete evaluation and expansion strategies.

mod demand;
mod policy;
mod protocol;
mod runtime;
mod strict;
mod thunk;

pub use demand::{Demand, PreparedArgs};
pub use policy::{
    EagerPolicy, EvalPolicy, EvalPolicyRef, HybridPolicy, LazyPolicy, NeedPolicy, NoopEvalPolicy,
    StrictByShapePolicy,
};
pub use protocol::{
    Consistency, EvalFabric, EvalFabricRef, EvalMode, EvalReply, EvalRequest, MacroExpander,
    MacroExpanderRef, Phase,
};
pub use runtime::{eval_expr_default, force_default};
pub use strict::StrictNames;
pub use thunk::{LazyThunkObject, Thunk, ThunkObject};

#[cfg(test)]
mod tests;
