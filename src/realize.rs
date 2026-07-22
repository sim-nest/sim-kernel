//! The `realize` surface: the location-transparent distributed eval contract.
//!
//! The kernel defines the realize request, observe modes, and event-draining
//! contract that server and agent code target instead of transport-specific
//! APIs; libraries supply the concrete transports behind it.

use std::{collections::VecDeque, sync::Mutex, time::Duration};

use crate::{
    capability::CapabilityName,
    env::Cx,
    error::{Error, Result},
    eval::{Consistency, EvalFabric, EvalMode, EvalReply, EvalRequest},
    event::{Event, EventKind, EventSource},
    event_ledger::EventLedger,
    expr::Expr,
    handle_store::HandleStore,
    id::{CORE_SEQUENCE_CLASS_ID, Symbol},
    object::{ClassRef, Object, ShapeRef},
    ref_id::{HandleId, Ref},
    ref_resolver::{RefResolver, ResolvedRef, TemporaryRefResolver, value_from_ref},
    seq::{Sequence, SequenceItem, sequence_item_from_event},
    shape_check::check_shape_value,
    term::Term,
    value::Value,
};

/// How much of an evaluation a caller wants to observe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObserveMode {
    /// Observe only the final value (the default).
    #[default]
    FinalOnly,
    /// Stream intermediate events as they occur.
    Events,
    /// Collect the full event ledger alongside the final value.
    Ledger,
}

/// A reference-based realize request: the portable form of an [`EvalRequest`].
///
/// Where [`EvalRequest`] carries an in-process [`Expr`] and live values, a
/// [`RealizeRequest`] carries a [`Term`] and [`Ref`]s, so it can cross a
/// transport boundary. The two convert via
/// [`from_eval_request`](RealizeRequest::from_eval_request) and
/// [`to_eval_request`](RealizeRequest::to_eval_request). See the README
/// section "Distributed evaluation".
///
/// # Examples
///
/// ```
/// use sim_kernel::realize::{ObserveMode, RealizeRequest};
/// use sim_kernel::term::Term;
/// use sim_kernel::Symbol;
///
/// let request = RealizeRequest::new(Term::Local(Symbol::new("x")))
///     .observing(ObserveMode::Events);
/// assert_eq!(request.observe, ObserveMode::Events);
/// assert!(request.required_capabilities.is_empty());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RealizeRequest {
    /// The term to evaluate.
    pub term: Term,
    /// Optional reference to a shape the result must satisfy.
    pub result_shape: Option<Ref>,
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
    pub buffer_limit: Option<usize>,
    /// How much of the evaluation to observe.
    pub observe: ObserveMode,
}

impl RealizeRequest {
    /// Creates a request to realize `term` with all defaults.
    pub fn new(term: Term) -> Self {
        Self {
            term,
            result_shape: None,
            required_capabilities: Vec::new(),
            deadline: None,
            consistency: Consistency::default(),
            mode: EvalMode::default(),
            answer_limit: None,
            buffer_limit: None,
            observe: ObserveMode::default(),
        }
    }

    /// Sets the [`ObserveMode`], returning the request (builder style).
    pub fn observing(mut self, observe: ObserveMode) -> Self {
        self.observe = observe;
        self
    }

    /// Adds a required capability, returning the request (builder style).
    pub fn requiring(mut self, capability: CapabilityName) -> Self {
        self.required_capabilities.push(capability);
        self
    }

    /// Builds a [`RealizeRequest`] from an in-process [`EvalRequest`].
    ///
    /// Live values become [`Ref`]s and the [`Expr`] becomes a [`Term`] so the
    /// request can cross a transport boundary.
    pub fn from_eval_request(cx: &mut Cx, request: &EvalRequest) -> Result<Self> {
        Ok(Self {
            term: term_from_eval_expr(cx, &request.expr)?,
            result_shape: request
                .result_shape
                .as_ref()
                .map(|shape| handle_ref_for_value(cx, shape))
                .transpose()?,
            required_capabilities: request.required_capabilities.clone(),
            deadline: request.deadline,
            consistency: request.consistency,
            mode: request.mode,
            answer_limit: request.answer_limit,
            buffer_limit: request.stream_buffer,
            observe: observe_from_eval_request(request),
        })
    }

