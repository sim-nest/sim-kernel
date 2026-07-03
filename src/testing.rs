//! Shared test-context constructors.
//!
//! Almost every crate's test module hand-rolled the same `fn cx() -> Cx`. These
//! are the two canonical shapes that duplication collapsed to (OVERLAP6.16);
//! a test helper routes here with `use sim_kernel::testing::bare_cx as cx;`.

use std::sync::Arc;

use crate::{Cx, DefaultFactory, EagerPolicy, NoopEvalPolicy};

/// A bare evaluation context: the no-op eval policy over the default factory.
///
/// Use for tests that exercise structure without driving evaluation.
pub fn bare_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

/// An eager evaluation context: the eager eval policy over the default factory.
///
/// Use for tests that evaluate forms to completion.
pub fn eager_cx() -> Cx {
    Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}
