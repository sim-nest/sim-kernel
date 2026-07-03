use std::sync::Arc;
use std::time::Duration;

use crate::{
    capability::CapabilityName,
    env::Cx,
    error::{Diagnostic, Result, Severity},
    expr::Expr,
    hint::{HintMetadata, diagnostic_hints_value},
    id::{CORE_EVAL_REPLY_CLASS_ID, CORE_EVAL_REQUEST_CLASS_ID, ClassId, Symbol},
    object::{ClassRef, Object, ShapeRef},
    value::Value,
};

/// A stage in the pipeline at which macro expansion may run.
///
/// Eval policies consult the phase via
/// [`EvalPolicy::allow_macro_expansion`](crate::EvalPolicy::allow_macro_expansion),
/// and expanders receive it in [`MacroExpander::expand_expr`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// While reading source forms into expressions.
    Read,
    /// During the dedicated macro-expansion pass.
    Expand,
    /// While compiling expanded expressions.
    Compile,
    /// During evaluation itself.
    Eval,
}

/// The macro-expander contract: rewrite an [`Expr`] in a given [`Phase`].
///
/// The kernel defines only the expansion hook; concrete macro systems and
/// expansion strategies are supplied by libraries.
pub trait MacroExpander: Send + Sync {
    /// Rewrites `expr` for the given `phase`, returning the expanded form.
    fn expand_expr(&self, cx: &mut Cx, phase: Phase, expr: Expr) -> Result<Expr>;
}

/// A shared, reference-counted handle to a [`MacroExpander`].
pub type MacroExpanderRef = Arc<dyn MacroExpander>;

/// Where a [`realize`](crate::realize) request may be answered from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Consistency {
    /// Answer only from the local node.
    LocalOnly,
    /// Prefer the local node, falling back to remote (the default).
    #[default]
    LocalFirst,
    /// Answer only from a remote node.
    RemoteOnly,
}

impl Consistency {
    /// Returns the canonical symbol naming this consistency mode.
    pub fn as_symbol(self) -> Symbol {
        Symbol::new(match self {
            Self::LocalOnly => "local-only",
            Self::LocalFirst => "local-first",
            Self::RemoteOnly => "remote-only",
        })
    }
}

/// Which evaluation discipline a request runs under.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EvalMode {
    /// Ordinary expression evaluation (the default).
    #[default]
    Eval,
    /// Relational/logic evaluation that may yield multiple answers.
    Logic,
}

impl EvalMode {
    /// Returns the canonical symbol naming this evaluation mode.
    pub fn as_symbol(self) -> Symbol {
        Symbol::new(match self {
            Self::Eval => "eval",
            Self::Logic => "logic",
        })
    }
}

// sim-non-citizen(reason = "kernel eval protocol projection; public transport descriptors live in server/Frame and agent-runner/ModelRequest", kind = "marker", descriptor = "")
/// A request submitted to an [`EvalFabric`] for location-transparent eval.
///
/// This is the kernel's protocol projection of an eval request; public
/// transport descriptors live in server and agent-runner crates. See the
/// README section "Distributed evaluation".
#[derive(Clone)]
pub struct EvalRequest {
    /// The expression to evaluate.
    pub expr: Expr,
    /// Optional shape the result must satisfy.
    pub result_shape: Option<ShapeRef>,
    /// Capabilities the evaluation requires.
    pub required_capabilities: Vec<CapabilityName>,
    /// Optional wall-clock deadline for the evaluation.
    pub deadline: Option<Duration>,
    /// Where the request may be answered from.
    pub consistency: Consistency,
    /// Which evaluation discipline to run under.
    pub mode: EvalMode,
    /// Optional cap on the number of answers (logic mode).
    pub answer_limit: Option<usize>,
    /// Optional buffer size for streamed events.
    pub stream_buffer: Option<usize>,
    /// Whether to stream intermediate events rather than only the final value.
    pub stream: bool,
    /// Whether to collect and return an evaluation trace.
    pub trace: bool,
}

// sim-non-citizen(reason = "kernel eval protocol projection; public transport descriptors live in server/Frame and agent-runner/ModelResponse", kind = "marker", descriptor = "")
/// The answer returned by an [`EvalFabric`] for an [`EvalRequest`].
#[derive(Clone)]
pub struct EvalReply {
    /// The evaluated result value.
    pub value: Value,
    /// Diagnostics produced during evaluation.
    pub diagnostics: Vec<Diagnostic>,
    /// Optional evaluation trace, when [`EvalRequest::trace`] was set.
    pub trace: Option<Value>,
}

/// The location-transparent distributed eval contract.
///
/// An [`EvalFabric`] answers an [`EvalRequest`] with an [`EvalReply`] without
/// the caller knowing whether evaluation is local or remote. Server and agent
/// code target this surface (and the [`realize`](crate::realize) helpers built
/// on it) instead of transport-specific APIs; libraries supply the concrete
/// transports. See the README section "Distributed evaluation".
pub trait EvalFabric: Send + Sync {
    /// Evaluates `request` and returns its reply.
    fn realize(&self, cx: &mut Cx, request: EvalRequest) -> Result<EvalReply>;
}