    /// Resolves this request back into an in-process [`EvalRequest`].
    ///
    /// The inverse of [`from_eval_request`](RealizeRequest::from_eval_request):
    /// [`Term`] and [`Ref`]s are resolved against `cx` into an [`Expr`] and
    /// live values.
    pub fn to_eval_request(&self, cx: &mut Cx) -> Result<EvalRequest> {
        Ok(EvalRequest {
            expr: eval_expr_from_term(cx, &self.term)?,
            result_shape: self
                .result_shape
                .as_ref()
                .map(|reference| shape_from_ref(cx, reference))
                .transpose()?,
            required_capabilities: self.required_capabilities.clone(),
            deadline: self.deadline,
            consistency: self.consistency,
            mode: self.mode,
            answer_limit: self.answer_limit,
            stream_buffer: self.buffer_limit,
            stream: self.observe == ObserveMode::Events,
            trace: self.observe != ObserveMode::FinalOnly,
        })
    }
}

// sim-non-citizen(reason = "buffered event-source handle; descriptor data is the event sequence payload", kind = "handle", descriptor = "core/EventSource")
/// An in-memory [`EventSource`] backed by a buffered queue of events.
///
/// Returned by [`realize_events`] to replay a completed evaluation's events
/// (diagnostics, trace, final value, done) without a live transport.
#[derive(Debug)]
pub struct BufferedEventSource {
    events: Mutex<VecDeque<Event>>,
}

impl BufferedEventSource {
    /// Wraps an ordered list of events as a drainable source.
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            events: Mutex::new(events.into()),
        }
    }
}

impl EventSource for BufferedEventSource {
    fn next(&self, _cx: &mut Cx) -> Result<Option<Event>> {
        Ok(self
            .events
            .lock()
            .map_err(|_| Error::PoisonedLock("event source"))?
            .pop_front())
    }

    fn close(&self, _cx: &mut Cx) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| Error::PoisonedLock("event source"))?
            .clear();
        Ok(())
    }
}

impl Object for BufferedEventSource {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<event-source>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for BufferedEventSource {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            CORE_SEQUENCE_CLASS_ID,
            Symbol::qualified("core", "EventSource"),
        )
    }
    fn as_sequence(&self) -> Option<&dyn Sequence> {
        Some(self)
    }
}

impl Sequence for BufferedEventSource {
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        while let Some(event) = EventSource::next(self, cx)? {
            let done = matches!(event.kind, EventKind::Done);
            if let Some(item) = sequence_item_from_event(cx, event)? {
                return Ok(Some(item));
            }
            if done {
                return Ok(None);
            }
        }
        Ok(None)
    }

    fn close(&self, cx: &mut Cx) -> Result<()> {
        EventSource::close(self, cx)
    }

    fn peek_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        let events = self
            .events
            .lock()
            .map_err(|_| Error::PoisonedLock("event source"))?
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for event in events {
            let done = matches!(event.kind, EventKind::Done);
            if let Some(item) = sequence_item_from_event(cx, event)? {
                return Ok(Some(item));
            }
            if done {
                return Ok(None);
            }
        }
        Ok(None)
    }

    fn is_done(&self, _cx: &mut Cx) -> Result<bool> {
        let events = self
            .events
            .lock()
            .map_err(|_| Error::PoisonedLock("event source"))?;
        for event in events.iter() {
            match event.kind {
                EventKind::Chunk { .. } | EventKind::Final(_) | EventKind::Failed(_) => {
                    return Ok(false);
                }
                EventKind::Done => return Ok(true),
                _ => {}
            }
        }
        Ok(true)
    }
}

