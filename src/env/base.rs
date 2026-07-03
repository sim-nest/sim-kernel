use std::{collections::BTreeMap, sync::Arc};

use crate::{error::Diagnostic, id::Symbol, value::Value};

/// A lexical environment: a frame of symbol bindings chained to a parent.
///
/// Lookups walk from the local frame outward through parent frames. The active
/// `Env` is held by the [`Cx`](crate::Cx) and swapped during nested scopes.
#[derive(Clone, Debug, Default)]
pub struct Env {
    frame: BTreeMap<Symbol, Value>,
    parent: Option<Arc<Env>>,
}

impl Env {
    /// Builds a child environment with an empty frame over `parent`.
    pub fn child(parent: Arc<Env>) -> Self {
        Self {
            frame: BTreeMap::new(),
            parent: Some(parent),
        }
    }

    /// Binds a name in the local frame, returning any previous binding.
    pub fn define(&mut self, name: Symbol, value: Value) -> Option<Value> {
        self.frame.insert(name, value)
    }

    /// Looks up a name, walking outward to parent frames.
    pub fn get(&self, name: &Symbol) -> Option<Value> {
        self.frame
            .get(name)
            .cloned()
            .or_else(|| self.parent.as_ref().and_then(|parent| parent.get(name)))
    }
}

/// A collected sink of [`Diagnostic`]s accumulated during a checked call.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    messages: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Pushes an error-level diagnostic from a message.
    pub fn push(&mut self, message: impl Into<String>) {
        self.messages.push(Diagnostic::error(message));
    }

    /// Pushes an already-built diagnostic.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.messages.push(diagnostic);
    }

    /// Pushes an info-level diagnostic from a message.
    pub fn push_info(&mut self, message: impl Into<String>) {
        self.messages.push(Diagnostic::info(message));
    }

    /// Returns the accumulated diagnostics.
    pub fn messages(&self) -> &[Diagnostic] {
        &self.messages
    }

    /// Drains and returns the accumulated diagnostics.
    pub fn take(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.messages)
    }
}
