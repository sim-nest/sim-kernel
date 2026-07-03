//! The [`Stream`] contract: an object that yields values until closed.
//!
//! This is protocol the libraries implement; the kernel defines the stream
//! trait, not the concrete transport behind any stream.

use crate::{env::Cx, error::Result, object::Object, value::Value};

/// A runtime object that yields values one at a time until it is closed.
pub trait Stream: Object {
    /// Pulls the next value, or `Ok(None)` once the stream is exhausted.
    fn next(&self, cx: &mut Cx) -> Result<Option<Value>>;

    /// Releases any resources backing the stream; idempotent by default.
    fn close(&self, _cx: &mut Cx) -> Result<()> {
        Ok(())
    }
}