/// Realizes `request` against `target` and returns its events as a source.
///
/// Runs the evaluation through the [`EvalFabric`], records the resulting
/// diagnostics, optional trace, final value, and a terminating `done` into an
/// event ledger, and hands them back as a [`BufferedEventSource`].
pub fn realize_events(
    cx: &mut Cx,
    target: &dyn EvalFabric,
    request: EvalRequest,
) -> Result<BufferedEventSource> {
    let eventful_request = RealizeRequest::from_eval_request(cx, &request)?;
    let request_ref = ref_for_realize_request(cx, &eventful_request)?;
    let run = Ref::Handle(HandleId::fresh());
    let mut ledger = EventLedger::new();
    ledger.started(run.clone(), request_ref)?;

    let result_shape = request.result_shape.clone();
    let reply = target.realize(cx, request)?;
    if let Some(shape) = result_shape {
        check_shape_value(cx, &shape, None, reply.value.clone())?;
    }
    for diagnostic in &reply.diagnostics {
        ledger.push(run.clone(), EventKind::Diagnostic(diagnostic.clone()))?;
    }
    if let Some(trace) = &reply.trace {
        ledger.push(
            run.clone(),
            EventKind::Trace(handle_ref_for_value(cx, trace)?),
        )?;
    }
    ledger.final_value(run.clone(), handle_ref_for_value(cx, &reply.value)?)?;
    ledger.done(run.clone())?;

    Ok(BufferedEventSource::new(
        ledger.events_for_run(&run).to_vec(),
    ))
}

/// Realizes `request` against `target` and collects only the final reply.
///
/// A convenience over [`realize_events`] plus [`drain_events_to_reply`] for
/// callers that want the [`EvalReply`] rather than the event stream.
pub fn realize_final(
    cx: &mut Cx,
    target: &dyn EvalFabric,
    request: EvalRequest,
) -> Result<EvalReply> {
    let events = realize_events(cx, target, request)?;
    drain_events_to_reply(cx, &events)
}

/// Drains an [`EventSource`] into a single [`EvalReply`].
///
/// Accumulates diagnostics and an optional trace, captures the final value,
/// and stops at `done`. A `Failed` event is turned into an error, and a
/// stream that ends without a final value is an error.
pub fn drain_events_to_reply(cx: &mut Cx, source: &dyn EventSource) -> Result<EvalReply> {
    let mut diagnostics = Vec::new();
    let mut trace = None;
    let mut value = None;

    while let Some(event) = source.next(cx)? {
        match event.kind {
            EventKind::Diagnostic(diagnostic) => diagnostics.push(diagnostic),
            EventKind::Trace(reference) => trace = Some(value_from_ref(cx, &reference)?),
            EventKind::Final(reference) => value = Some(value_from_ref(cx, &reference)?),
            EventKind::Failed(reference) => return Err(error_from_failed_ref(cx, &reference)),
            EventKind::Done => break,
            EventKind::Started { .. }
            | EventKind::Claim { .. }
            | EventKind::Chunk { .. }
            | EventKind::EffectRequested { .. }
            | EventKind::EffectResolved { .. }
            | EventKind::Capture { .. }
            | EventKind::Card { .. } => {}
        }
    }

    Ok(EvalReply {
        value: value
            .ok_or_else(|| Error::Eval("eventful realize ended without Final".to_owned()))?,
        diagnostics,
        trace,
    })
}

fn observe_from_eval_request(request: &EvalRequest) -> ObserveMode {
    if request.stream {
        ObserveMode::Events
    } else if request.trace {
        ObserveMode::Ledger
    } else {
        ObserveMode::FinalOnly
    }
}

fn term_from_eval_expr(cx: &mut Cx, expr: &Expr) -> Result<Term> {
    match Term::try_from(expr.clone()) {
        Ok(term) => Ok(term),
        Err(_) => {
            let value = cx.factory().expr(expr.clone())?;
            Ok(Term::Ref(handle_ref_for_value(cx, &value)?))
        }
    }
}

fn eval_expr_from_term(cx: &mut Cx, term: &Term) -> Result<Expr> {
    let Term::Ref(reference) = term else {
        return Ok(Expr::from(term.clone()));
    };
    match TemporaryRefResolver::new().resolve_ref(cx, reference)? {
        ResolvedRef::Symbol(symbol) => Ok(Expr::Symbol(symbol)),
        ResolvedRef::Datum(datum) => Ok(Expr::from(datum)),
        ResolvedRef::Value(value) => value.object().as_expr(cx),
        ResolvedRef::Coordinate(_) | ResolvedRef::Missing(_) => Ok(Expr::from(term.clone())),
    }
}

