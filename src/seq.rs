//! The [`Sequence`] contract: pull-based iteration over runtime values.
//!
//! The kernel defines the sequence protocol and its next/close/peek surface,
//! including bridging events into sequence items; libraries supply the concrete
//! sequence sources.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::{
    env::Cx,
    error::{Error, Result},
    event::{Event, EventKind, EventSource, Tick, validate_ticks},
    expr::Expr,
    id::{CORE_SEQUENCE_CLASS_ID, Symbol},
    list::ListValue,
    object::{ClassRef, Object},
    ref_id::Ref,
    ref_resolver::value_from_ref,
    stream::Stream,
    term::Term,
    value::Value,
};

/// One item produced by a sequence: a value plus its clock ticks.
#[derive(Clone, Debug)]
pub struct SequenceItem {
    value: Value,
    ticks: Vec<Tick>,
}

impl SequenceItem {
    /// Builds an item carrying a value with no ticks.
    pub fn new(value: Value) -> Self {
        Self {
            value,
            ticks: Vec::new(),
        }
    }

    /// Builds an item with ticks, validating their clock ordering.
    pub fn with_ticks(value: Value, ticks: Vec<Tick>) -> Result<Self> {
        validate_ticks(&ticks)?;
        Ok(Self { value, ticks })
    }

    /// Borrows the carried value.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Borrows the carried clock ticks.
    pub fn ticks(&self) -> &[Tick] {
        &self.ticks
    }

    /// Projects the item to a runtime value, folding in any ticks.
    pub fn into_value(self, cx: &mut Cx) -> Result<Value> {
        sequence_item_value(cx, self)
    }
}

/// Pull-based iteration protocol over runtime values.
pub trait Sequence: Send + Sync {
    /// Pulls the next item, or `Ok(None)` once exhausted.
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>>;

    /// Releases resources backing the sequence; idempotent by default.
    fn close(&self, _cx: &mut Cx) -> Result<()> {
        Ok(())
    }

    /// Peeks the next item without consuming it; unsupported by default.
    fn peek_item(&self, _cx: &mut Cx) -> Result<Option<SequenceItem>> {
        Err(Error::Eval("sequence does not support peek".to_owned()))
    }

    /// Whether the sequence is known to be exhausted; `false` by default.
    fn is_done(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(false)
    }
}

/// Pulls the next item from a sequence.
pub fn seq_next(cx: &mut Cx, sequence: &dyn Sequence) -> Result<Option<SequenceItem>> {
    sequence.next_item(cx)
}

/// Closes a sequence.
pub fn seq_close(cx: &mut Cx, sequence: &dyn Sequence) -> Result<()> {
    sequence.close(cx)
}

/// Peeks the next item of a sequence without consuming it.
pub fn seq_peek(cx: &mut Cx, sequence: &dyn Sequence) -> Result<Option<SequenceItem>> {
    sequence.peek_item(cx)
}

/// Reports whether a sequence is exhausted.
pub fn seq_is_done(cx: &mut Cx, sequence: &dyn Sequence) -> Result<bool> {
    sequence.is_done(cx)
}

/// Pulls the next item from a value that is a sequence or a stream.
pub fn seq_next_value(cx: &mut Cx, target: &Value) -> Result<Option<SequenceItem>> {
    if let Some(sequence) = target.object().as_sequence() {
        return seq_next(cx, sequence);
    }
    if let Some(stream) = target.object().as_stream() {
        return stream.next(cx).map(|item| item.map(SequenceItem::new));
    }
    Err(Error::TypeMismatch {
        expected: "sequence",
        found: "non-sequence",
    })
}

/// Closes a value that is a sequence or a stream.
pub fn seq_close_value(cx: &mut Cx, target: &Value) -> Result<()> {
    if let Some(sequence) = target.object().as_sequence() {
        return seq_close(cx, sequence);
    }
    if let Some(stream) = target.object().as_stream() {
        return stream.close(cx);
    }
    Err(Error::TypeMismatch {
        expected: "sequence",
        found: "non-sequence",
    })
}

