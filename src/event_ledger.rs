//! The event ledger: the contract for accumulating ordered events by tick.
//!
//! The kernel defines the [`EventLedger`] store and its ordering rules;
//! libraries decide what events flow through it and how it is persisted.

use std::collections::BTreeMap;

use crate::{
    error::{Error, Result},
    event::{Event, EventKind, Tick, validate_ticks},
    ref_id::Ref,
};

/// Ordered store of events grouped by run, allocating sequence numbers.
///
/// # Examples
///
/// ```
/// # use sim_kernel::event_ledger::EventLedger;
/// # use sim_kernel::id::Symbol;
/// # use sim_kernel::ref_id::Ref;
/// let mut ledger = EventLedger::new();
/// let run = Ref::Symbol(Symbol::new("run"));
/// let request = Ref::Symbol(Symbol::new("request"));
/// let started = ledger.started(run.clone(), request).unwrap();
/// let done = ledger.done(run.clone()).unwrap();
/// assert_eq!(started.seq, 0);
/// assert_eq!(done.seq, 1);
/// assert_eq!(ledger.len_for_run(&run), 2);
/// ```
#[derive(Clone, Debug, Default)]
pub struct EventLedger {
    next_seq_by_run: BTreeMap<Ref, u64>,
    events_by_run: BTreeMap<Ref, Vec<Event>>,
}

impl EventLedger {
    /// Create an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total number of events across all runs.
    pub fn len(&self) -> usize {
        self.events_by_run.values().map(Vec::len).sum()
    }

    /// Whether the ledger holds no events.
    pub fn is_empty(&self) -> bool {
        self.events_by_run.values().all(Vec::is_empty)
    }

    /// Number of events recorded for `run`.
    pub fn len_for_run(&self, run: &Ref) -> usize {
        self.events_by_run.get(run).map_or(0, Vec::len)
    }

    /// Events recorded for `run`, in order.
    pub fn events_for_run(&self, run: &Ref) -> &[Event] {
        self.events_by_run.get(run).map_or(&[], Vec::as_slice)
    }

    /// Iterate over all events across all runs.
    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events_by_run.values().flat_map(|events| events.iter())
    }

    /// Append an event of `kind` to `run` with no ticks.
    pub fn push(&mut self, run: Ref, kind: EventKind) -> Result<Event> {
        self.push_with_ticks(run, Vec::new(), kind)
    }

    /// Append an event of `kind` to `run` carrying the given ticks.
    pub fn push_with_ticks(
        &mut self,
        run: Ref,
        ticks: Vec<Tick>,
        kind: EventKind,
    ) -> Result<Event> {
        validate_ticks(&ticks)?;
        let seq = self.allocate_seq(&run)?;
        let event = Event::new(run.clone(), seq, ticks, kind)?;
        self.events_by_run
            .entry(run)
            .or_default()
            .push(event.clone());
        Ok(event)
    }

    /// Append a [`EventKind::Started`] event to `run`.
    pub fn started(&mut self, run: Ref, request: Ref) -> Result<Event> {
        self.push(run, EventKind::Started { request })
    }

    /// Append a [`EventKind::Final`] event to `run`.
    pub fn final_value(&mut self, run: Ref, value: Ref) -> Result<Event> {
        self.push(run, EventKind::Final(value))
    }

    /// Append a [`EventKind::Failed`] event to `run`.
    pub fn failed(&mut self, run: Ref, error: Ref) -> Result<Event> {
        self.push(run, EventKind::Failed(error))
    }

    /// Append a [`EventKind::Done`] event to `run`.
    pub fn done(&mut self, run: Ref) -> Result<Event> {
        self.push(run, EventKind::Done)
    }

    fn allocate_seq(&mut self, run: &Ref) -> Result<u64> {
        let next = self.next_seq_by_run.entry(run.clone()).or_insert(0);
        let seq = *next;
        *next = (*next)
            .checked_add(1)
            .ok_or_else(|| Error::Eval("event sequence overflow".to_owned()))?;
        Ok(seq)
    }
}