/// A shared, reference-counted handle to an [`EvalFabric`].
pub type EvalFabricRef = Arc<dyn EvalFabric>;

impl Object for EvalRequest {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<EvalRequest>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EvalRequest {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        runtime_class(
            cx,
            CORE_EVAL_REQUEST_CLASS_ID,
            Symbol::qualified("core", "EvalRequest"),
        )
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table(cx)?.object().as_expr(cx)
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let result_shape = match &self.result_shape {
            Some(shape) => shape.clone(),
            None => cx.factory().nil()?,
        };
        let deadline = match self.deadline {
            Some(deadline) => cx.factory().string(format_duration(deadline))?,
            None => cx.factory().nil()?,
        };
        let required_capabilities = cx.factory().list(
            self.required_capabilities
                .iter()
                .map(|capability| cx.factory().string(capability.as_str().to_owned()))
                .collect::<Result<Vec<_>>>()?,
        )?;
        cx.factory().table(vec![
            (Symbol::new("expr"), cx.factory().expr(self.expr.clone())?),
            (Symbol::new("result-shape"), result_shape),
            (Symbol::new("requires"), required_capabilities),
            (Symbol::new("deadline"), deadline),
            (
                Symbol::new("consistency"),
                cx.factory().symbol(self.consistency.as_symbol())?,
            ),
            (
                Symbol::new("mode"),
                cx.factory().symbol(self.mode.as_symbol())?,
            ),
            (
                Symbol::new("answer-limit"),
                match self.answer_limit {
                    Some(limit) => cx.factory().string(limit.to_string())?,
                    None => cx.factory().nil()?,
                },
            ),
            (
                Symbol::new("stream-buffer"),
                match self.stream_buffer {
                    Some(limit) => cx.factory().string(limit.to_string())?,
                    None => cx.factory().nil()?,
                },
            ),
            (Symbol::new("stream"), cx.factory().bool(self.stream)?),
            (Symbol::new("trace"), cx.factory().bool(self.trace)?),
        ])
    }
}

impl Object for EvalReply {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<EvalReply>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EvalReply {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        runtime_class(
            cx,
            CORE_EVAL_REPLY_CLASS_ID,
            Symbol::qualified("core", "EvalReply"),
        )
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table(cx)?.object().as_expr(cx)
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let trace = match &self.trace {
            Some(trace) => trace.clone(),
            None => cx.factory().nil()?,
        };
        let diagnostic_values = self
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic_value(cx, diagnostic))
            .collect::<Result<Vec<_>>>()?;
        let diagnostics = cx.factory().list(diagnostic_values)?;
        cx.factory().table(vec![
            (Symbol::new("value"), self.value.clone()),
            (Symbol::new("diagnostics"), diagnostics),
            (Symbol::new("trace"), trace),
        ])
    }
}

fn runtime_class(cx: &mut Cx, id: ClassId, symbol: Symbol) -> Result<ClassRef> {
    if let Some(value) = cx.registry().class_by_symbol(&symbol) {
        return Ok(value.clone());
    }
    cx.factory().class_stub(id, symbol)
}

fn format_duration(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 && duration.as_secs() > 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn diagnostic_value(cx: &mut Cx, diagnostic: &Diagnostic) -> Result<Value> {
    let hints = diagnostic_hints_value(cx, diagnostic)?;
    let severity = cx.factory().symbol(Symbol::new(match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Note => "note",
    }))?;
    let source = match &diagnostic.source {
        Some(source) => cx.factory().string(source.0.to_string())?,
        None => cx.factory().nil()?,
    };
    let span = match &diagnostic.span {
        Some(span) => cx.factory().table(vec![
            (
                Symbol::new("start"),
                cx.factory().string(span.start.to_string())?,
            ),
            (
                Symbol::new("end"),
                cx.factory().string(span.end.to_string())?,
            ),
        ])?,
        None => cx.factory().nil()?,
    };
    let code = match &diagnostic.code {
        Some(code) => cx.factory().symbol(code.clone())?,
        None => cx.factory().nil()?,
    };
    let related_values = diagnostic
        .related
        .iter()
        .filter(|related| !HintMetadata::is_hint_diagnostic(related))
        .map(|related| diagnostic_value(cx, related))
        .collect::<Result<Vec<_>>>()?;
    let related = cx.factory().list(related_values)?;
    cx.factory().table(vec![
        (Symbol::new("severity"), severity),
        (
            Symbol::new("message"),
            cx.factory().string(diagnostic.message.clone())?,
        ),
        (Symbol::new("source"), source),
        (Symbol::new("span"), span),
        (Symbol::new("code"), code),
        (Symbol::new("related"), related),
        (Symbol::new("hints"), hints),
    ])
}