/// Projects a sequence item to a value, wrapping it in a payload/ticks table
/// when ticks are present.
pub fn sequence_item_value(cx: &mut Cx, item: SequenceItem) -> Result<Value> {
    if item.ticks.is_empty() {
        return Ok(item.value);
    }

    let ticks = item
        .ticks
        .into_iter()
        .map(|tick| tick_value(cx, tick))
        .collect::<Result<Vec<_>>>()?;
    let ticks = cx.factory().list(ticks)?;
    cx.factory().table(vec![
        (Symbol::new("payload"), item.value),
        (Symbol::new("ticks"), ticks),
    ])
}

/// Maps an event into a sequence item, or `Ok(None)` for non-payload events;
/// failure events are surfaced as errors.
pub fn sequence_item_from_event(cx: &mut Cx, event: Event) -> Result<Option<SequenceItem>> {
    match event.kind {
        EventKind::Chunk { payload } | EventKind::Final(payload) => {
            SequenceItem::with_ticks(value_from_ref(cx, &payload)?, event.ticks).map(Some)
        }
        EventKind::Failed(error) => Err(error_from_failed_ref(cx, &error)),
        EventKind::Started { .. }
        | EventKind::Claim { .. }
        | EventKind::Diagnostic(_)
        | EventKind::Trace(_)
        | EventKind::EffectRequested { .. }
        | EventKind::EffectResolved { .. }
        | EventKind::Capture { .. }
        | EventKind::Card { .. }
        | EventKind::Done => Ok(None),
    }
}

// sim-non-citizen(reason = "live event-source sequence adapter; descriptor data is the underlying event source", kind = "handle", descriptor = "core/EventSource")
/// Sequence adapter that drains an [`EventSource`] into sequence items.
pub struct EventSourceSequence {
    source: Arc<dyn EventSource>,
    state: Mutex<EventSourceSequenceState>,
}

#[derive(Debug, Default)]
struct EventSourceSequenceState {
    pending: VecDeque<Event>,
    done: bool,
}

impl EventSourceSequence {
    /// Wraps an event source as a sequence.
    pub fn new(source: Arc<dyn EventSource>) -> Self {
        Self {
            source,
            state: Mutex::new(EventSourceSequenceState::default()),
        }
    }
}

impl Object for EventSourceSequence {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<event-source-sequence>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EventSourceSequence {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            CORE_SEQUENCE_CLASS_ID,
            Symbol::qualified("core", "EventSourceSequence"),
        )
    }
    fn as_sequence(&self) -> Option<&dyn Sequence> {
        Some(self)
    }
}

impl Sequence for EventSourceSequence {
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        while let Some(event) = self.next_event(cx)? {
            let done = matches!(event.kind, EventKind::Done);
            if let Some(item) = sequence_item_from_event(cx, event)? {
                return Ok(Some(item));
            }
            if done {
                self.mark_done()?;
                return Ok(None);
            }
        }
        Ok(None)
    }

    fn close(&self, cx: &mut Cx) -> Result<()> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("event source sequence"))?;
            state.pending.clear();
            state.done = true;
        }
        self.source.close(cx)
    }

    fn peek_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        loop {
            if let Some(item) = self.pending_item(cx)? {
                return Ok(Some(item));
            }
            if self.is_done(cx)? {
                return Ok(None);
            }
            let Some(event) = self.source.next(cx)? else {
                self.mark_done()?;
                return Ok(None);
            };
            let done = matches!(event.kind, EventKind::Done);
            self.push_pending(event)?;
            if done {
                self.mark_done()?;
            }
        }
    }

    fn is_done(&self, _cx: &mut Cx) -> Result<bool> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("event source sequence"))?;
        Ok(state.done && state.pending.is_empty())
    }
}

