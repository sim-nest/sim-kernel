//! The effect ledger: the contract for recording and replaying effects.
//!
//! The kernel defines the [`EffectLedger`] record store; libraries decide how
//! effects are performed and how the ledger is persisted.

use crate::{
    datum::Datum,
    datum_store::DatumStore,
    effect::{Effect, EffectRecord},
    error::{Error, Result},
    event::{Event, EventKind, Tick},
    event_ledger::EventLedger,
    expr::NumberLiteral,
    id::Symbol,
    ref_id::{ContentId, Coordinate, HandleId, Ref},
};

/// Per-run store of effect records, their events, and replay cassettes.
#[derive(Clone, Debug)]
pub struct EffectLedger {
    run: Ref,
    events: EventLedger,
    records: Vec<EffectRecord>,
    effects_by_ref: Vec<(Ref, Effect)>,
    cassette_results: Vec<(ContentId, Ref)>,
}

impl Default for EffectLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectLedger {
    /// Create a ledger for the default `effect.ledger` run.
    pub fn new() -> Self {
        Self::with_run(Ref::Symbol(Symbol::qualified("effect", "ledger")))
    }

    /// Create a ledger scoped to `run`.
    pub fn with_run(run: Ref) -> Self {
        Self {
            run,
            events: EventLedger::new(),
            records: Vec::new(),
            effects_by_ref: Vec::new(),
            cassette_results: Vec::new(),
        }
    }

    /// All effect records, in request order.
    pub fn records(&self) -> &[EffectRecord] {
        &self.records
    }

    /// The underlying event ledger.
    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// Events recorded for this ledger's run, in order.
    pub fn events_for_run(&self) -> &[Event] {
        self.events.events_for_run(&self.run)
    }

    /// Recorded cassette result for replay key `key`, if any.
    pub fn cassette_result(&self, key: &ContentId) -> Option<&Ref> {
        self.cassette_results
            .iter()
            .rev()
            .find_map(|(recorded_key, result)| (recorded_key == key).then_some(result))
    }

    /// Record a cassette result for replay key `key`.
    pub fn insert_cassette_result(&mut self, key: ContentId, result: Ref) {
        self.cassette_results.push((key, result));
    }

    /// The effect recorded under `reference`, if any.
    pub fn effect(&self, reference: &Ref) -> Option<&Effect> {
        self.effects_by_ref
            .iter()
            .rev()
            .find_map(|(effect_ref, effect)| (effect_ref == reference).then_some(effect))
    }

    /// Record an effect request, emitting an [`EventKind::EffectRequested`]
    /// event and opening a new record.
    pub fn record_requested(
        &mut self,
        datum_store: &mut impl DatumStore,
        effect: Effect,
    ) -> Result<EffectRecord> {
        let effect_ref = effect.id.clone();
        let event = self.events.push(
            self.run.clone(),
            EventKind::EffectRequested {
                effect: effect_ref.clone(),
            },
        )?;
        let requested_event = event_ref(datum_store, &event)?;
        let record = EffectRecord {
            effect: effect_ref.clone(),
            requested_event,
            resolved_event: None,
            result: None,
            aborted: false,
        };
        self.effects_by_ref.push((effect_ref, effect));
        self.records.push(record.clone());
        Ok(record)
    }

    /// Record an effect resolution, emitting an [`EventKind::EffectResolved`]
    /// event and closing the open record with its result.
    pub fn record_resolved(
        &mut self,
        datum_store: &mut impl DatumStore,
        effect: Ref,
        result: Ref,
    ) -> Result<EffectRecord> {
        let event = self.events.push(
            self.run.clone(),
            EventKind::EffectResolved {
                effect: effect.clone(),
                result: result.clone(),
            },
        )?;
        let resolved_event = event_ref(datum_store, &event)?;
        let record = self.open_record_mut(&effect)?;
        record.resolved_event = Some(resolved_event);
        record.result = Some(result.clone());
        Ok(record.clone())
    }

    /// Record an effect failure, emitting an [`EventKind::Failed`] event and
    /// marking the open record aborted.
    pub fn record_failed(
        &mut self,
        datum_store: &mut impl DatumStore,
        effect: Ref,
        error: Ref,
    ) -> Result<EffectRecord> {
        let event = self
            .events
            .push(self.run.clone(), EventKind::Failed(error))?;
        let failed_event = event_ref(datum_store, &event)?;
        let record = self.open_record_mut(&effect)?;
        record.resolved_event = Some(failed_event);
        record.aborted = true;
        Ok(record.clone())
    }

