//! Stream metadata and packet events: the contract for stream transport.
//!
//! The kernel defines the stream predicate vocabulary and the packet/frame
//! event shapes; concrete stream transports and clocks live in library crates.

use crate::{
    card::card_kind_predicate,
    claim::{Claim, ClaimPattern},
    env::Cx,
    error::{Error, Result},
    event::{Event, EventKind, Tick},
    id::Symbol,
    ref_id::Ref,
};

/// Card-kind symbol marking a ref as a stream (`core/stream`).
pub fn stream_kind() -> Symbol {
    Symbol::qualified("core", "stream")
}

/// Predicate symbol naming a stream's clock (`stream/clock`).
pub fn stream_clock_predicate() -> Symbol {
    Symbol::qualified("stream", "clock")
}

/// Predicate symbol naming a stream's payload codec (`stream/codec`).
pub fn stream_codec_predicate() -> Symbol {
    Symbol::qualified("stream", "codec")
}

/// Predicate symbol naming a stream's transport (`stream/transport`).
pub fn stream_transport_predicate() -> Symbol {
    Symbol::qualified("stream", "transport")
}

/// Records the stream kind plus the given metadata predicates as facts, once.
pub fn publish_stream_metadata_claims(
    cx: &mut Cx,
    stream: Ref,
    metadata: impl IntoIterator<Item = (Symbol, Ref)>,
) -> Result<()> {
    insert_once(
        cx,
        stream.clone(),
        card_kind_predicate(),
        Ref::Symbol(stream_kind()),
    )?;
    for (predicate, object) in metadata {
        insert_once(cx, stream.clone(), predicate, object)?;
    }
    Ok(())
}

/// Builds a stream packet event whose payload is a content or handle ref.
pub fn stream_packet_event(run: Ref, seq: u64, ticks: Vec<Tick>, payload: Ref) -> Result<Event> {
    match payload {
        Ref::Content(_) | Ref::Handle(_) => {
            Event::new(run, seq, ticks, EventKind::Chunk { payload })
        }
        Ref::Symbol(_) | Ref::Coord(_) => Err(Error::Eval(
            "stream packet payload must be a content or handle ref".to_owned(),
        )),
    }
}

/// Builds a remote stream frame event; an alias for [`stream_packet_event`].
pub fn remote_stream_frame_event(
    run: Ref,
    seq: u64,
    ticks: Vec<Tick>,
    frame: Ref,
) -> Result<Event> {
    stream_packet_event(run, seq, ticks, frame)
}

fn insert_once(cx: &mut Cx, subject: Ref, predicate: Symbol, object: Ref) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        cx.insert_fact(Claim::public(subject, predicate, object))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentId, Coordinate, DefaultFactory, Expr, NoopEvalPolicy, card::card_for_ref};
    use std::sync::Arc;

    #[test]
    fn stream_metadata_and_packet_events_use_claims_and_refs() {
        let mut cx = Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let stream = Ref::Handle(cx.fresh_handle());
        publish_stream_metadata_claims(
            &mut cx,
            stream.clone(),
            [(
                stream_clock_predicate(),
                Ref::Symbol(Symbol::qualified("clock", "sample")),
            )],
        )
        .unwrap();

        let claims = cx
            .query_facts(ClaimPattern::exact(
                stream,
                stream_clock_predicate(),
                Ref::Symbol(Symbol::qualified("clock", "sample")),
            ))
            .unwrap();
        assert_eq!(claims.len(), 1);

        let payload = Ref::Content(ContentId::from_bytes(
            Symbol::qualified("core", "sha256"),
            [7; 32],
        ));
        let event = stream_packet_event(
            Ref::Symbol(Symbol::qualified("run", "one")),
            0,
            Vec::new(),
            payload.clone(),
        )
        .unwrap();
        assert!(matches!(event.kind, EventKind::Chunk { payload: actual } if actual == payload));
    }

    #[test]
    fn stream_metadata_and_payload_rules_project_to_cards() {
        let mut cx = Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let stream = Ref::Handle(cx.fresh_handle());
        publish_stream_metadata_claims(
            &mut cx,
            stream.clone(),
            [
                (
                    stream_clock_predicate(),
                    Ref::Symbol(Symbol::qualified("clock", "sample")),
                ),
                (
                    stream_codec_predicate(),
                    Ref::Symbol(Symbol::qualified("codec", "binary-base64")),
                ),
                (
                    stream_transport_predicate(),
                    Ref::Symbol(Symbol::qualified("stream", "memory")),
                ),
            ],
        )
        .unwrap();

        let card = card_expr(&mut cx, stream.clone());
        assert_eq!(
            table_value(&card, "kind"),
            Some(&Expr::Symbol(stream_kind()))
        );

        let run = Ref::Symbol(Symbol::qualified("run", "one"));
        let ordinal = ContentId::from_bytes(Symbol::qualified("core", "sha256"), [3; 32]);
        let coordinate = Ref::Coord(Coordinate {
            space: Symbol::qualified("rank", "space"),
            ordinal,
        });
        assert!(
            stream_packet_event(run.clone(), 0, Vec::new(), Ref::Symbol(Symbol::new("bad")))
                .is_err()
        );
        assert!(stream_packet_event(run, 0, Vec::new(), coordinate).is_err());
    }

    fn card_expr(cx: &mut Cx, subject: Ref) -> Expr {
        card_for_ref(cx, subject)
            .unwrap()
            .object()
            .as_expr(cx)
            .unwrap()
    }

    fn table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a Expr> {
        let Expr::Map(entries) = expr else {
            return None;
        };
        entries.iter().find_map(|(entry_key, entry_value)| {
            let Expr::Symbol(entry_key) = entry_key else {
                return None;
            };
            (entry_key == &Symbol::new(key)).then_some(entry_value)
        })
    }
}