impl EventSourceSequence {
    fn next_event(&self, cx: &mut Cx) -> Result<Option<Event>> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("event source sequence"))?;
            if let Some(event) = state.pending.pop_front() {
                return Ok(Some(event));
            }
            if state.done {
                return Ok(None);
            }
        }

        match self.source.next(cx)? {
            Some(event) => Ok(Some(event)),
            None => {
                self.mark_done()?;
                Ok(None)
            }
        }
    }

    fn pending_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        let events = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("event source sequence"))?
            .pending
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for event in events {
            if let Some(item) = sequence_item_from_event(cx, event)? {
                return Ok(Some(item));
            }
        }
        Ok(None)
    }

    fn push_pending(&self, event: Event) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("event source sequence"))?;
        state.pending.push_back(event);
        Ok(())
    }

    fn mark_done(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("event source sequence"))?;
        state.done = true;
        Ok(())
    }
}

// sim-non-citizen(reason = "live list sequence cursor; canonical list data remains the source list value", kind = "handle", descriptor = "core/List")
/// Sequence cursor that walks the spine of a kernel list value.
#[derive(Debug)]
pub struct ListSequence {
    state: Mutex<ListSequenceState>,
}

#[derive(Debug)]
struct ListSequenceState {
    next: Option<Value>,
    closed: bool,
}

impl ListSequence {
    /// Builds a sequence that iterates the given list value.
    pub fn new(list: Value) -> Self {
        Self {
            state: Mutex::new(ListSequenceState {
                next: Some(list),
                closed: false,
            }),
        }
    }
}

impl Object for ListSequence {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<list-sequence>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for ListSequence {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            CORE_SEQUENCE_CLASS_ID,
            Symbol::qualified("core", "Sequence"),
        )
    }
    fn as_sequence(&self) -> Option<&dyn Sequence> {
        Some(self)
    }
}

impl Sequence for ListSequence {
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("list sequence"))?;
        if state.closed {
            return Ok(None);
        }

        let Some(node) = state.next.clone() else {
            state.closed = true;
            return Ok(None);
        };
        let list = list_from_value(&node)?;
        if list.is_empty(cx)? {
            state.next = None;
            state.closed = true;
            return Ok(None);
        }

        let value = list
            .car(cx)?
            .ok_or_else(|| Error::Eval("list car missing for non-empty list".to_owned()))?;
        state.next = list.cdr(cx)?;
        Ok(Some(SequenceItem::new(value)))
    }

    fn close(&self, _cx: &mut Cx) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("list sequence"))?;
        state.next = None;
        state.closed = true;
        Ok(())
    }

    fn peek_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("list sequence"))?;
        if state.closed {
            return Ok(None);
        }

        let Some(node) = state.next.as_ref() else {
            state.closed = true;
            return Ok(None);
        };
        let list = list_from_value(node)?;
        if list.is_empty(cx)? {
            state.next = None;
            state.closed = true;
            return Ok(None);
        }
        list.car(cx).map(|item| item.map(SequenceItem::new))
    }

    fn is_done(&self, cx: &mut Cx) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("list sequence"))?;
        if state.closed {
            return Ok(true);
        }

        let Some(node) = state.next.as_ref() else {
            state.closed = true;
            return Ok(true);
        };
        let done = list_from_value(node)?.is_empty(cx)?;
        if done {
            state.next = None;
            state.closed = true;
        }
        Ok(done)
    }
}

impl<T> Sequence for T
where
    T: Stream + ?Sized,
{
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        self.next(cx).map(|item| item.map(SequenceItem::new))
    }

    fn close(&self, cx: &mut Cx) -> Result<()> {
        Stream::close(self, cx)
    }
}

fn list_from_value(value: &Value) -> Result<&dyn ListValue> {
    value.object().as_list().ok_or(Error::TypeMismatch {
        expected: "list",
        found: "non-list",
    })
}

fn tick_value(cx: &mut Cx, tick: Tick) -> Result<Value> {
    let clock = cx.factory().symbol(tick.clock)?;
    let index = ref_value(cx, tick.index)?;
    cx.factory().table(vec![
        (Symbol::new("clock"), clock),
        (Symbol::new("index"), index),
    ])
}

fn ref_value(cx: &mut Cx, reference: Ref) -> Result<Value> {
    cx.factory().expr(Expr::from(Term::Ref(reference)))
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
