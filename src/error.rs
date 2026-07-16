//! The error and diagnostic contract for the kernel.
//!
//! Defines the [`enum@Error`] enum, the [`Result`] alias, and the [`Diagnostic`]
//! and [`Severity`] types that every kernel contract and library reports
//! against.

use thiserror::Error;

use crate::{
    capability::{CapabilityName, TrustLevel},
    expr::{SourceId, Span},
    id::{CaseId, ClassId, CodecId, FunctionId, ShapeId, Symbol},
    library::{ExportKind, Version},
};

/// The kernel result alias, defaulting the error type to [`enum@Error`].
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// A structured diagnostic message with optional source location.
///
/// The kernel defines the diagnostic record; libraries and contracts attach
/// diagnostics to errors to explain failures with source context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The severity level.
    pub severity: Severity,
    /// The human-readable message.
    pub message: String,
    /// The optional source the diagnostic refers to.
    pub source: Option<SourceId>,
    /// The optional span within the source.
    pub span: Option<Span>,
    /// The optional machine-readable diagnostic code.
    pub code: Option<Symbol>,
    /// Related sub-diagnostics providing more detail.
    pub related: Vec<Diagnostic>,
}

/// The severity level of a [`Diagnostic`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// Informational output.
    Info,
    /// A note attached to another diagnostic.
    Note,
}

impl Diagnostic {
    /// Builds an error-severity diagnostic with just a message.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            source: None,
            span: None,
            code: None,
            related: Vec::new(),
        }
    }

    /// Builds an info-severity diagnostic with just a message.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            message: message.into(),
            source: None,
            span: None,
            code: None,
            related: Vec::new(),
        }
    }

    /// Adds a machine-readable diagnostic code.
    pub fn with_code(mut self, code: Symbol) -> Self {
        self.code = Some(code);
        self
    }

    /// Adds a related sub-diagnostic.
    pub fn with_related(mut self, related: Diagnostic) -> Self {
        self.related.push(related);
        self
    }
}

