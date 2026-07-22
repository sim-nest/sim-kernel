//! The evaluation environment: [`Cx`], [`Env`], capabilities, and diagnostics.
//!
//! This is the context threaded through every checked call -- the registry
//! handle, capability set, and diagnostic sink the kernel contracts operate
//! against; libraries supply the behavior reached through it.

mod base;
mod core;
mod fork;
mod load_ledger;
mod numbers;
mod promotion;
mod selection;

pub use base::{Diagnostics, Env};
pub use core::{Capabilities, Cx};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