    fn open_record_mut(&mut self, effect: &Ref) -> Result<&mut EffectRecord> {
        self.records
            .iter_mut()
            .rev()
            .find(|record| &record.effect == effect && record.resolved_event.is_none())
            .ok_or_else(|| Error::Eval(format!("effect record not found for {effect:?}")))
    }
}

fn event_ref(datum_store: &mut impl DatumStore, event: &Event) -> Result<Ref> {
    let id = datum_store.intern(event_datum(event))?;
    Ok(Ref::Content(id))
}

fn event_datum(event: &Event) -> Datum {
    Datum::Node {
        tag: core_symbol("Event"),
        fields: vec![
            (Symbol::new("run"), ref_datum(event.run.clone())),
            (Symbol::new("seq"), u64_datum(event.seq)),
            (
                Symbol::new("ticks"),
                Datum::List(event.ticks.iter().map(tick_datum).collect()),
            ),
            (Symbol::new("kind"), event_kind_datum(&event.kind)),
        ],
    }
}

fn tick_datum(tick: &Tick) -> Datum {
    Datum::Node {
        tag: core_symbol("Tick"),
        fields: vec![
            (Symbol::new("clock"), Datum::Symbol(tick.clock.clone())),
            (Symbol::new("index"), ref_datum(tick.index.clone())),
        ],
    }
}

fn event_kind_datum(kind: &EventKind) -> Datum {
    match kind {
        EventKind::Started { request } => tagged_ref("Started", "request", request.clone()),
        EventKind::Claim { claim } => tagged_ref("Claim", "claim", claim.clone()),
        EventKind::Diagnostic(diagnostic) => Datum::Node {
            tag: core_symbol("Diagnostic"),
            fields: vec![(
                Symbol::new("debug"),
                Datum::String(format!("{diagnostic:?}")),
            )],
        },
        EventKind::Trace(trace) => tagged_ref("Trace", "trace", trace.clone()),
        EventKind::Chunk { payload } => tagged_ref("Chunk", "payload", payload.clone()),
        EventKind::EffectRequested { effect } => {
            tagged_ref("EffectRequested", "effect", effect.clone())
        }
        EventKind::EffectResolved { effect, result } => Datum::Node {
            tag: core_symbol("EffectResolved"),
            fields: vec![
                (Symbol::new("effect"), ref_datum(effect.clone())),
                (Symbol::new("result"), ref_datum(result.clone())),
            ],
        },
        EventKind::Capture { effect } => tagged_ref("Capture", "effect", effect.clone()),
        EventKind::Card { subject, card } => Datum::Node {
            tag: core_symbol("Card"),
            fields: vec![
                (Symbol::new("subject"), ref_datum(subject.clone())),
                (Symbol::new("card"), ref_datum(card.clone())),
            ],
        },
        EventKind::Final(value) => tagged_ref("Final", "value", value.clone()),
        EventKind::Failed(error) => tagged_ref("Failed", "error", error.clone()),
        EventKind::Done => Datum::Node {
            tag: core_symbol("Done"),
            fields: Vec::new(),
        },
    }
}

fn tagged_ref(tag: &str, field: &str, reference: Ref) -> Datum {
    Datum::Node {
        tag: core_symbol(tag),
        fields: vec![(Symbol::new(field), ref_datum(reference))],
    }
}

fn ref_datum(reference: Ref) -> Datum {
    match reference {
        Ref::Symbol(symbol) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("symbol"))),
                (Symbol::new("symbol"), Datum::Symbol(symbol)),
            ],
        },
        Ref::Content(content) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("content"))),
                (Symbol::new("content"), content_id_datum(content)),
            ],
        },
        Ref::Handle(handle) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("handle"))),
                (Symbol::new("id"), handle_id_datum(handle)),
            ],
        },
        Ref::Coord(coordinate) => coordinate_datum(coordinate),
    }
}

fn coordinate_datum(coordinate: Coordinate) -> Datum {
    Datum::Node {
        tag: core_symbol("ref"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(core_symbol("coord"))),
            (Symbol::new("space"), Datum::Symbol(coordinate.space)),
            (Symbol::new("ordinal"), content_id_datum(coordinate.ordinal)),
        ],
    }
}

fn content_id_datum(content: ContentId) -> Datum {
    Datum::Node {
        tag: core_symbol("content-id"),
        fields: vec![
            (Symbol::new("algorithm"), Datum::Symbol(content.algorithm)),
            (Symbol::new("bytes"), Datum::Bytes(content.bytes.to_vec())),
        ],
    }
}

fn handle_id_datum(handle: HandleId) -> Datum {
    Datum::Bytes(handle.0.to_be_bytes().to_vec())
}

fn u64_datum(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: core_symbol("u64"),
        canonical: value.to_string(),
    })
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}
