use std::sync::Arc;

use crate::{
    CapabilityName, Consistency, Cx, Diagnostic, Error, EvalFabric, EvalMode, EvalReply,
    EvalRequest, EventKind, EventSource, Expr, MatchScore, ObserveMode, RealizeRequest, Result,
    Shape, ShapeDoc, ShapeMatch, Value, realize_events, realize_final,
};

#[derive(Clone)]
struct StaticFabric {
    reply: EvalReply,
}

impl EvalFabric for StaticFabric {
    fn realize(&self, _cx: &mut Cx, _request: EvalRequest) -> Result<EvalReply> {
        Ok(self.reply.clone())
    }
}

struct FixedShape {
    accepted: bool,
}

impl Shape for FixedShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        if self.accepted {
            Ok(ShapeMatch::accept(MatchScore::exact(1)))
        } else {
            Ok(ShapeMatch::reject("realize result rejected"))
        }
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::reject("value-only shape"))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("fixed"))
    }
}

fn request(expr: Expr) -> EvalRequest {
    EvalRequest {
        expr,
        result_shape: None,
        required_capabilities: Vec::new(),
        deadline: None,
        consistency: Consistency::LocalFirst,
        mode: EvalMode::Eval,
        answer_limit: None,
        stream_buffer: None,
        stream: false,
        trace: false,
    }
}

fn reply(value: Value) -> EvalReply {
    EvalReply {
        value,
        diagnostics: Vec::new(),
        trace: None,
    }
}

fn shape_value(cx: &mut Cx, accepted: bool) -> Value {
    cx.factory()
        .opaque(Arc::new(FixedShape { accepted }))
        .unwrap()
}

#[test]
fn eventful_realize_source_drains_to_final_then_done() {
    let mut cx = Cx::stub();
    let value = cx.factory().string("ok".to_owned()).unwrap();
    let fabric = StaticFabric {
        reply: reply(value.clone()),
    };

    let source = realize_events(&mut cx, &fabric, request(Expr::String("task".to_owned())))
        .expect("event source");
    let mut kinds = Vec::new();
    while let Some(event) = source.next(&mut cx).unwrap() {
        kinds.push(event.kind);
    }

    assert!(matches!(kinds.first(), Some(EventKind::Started { .. })));
    assert!(matches!(
        kinds.get(kinds.len().saturating_sub(2)),
        Some(EventKind::Final(_))
    ));
    assert!(matches!(kinds.last(), Some(EventKind::Done)));
}

#[test]
fn eventful_realize_drains_back_to_eval_reply() {
    let mut cx = Cx::stub();
    let value = cx.factory().string("final".to_owned()).unwrap();
    let fabric = StaticFabric {
        reply: reply(value.clone()),
    };

    let drained = realize_final(&mut cx, &fabric, request(Expr::Nil)).unwrap();

    assert_eq!(drained.value, value);
    assert!(drained.diagnostics.is_empty());
    assert!(drained.trace.is_none());
}

#[test]
fn eventful_realize_accepts_matching_result_shape() {
    let mut cx = Cx::stub();
    let value = cx.factory().string("final".to_owned()).unwrap();
    let mut request = request(Expr::Nil);
    request.result_shape = Some(shape_value(&mut cx, true));
    let fabric = StaticFabric {
        reply: reply(value.clone()),
    };

    let drained = realize_final(&mut cx, &fabric, request).unwrap();

    assert_eq!(drained.value, value);
}

#[test]
fn eventful_realize_rejects_mismatched_result_shape() {
    let mut cx = Cx::stub();
    let value = cx.factory().string("final".to_owned()).unwrap();
    let mut request = request(Expr::Nil);
    request.result_shape = Some(shape_value(&mut cx, false));
    let fabric = StaticFabric {
        reply: reply(value),
    };

    let err = realize_events(&mut cx, &fabric, request).unwrap_err();

    assert!(matches!(err, Error::WrongShape { .. }));
}

#[test]
fn diagnostics_survive_eventful_realize() {
    let mut cx = Cx::stub();
    let value = cx.factory().nil().unwrap();
    let diagnostic = Diagnostic::info("kept");
    let fabric = StaticFabric {
        reply: EvalReply {
            value,
            diagnostics: vec![diagnostic.clone()],
            trace: None,
        },
    };

    let drained = realize_final(&mut cx, &fabric, request(Expr::Nil)).unwrap();

    assert_eq!(drained.diagnostics, vec![diagnostic]);
}

#[test]
fn trace_survives_eventful_realize() {
    let mut cx = Cx::stub();
    let value = cx.factory().nil().unwrap();
    let trace = cx.factory().string("trace".to_owned()).unwrap();
    let fabric = StaticFabric {
        reply: EvalReply {
            value,
            diagnostics: Vec::new(),
            trace: Some(trace.clone()),
        },
    };

    let drained = realize_final(&mut cx, &fabric, request(Expr::Nil)).unwrap();

    assert_eq!(drained.trace, Some(trace));
}

#[test]
fn old_eval_request_observation_flags_map_to_realize_request() {
    let mut cx = Cx::stub();
    let mut streaming = request(Expr::Nil);
    streaming.stream = true;
    streaming.trace = true;
    streaming.stream_buffer = Some(8);
    streaming.required_capabilities = vec![CapabilityName::new("demo")];

    let eventful = RealizeRequest::from_eval_request(&mut cx, &streaming).unwrap();
    let back = eventful.to_eval_request(&mut cx).unwrap();

    assert_eq!(eventful.observe, ObserveMode::Events);
    assert_eq!(eventful.buffer_limit, Some(8));
    assert_eq!(
        eventful.required_capabilities,
        vec![CapabilityName::new("demo")]
    );
    assert!(back.stream);
    assert!(back.trace);
    assert_eq!(back.stream_buffer, Some(8));
}