/// The kernel error type reported by every contract and library.
///
/// The kernel defines the shared error vocabulary; libraries raise these
/// variants (and wrap their own messages in the string-carrying variants)
/// rather than inventing parallel error types.
#[derive(Clone, Debug, Error)]
pub enum Error {
    /// A symbol could not be resolved.
    #[error("unknown symbol {symbol}")]
    UnknownSymbol {
        /// The unresolved symbol.
        symbol: Symbol,
    },
    /// A class name could not be resolved.
    #[error("unknown class {class}")]
    UnknownClass {
        /// The unresolved class name.
        class: Symbol,
    },
    /// A class exists but does not provide a callable constructor.
    #[error("class {class} is not constructible")]
    NonConstructibleClass {
        /// The class that was called as a constructor.
        class: Symbol,
    },
    /// A function name could not be resolved.
    #[error("unknown function {function}")]
    UnknownFunction {
        /// The unresolved function name.
        function: Symbol,
    },
    /// No class is registered under the given id.
    #[error("missing class with id {0:?}")]
    MissingClass(ClassId),
    /// No shape is registered under the given id.
    #[error("missing shape with id {0:?}")]
    MissingShape(ShapeId),
    /// A shape reference could not be resolved.
    #[error("unresolved shape ref {reference:?}")]
    UnresolvedShapeRef {
        /// The unresolved shape reference.
        reference: Box<crate::Ref>,
    },
    /// A value's class did not match the expected class.
    #[error("wrong class: expected {expected:?}, found {found:?}")]
    WrongClass {
        /// The class that was required.
        expected: ClassId,
        /// The class actually found.
        found: ClassId,
    },
    /// A value did not match the expected shape.
    #[error("wrong shape: expected {expected:?}")]
    WrongShape {
        /// The shape that was required.
        expected: ShapeId,
        /// Diagnostics explaining the mismatch.
        diagnostics: Vec<Diagnostic>,
    },
    /// More than one overload matched a call.
    #[error("ambiguous overload for function {function:?}")]
    AmbiguousOverload {
        /// The function being dispatched.
        function: FunctionId,
        /// The competing candidate cases.
        candidates: Vec<CaseId>,
    },
    /// No overload matched a call.
    #[error("no matching overload for function {function:?}")]
    NoMatchingOverload {
        /// The function being dispatched.
        function: FunctionId,
        /// Diagnostics explaining why each case was rejected.
        diagnostics: Vec<Diagnostic>,
    },
    /// More than one numeric domain pairing matched an operator.
    #[error("ambiguous numeric dispatch for operator {operator}")]
    AmbiguousNumberDispatch {
        /// The operator being dispatched.
        operator: Symbol,
        /// The competing left/right domain pairings.
        candidates: Vec<(Symbol, Symbol)>,
    },
    /// No promotion path joined two number domains for an operator.
    #[error("no promotion path for operator {operator} from {left_domain} and {right_domain}")]
    NoPromotionPath {
        /// The operator being dispatched.
        operator: Symbol,
        /// The left operand's domain.
        left_domain: Symbol,
        /// The right operand's domain.
        right_domain: Symbol,
    },
    /// Promotion search exceeded its configured limits.
    #[error(
        "number promotion search from {from_domain} to {target_domain} exceeded limits \
         (max_depth={max_depth}, max_states={max_states})"
    )]
    PromotionSearchLimitExceeded {
        /// The domain the search started from.
        from_domain: Symbol,
        /// The domain the search was targeting.
        target_domain: Symbol,
        /// The depth limit that was hit.
        max_depth: usize,
        /// The state-count limit that was hit.
        max_states: usize,
    },
    /// A required capability was not granted.
    #[error("capability denied: {capability}")]
    CapabilityDenied {
        /// The denied capability.
        capability: CapabilityName,
    },
    /// A capability is not allowed at the caller's trust level.
    #[error("trust denied: {capability} is not allowed for {trust:?}")]
    TrustDenied {
        /// The requested capability.
        capability: CapabilityName,
        /// The caller's trust level.
        trust: TrustLevel,
    },
    /// A codec failed to read or write a form.
    #[error("codec error in {codec:?}: {message}")]
    CodecError {
        /// The codec that failed.
        codec: CodecId,
        /// The failure message.
        message: String,
    },
    /// A number (or other) domain reported a categorized failure.
    #[error("domain error in {domain} ({category}): {message}")]
    DomainError {
        /// The domain that failed.
        domain: Symbol,
        /// The error category within the domain.
        category: Symbol,
        /// The failure message.
        message: String,
    },
    /// Two exports of the same kind claimed the same symbol.
    #[error("duplicate export for {kind} {symbol}")]
    DuplicateExport {
        /// The export kind label.
        kind: &'static str,
        /// The conflicting symbol.
        symbol: Symbol,
    },
    /// Two libraries claimed the same name.
    #[error("duplicate lib {symbol}")]
    DuplicateLib {
        /// The conflicting library name.
        symbol: Symbol,
    },
    /// A catalog write conflicted with an existing key.
    #[error("catalog conflict in {table} for {key}")]
    CatalogConflict {
        /// The catalog table.
        table: Symbol,
        /// The conflicting key.
        key: Symbol,
    },
    /// A write targeted a read-only catalog table.
    #[error("catalog table {table} is read-only")]
    CatalogReadOnly {
        /// The read-only table.
        table: Symbol,
    },
    /// A catalog row violated its table schema.
    #[error("catalog schema error in {table}: {message}")]
    CatalogSchema {
        /// The table whose schema was violated.
        table: Symbol,
        /// The schema-violation message.
        message: String,
    },
    /// A library declared a dependency that is not loaded.
    #[error("missing dependency {dependency} for {lib}")]
    MissingDependency {
        /// The depending library.
        lib: Symbol,
        /// The missing dependency.
        dependency: Symbol,
    },
    /// A dependency is present but older than required.
    #[error(
        "dependency {dependency} for {lib} requires at least version {required:?} but loaded {loaded:?}"
    )]
    DependencyVersionMismatch {
        /// The depending library.
        lib: Symbol,
        /// The dependency name.
        dependency: Symbol,
        /// The minimum required version.
        required: Version,
        /// The version actually loaded.
        loaded: Version,
    },
    /// A cycle was found among library dependencies.
    #[error("cyclic lib dependency involving {symbol}")]
    CyclicDependency {
        /// A library on the cycle.
        symbol: Symbol,
    },
    /// A library cannot be unloaded because loaded libraries depend on it.
    #[error("cannot unload {lib}; loaded dependents remain: {dependents:?}")]
    LibHasDependents {
        /// The library requested for unload.
        lib: Symbol,
        /// Loaded libraries that depend on it.
        dependents: Vec<Symbol>,
    },
    /// An export record was produced without a matching manifest declaration.
    #[error("export record for {kind:?} {symbol} was not declared in the manifest")]
    UndeclaredExportRecord {
        /// The export kind.
        kind: ExportKind,
        /// The undeclared symbol.
        symbol: Symbol,
    },
    /// A value's static type did not match what was expected.
    #[error("type mismatch: expected {expected}, found {found}")]
    TypeMismatch {
        /// The expected type label.
        expected: &'static str,
        /// The type label actually found.
        found: &'static str,
    },
    /// Evaluation failed; carries a free-form message.
    #[error("evaluation error: {0}")]
    Eval(String),
    /// A library reported a free-form failure.
    #[error("lib error: {0}")]
    Lib(String),
    /// A lock was poisoned by a panic; carries the lock name.
    #[error("poisoned lock: {0}")]
    PoisonedLock(&'static str),
    /// A thunk was forced while already being forced.
    #[error("recursive thunk force detected")]
    RecursiveThunkForce,
    /// The host environment reported a free-form failure.
    #[error("host error: {0}")]
    HostError(String),
}

impl Error {
    /// Builds a [`Error::DomainError`] from its parts.
    pub fn domain_error(domain: Symbol, category: Symbol, message: impl Into<String>) -> Self {
        Self::DomainError {
            domain,
            category,
            message: message.into(),
        }
    }

    /// Wraps a host [`std::io::Error`] into an [`Error::HostError`].
    ///
    /// This is a named constructor, deliberately not a blanket `From` impl, so
    /// that every host-IO-to-kernel conversion stays explicit at the call site.
    pub fn host_io(err: std::io::Error) -> Self {
        Self::HostError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn host_io_wraps_an_io_error_into_host_error() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let error = Error::host_io(io);
        match error {
            Error::HostError(message) => assert_eq!(message, "no such file"),
            other => panic!("expected HostError, got {other:?}"),
        }
    }
}
