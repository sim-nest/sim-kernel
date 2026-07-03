use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::{
    ClassRef, ContentId, Cx, Event, EventKind, EventSourceSequence, HandleStore, ListSequence,
    Object, Ref, Result, Stream, Symbol, Tick, Value, core_seq_next_op_key, invoke_op, seq_is_done,
    seq_next, seq_peek, sequence_item_value,
};

fn symbol(name: &str) -> Symbol {
    Symbol::qualified("test", name)
}

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn content_ref(byte: u8) -> Ref {
    Ref::Content(ContentId::from_bytes(symbol("content"), [byte; 32]))
}

#[derive(Debug)]
struct QueueStream {
    items: Mutex<VecDeque<Value>>,
}

impl QueueStream {
    fn new(items: Vec<Value>) -> Self {
        Self {
            items: Mutex::new(items.into()),
        }
    }
}

impl Object for QueueStream {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<queue-stream>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for QueueStream {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            crate::CORE_SEQUENCE_CLASS_ID,
            Symbol::qualified("test", "Stream"),
        )
    }
    fn as_stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }
}

impl Stream for QueueStream {
    fn next(&self, _cx: &mut Cx) -> Result<Option<Value>> {
        Ok(self.items.lock().unwrap().pop_front())
    }

    fn close(&self, _cx: &mut Cx) -> Result<()> {
        self.items.lock().unwrap().clear();
        Ok(())
    }
}

#[test]
fn list_sequence_supports_next_peek_and_done_helpers() {
    let mut cx = Cx::stub();
    let first = string(&mut cx, "a");
    let second = string(&mut cx, "b");
    let list = cx
        .factory()
        .list(vec![first.clone(), second.clone()])
        .unwrap();
    let sequence = ListSequence::new(list);

    let peeked = seq_peek(&mut cx, &sequence).unwrap().unwrap();
    assert_eq!(peeked.value(), &first);
    assert!(!seq_is_done(&mut cx, &sequence).unwrap());

    assert_eq!(
        seq_next(&mut cx, &sequence).unwrap().unwrap().value(),
        &first
    );
    assert_eq!(
        seq_next(&mut cx, &sequence).unwrap().unwrap().value(),
        &second
    );
    assert!(seq_next(&mut cx, &sequence).unwrap().is_none());
    assert!(seq_is_done(&mut cx, &sequence).unwrap());
}

#[test]
fn old_stream_values_work_through_core_seq_next() {
    let mut cx = Cx::stub();
    let expected = string(&mut cx, "streamed");
    let target = cx
        .factory()
        .opaque(Arc::new(QueueStream::new(vec![expected.clone()])))
        .unwrap();
    let input = cx.factory().nil().unwrap();

    let first = invoke_op(
        &mut cx,
        target.clone(),
        &core_seq_next_op_key(),
        input.clone(),
    )
    .unwrap();
    let crate::Step::Value(value) = first else {
        panic!("seq-next should return a value step");
    };
    assert_eq!(value, expected);

    let done = invoke_op(&mut cx, target, &core_seq_next_op_key(), input).unwrap();
    let crate::Step::Value(value) = done else {
        panic!("seq-next should return a value step");
    };
    assert!(matches!(
        value.object().as_expr(&mut cx).unwrap(),
        crate::Expr::Nil
    ));
}

#[test]
fn event_source_sequence_exposes_chunk_ticks() {
    let mut cx = Cx::stub();
    let run = Ref::Symbol(symbol("run"));
    let payload = string(&mut cx, "chunk");
    let payload_ref = Ref::Handle(cx.handles_mut().intern(payload.clone()));
    let index = content_ref(9);
    let clock = symbol("sample-clock");
    let event = Event::new(
        run.clone(),
        0,
        vec![Tick::new(clock.clone(), index.clone())],
        EventKind::Chunk {
            payload: payload_ref,
        },
    )
    .unwrap();
    let done = Event::done(run, 1).unwrap();
    let source = Arc::new(crate::BufferedEventSource::new(vec![event, done]));
    let sequence = EventSourceSequence::new(source);

    let item = seq_next(&mut cx, &sequence).unwrap().unwrap();
    assert_eq!(item.value(), &payload);
    assert_eq!(item.ticks()[0].clock, clock.clone());
    assert_eq!(item.ticks()[0].index, index.clone());

    let value = sequence_item_value(&mut cx, item).unwrap();
    let table = value.object().as_table_impl().unwrap();
    assert_eq!(
        table
            .get(&mut cx, Symbol::new("payload"))
            .unwrap()
            .object()
            .display(&mut cx)
            .unwrap(),
        "chunk"
    );
    let ticks = table.get(&mut cx, Symbol::new("ticks")).unwrap();
    let tick = ticks
        .object()
        .as_list()
        .unwrap()
        .car(&mut cx)
        .unwrap()
        .unwrap();
    let tick_table = tick.object().as_table_impl().unwrap();
    assert_eq!(
        tick_table
            .get(&mut cx, Symbol::new("clock"))
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        crate::Expr::Symbol(clock)
    );
    assert_eq!(
        tick_table
            .get(&mut cx, Symbol::new("index"))
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        crate::Expr::from(crate::Term::Ref(index))
    );
}
