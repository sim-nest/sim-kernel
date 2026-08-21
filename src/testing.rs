//! Shared test-context constructors.
//!
//! Use these helpers instead of hand-rolling local `Cx` constructors in tests.
//! The two constructors cover structure-only tests and eager-evaluation tests;
//! a test module can route through them with
//! `use sim_kernel::testing::bare_cx as cx;`.

use std::sync::Arc;

use crate::{Cx, DefaultFactory, EagerPolicy, NoopEvalPolicy};

/// A bare evaluation context: the no-op eval policy over the default factory.
///
/// Use for tests that exercise structure without driving evaluation.
pub fn bare_cx() -> Cx {
    Cx::new(
        Arc::new(NoopEvalPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(0x5445_5354),
    )
}

/// An eager evaluation context: the eager eval policy over the default factory.
///
/// Use for tests that evaluate forms to completion.
pub fn eager_cx() -> Cx {
    Cx::new(
        Arc::new(EagerPolicy),
        Arc::new(DefaultFactory),
        crate::HandleSeed::new(0x5445_5354),
    )
}
