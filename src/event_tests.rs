use crate::{ContentId, Error, EventKind, EventLedger, Ref, Symbol, Tick};

fn symbol(name: &str) -> Symbol {
    Symbol::qualified("test", name)
}

fn symbol_ref(name: &str) -> Ref {
    Ref::Symbol(symbol(name))
}

fn content_ref(byte: u8) -> Ref {
    Ref::Content(ContentId::from_bytes(symbol("content"), [byte; 32]))
}

#[test]
fn events_in_one_run_get_monotonic_sequence_numbers() {
    let mut ledger = EventLedger::new();
    let run = symbol_ref("run");

    let started = ledger.started(run.clone(), symbol_ref("request")).unwrap();
    let final_value = ledger
        .final_value(run.clone(), symbol_ref("result"))
        .unwrap();
    let done = ledger.done(run.clone()).unwrap();

    assert_eq!(started.seq, 0);
    assert_eq!(final_value.seq, 1);
    assert_eq!(done.seq, 2);
    assert_eq!(ledger.events_for_run(&run), &[started, final_value, done]);

    let other_run = symbol_ref("other-run");
    assert_eq!(ledger.done(other_run).unwrap().seq, 0);
}

#[test]
fn events_with_ticks_still_order_by_sequence() {
    let mut ledger = EventLedger::new();
    let run = symbol_ref("tick-run");

    let late_tick = Tick::new(symbol("clock"), content_ref(9));
    let early_tick = Tick::new(symbol("clock"), content_ref(1));

    let first = ledger
        .push_with_ticks(
            run.clone(),
            vec![late_tick],
            EventKind::Chunk {
                payload: symbol_ref("first"),
            },
        )
        .unwrap();
    let second = ledger
        .push_with_ticks(
            run.clone(),
            vec![early_tick],
            EventKind::Chunk {
                payload: symbol_ref("second"),
            },
        )
        .unwrap();

    assert_eq!(first.seq, 0);
    assert_eq!(second.seq, 1);
    assert_eq!(ledger.events_for_run(&run)[0].seq, 0);
    assert_eq!(ledger.events_for_run(&run)[1].seq, 1);
}

#[test]
fn duplicate_clock_ticks_in_one_event_are_rejected() {
    let mut ledger = EventLedger::new();
    let run = symbol_ref("duplicate-run");
    let clock = symbol("clock");
    let ticks = vec![
        Tick::new(clock.clone(), content_ref(1)),
        Tick::new(clock, content_ref(2)),
    ];

    let error = ledger
        .push_with_ticks(run.clone(), ticks, EventKind::Done)
        .unwrap_err();

    assert!(
        matches!(error, Error::Eval(message) if message.contains("duplicate event tick clock"))
    );
    assert!(ledger.events_for_run(&run).is_empty());
    assert_eq!(ledger.done(run).unwrap().seq, 0);
}

#[test]
fn chunk_event_carries_payload_ref_and_tick_ref() {
    let mut ledger = EventLedger::new();
    let run = symbol_ref("chunk-run");
    let payload = content_ref(7);
    let tick_ref = content_ref(8);

    let event = ledger
        .push_with_ticks(
            run,
            vec![Tick::new(symbol("sample-clock"), tick_ref.clone())],
            EventKind::Chunk {
                payload: payload.clone(),
            },
        )
        .unwrap();

    assert_eq!(event.ticks[0].index, tick_ref);
    assert_eq!(event.kind, EventKind::Chunk { payload });
}