fn shape_from_ref(cx: &mut Cx, reference: &Ref) -> Result<ShapeRef> {
    match TemporaryRefResolver::new().resolve_ref(cx, reference)? {
        ResolvedRef::Symbol(symbol) => cx.resolve_shape(&symbol),
        ResolvedRef::Value(value) => {
            if value.object().as_shape().is_some() {
                Ok(value)
            } else if let Some(class) = value.object().as_class() {
                class.instance_shape(cx)
            } else {
                Err(Error::TypeMismatch {
                    expected: "shape ref",
                    found: "non-shape",
                })
            }
        }
        ResolvedRef::Datum(_) => Err(Error::TypeMismatch {
            expected: "shape ref",
            found: "non-shape",
        }),
        ResolvedRef::Coordinate(_) | ResolvedRef::Missing(_) => Err(Error::Eval(format!(
            "unresolved result shape ref {reference:?}"
        ))),
    }
}

fn ref_for_realize_request(cx: &mut Cx, request: &RealizeRequest) -> Result<Ref> {
    let value = realize_request_value(cx, request)?;
    handle_ref_for_value(cx, &value)
}

fn realize_request_value(cx: &mut Cx, request: &RealizeRequest) -> Result<Value> {
    let capabilities = request
        .required_capabilities
        .iter()
        .map(|capability| cx.factory().string(capability.as_str().to_owned()))
        .collect::<Result<Vec<_>>>()?;
    let term = cx.factory().expr(Expr::from(request.term.clone()))?;
    let result_shape = optional_ref_value(cx, request.result_shape.as_ref())?;
    let requires = cx.factory().list(capabilities)?;
    let deadline = match request.deadline {
        Some(deadline) => cx.factory().string(format!("{}ms", deadline.as_millis()))?,
        None => cx.factory().nil()?,
    };
    let consistency = cx.factory().symbol(request.consistency.as_symbol())?;
    let mode = cx.factory().symbol(request.mode.as_symbol())?;
    let answer_limit = optional_usize_value(cx, request.answer_limit)?;
    let buffer_limit = optional_usize_value(cx, request.buffer_limit)?;
    let observe = cx.factory().symbol(observe_symbol(request.observe))?;

    cx.factory().table(vec![
        (Symbol::new("term"), term),
        (Symbol::new("result-shape"), result_shape),
        (Symbol::new("requires"), requires),
        (Symbol::new("deadline"), deadline),
        (Symbol::new("consistency"), consistency),
        (Symbol::new("mode"), mode),
        (Symbol::new("answer-limit"), answer_limit),
        (Symbol::new("buffer-limit"), buffer_limit),
        (Symbol::new("observe"), observe),
    ])
}

fn optional_ref_value(cx: &mut Cx, reference: Option<&Ref>) -> Result<Value> {
    match reference {
        Some(reference) => cx.factory().expr(Expr::from(Term::Ref(reference.clone()))),
        None => cx.factory().nil(),
    }
}

fn optional_usize_value(cx: &mut Cx, value: Option<usize>) -> Result<Value> {
    match value {
        Some(value) => cx.factory().string(value.to_string()),
        None => cx.factory().nil(),
    }
}

fn observe_symbol(observe: ObserveMode) -> Symbol {
    Symbol::new(match observe {
        ObserveMode::FinalOnly => "final-only",
        ObserveMode::Events => "events",
        ObserveMode::Ledger => "ledger",
    })
}

fn handle_ref_for_value(cx: &mut Cx, value: &Value) -> Result<Ref> {
    Ok(Ref::Handle(cx.handles_mut().intern(value.clone())))
}

fn error_from_failed_ref(cx: &mut Cx, reference: &Ref) -> Error {
    match value_from_ref(cx, reference) {
        Ok(value) => match value.object().display(cx) {
            Ok(message) => Error::Eval(message),
            Err(err) => err,
        },
        Err(err) => err,
    }
}
