//! The event contract: timestamped records emitted during evaluation.
//!
//! The kernel defines the [`Event`] record, its kinds, logical [`Tick`]
//! clocks, and the [`EventSource`] contract; libraries produce the events.

use std::collections::BTreeSet;

use crate::{
    env::Cx,
    error::{Diagnostic, Error, Result},
    id::Symbol,
    ref_id::Ref,
};

/// A logical-clock reading: one index along a named clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tick {
    /// Symbol naming the logical clock.
    pub clock: Symbol,
    /// Reference to this clock's index value.
    pub index: Ref,
}

impl Tick {
    /// Build a tick for `clock` at `index`.
    pub fn new(clock: Symbol, index: Ref) -> Self {
        Self { clock, index }
    }
}

/// A timestamped record emitted during a run's evaluation.
///
/// # Examples
///
/// ```
/// # use sim_kernel::event::{Event, EventKind, Tick};
/// # use sim_kernel::id::Symbol;
/// # use sim_kernel::ref_id::Ref;
/// let run = Ref::Symbol(Symbol::new("run"));
/// let event = Event::started(run, 0, Ref::Symbol(Symbol::new("request"))).unwrap();
/// assert!(matches!(event.kind, EventKind::Started { .. }));
///
/// // Ticks must carry distinct clocks.
/// let dup = vec![
///     Tick::new(Symbol::new("clock"), Ref::Symbol(Symbol::new("a"))),
///     Tick::new(Symbol::new("clock"), Ref::Symbol(Symbol::new("b"))),
/// ];
/// assert!(
///     Event::new(Ref::Symbol(Symbol::new("run")), 1, dup, EventKind::Done).is_err()
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Reference identifying the run that emitted the event.
    pub run: Ref,
    /// Per-run monotonic sequence number.
    pub seq: u64,
    /// Logical-clock readings at the time of emission.
    pub ticks: Vec<Tick>,
    /// The event payload.
    pub kind: EventKind,
}

impl Event {
    /// Build an event after validating its ticks have distinct clocks.
    pub fn new(run: Ref, seq: u64, ticks: Vec<Tick>, kind: EventKind) -> Result<Self> {
        validate_ticks(&ticks)?;
        Ok(Self {
            run,
            seq,
            ticks,
            kind,
        })
    }

    /// Build a [`EventKind::Started`] event with no ticks.
    pub fn started(run: Ref, seq: u64, request: Ref) -> Result<Self> {
        Self::new(run, seq, Vec::new(), EventKind::Started { request })
    }

    /// Build a [`EventKind::Final`] event with no ticks.
    pub fn final_value(run: Ref, seq: u64, value: Ref) -> Result<Self> {
        Self::new(run, seq, Vec::new(), EventKind::Final(value))
    }

    /// Build a [`EventKind::Failed`] event with no ticks.
    pub fn failed(run: Ref, seq: u64, error: Ref) -> Result<Self> {
        Self::new(run, seq, Vec::new(), EventKind::Failed(error))
    }

    /// Build a [`EventKind::Done`] event with no ticks.
    pub fn done(run: Ref, seq: u64) -> Result<Self> {
        Self::new(run, seq, Vec::new(), EventKind::Done)
    }
}

/// The payload variants an [`Event`] can carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// A run started from the given request.
    Started {
        /// Reference to the originating request.
        request: Ref,
    },
    /// A claim was asserted.
    Claim {
        /// Reference to the asserted claim.
        claim: Ref,
    },
    /// A diagnostic was emitted.
    Diagnostic(Diagnostic),
    /// A trace marker was emitted.
    Trace(Ref),
    /// A streamed chunk of output.
    Chunk {
        /// Reference to the chunk payload.
        payload: Ref,
    },
    /// An effect was requested.
    EffectRequested {
        /// Reference to the requested effect.
        effect: Ref,
    },
    /// An effect was resolved with a result.
    EffectResolved {
        /// Reference to the resolved effect.
        effect: Ref,
        /// Reference to the effect result.
        result: Ref,
    },
    /// A continuation capture occurred at an effect.
    Capture {
        /// Reference to the captured effect.
        effect: Ref,
    },
    /// A card was published for a subject.
    Card {
        /// Reference to the card's subject.
        subject: Ref,
        /// Reference to the card.
        card: Ref,
    },
    /// The run produced its final value.
    Final(Ref),
    /// The run failed with an error.
    Failed(Ref),
    /// The run finished.
    Done,
}

/// A source that produces a run's events in order.
///
/// The kernel defines the pull contract; libraries supply the production.
pub trait EventSource: Send + Sync {
    /// Pull the next event, or `None` when the source is exhausted.
    fn next(&self, cx: &mut Cx) -> Result<Option<Event>>;

    /// Release the source's resources; the default is a no-op.
    fn close(&self, _cx: &mut Cx) -> Result<()> {
        Ok(())
    }
}

/// Validate that `ticks` carry distinct clocks, erroring on a duplicate.
pub fn validate_ticks(ticks: &[Tick]) -> Result<()> {
    let mut clocks = BTreeSet::new();
    for tick in ticks {
        if !clocks.insert(tick.clock.clone()) {
            return Err(Error::Eval(format!(
                "duplicate event tick clock {}",
                tick.clock
            )));
        }
    }
    Ok(())
}
