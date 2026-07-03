//! The effect contract: capability-gated requests resolved by an implementation.
//!
//! The kernel defines the [`Effect`] record, its replay-key identity, and the
//! well-known effect kinds; libraries supply the handlers that perform effects.

use crate::{
    capability::CapabilityName,
    datum::Datum,
    datum_store::DatumStore,
    env::Cx,
    error::{Error, Result},
    expr::NumberLiteral,
    id::Symbol,
    ref_id::{ContentId, Coordinate, HandleId, Ref},
    term::OpKey,
};

/// Version tag mixed into every effect replay-key preimage.
pub const EFFECT_REPLAY_VERSION: &str = "sim6-effect-replay-v1";

/// A capability-gated request resolved by an effect handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// Fresh handle identifying this effect occurrence.
    pub id: Ref,
    /// Symbol naming the effect kind.
    pub kind: Symbol,
    /// Reference to the effect's subject.
    pub subject: Ref,
    /// Reference to the effect's input.
    pub input: Ref,
    /// Shape the effect result must satisfy.
    pub result_shape: Ref,
    /// Operation key used to resume the effect.
    pub resume_op: OpKey,
    /// Operation key used to abort the effect.
    pub abort_op: OpKey,
    /// Capabilities the caller must hold for the effect to run.
    pub requires: Vec<CapabilityName>,
    /// Content-addressed replay key, once computed.
    pub replay_key: Option<ContentId>,
}

impl Effect {
    /// Build an effect with a fresh handle id and no requirements.
    pub fn new(
        kind: Symbol,
        subject: Ref,
        input: Ref,
        result_shape: Ref,
        resume_op: OpKey,
        abort_op: OpKey,
    ) -> Self {
        Self {
            id: Ref::Handle(HandleId::fresh()),
            kind,
            subject,
            input,
            result_shape,
            resume_op,
            abort_op,
            requires: Vec::new(),
            replay_key: None,
        }
    }

    /// Override the effect's id.
    pub fn with_id(mut self, id: Ref) -> Self {
        self.id = id;
        self
    }

    /// Add one required capability to the effect.
    pub fn requiring(mut self, capability: CapabilityName) -> Self {
        self.requires.push(capability);
        self
    }

    /// Replace the effect's required capabilities.
    pub fn with_requirements(mut self, requires: Vec<CapabilityName>) -> Self {
        self.requires = requires;
        self
    }

    /// Compute and attach the replay key for the given implementation.
    pub fn with_replay_key(mut self, implementation: Option<Ref>) -> Result<Self> {
        self.replay_key = Some(effect_replay_key(&self, implementation)?);
        Ok(self)
    }

    /// Return the replay key, computing and caching it if absent.
    pub fn ensure_replay_key(&mut self, implementation: Option<Ref>) -> Result<ContentId> {
        if let Some(key) = &self.replay_key {
            return Ok(key.clone());
        }
        let key = effect_replay_key(self, implementation)?;
        self.replay_key = Some(key.clone());
        Ok(key)
    }
}

/// Ledger record tracking one effect's request, resolution, and outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectRecord {
    /// Reference to the recorded effect.
    pub effect: Ref,
    /// Reference to the requested event.
    pub requested_event: Ref,
    /// Reference to the resolved (or failed) event, once known.
    pub resolved_event: Option<Ref>,
    /// Reference to the effect result, once known.
    pub result: Option<Ref>,
    /// Whether the effect was aborted rather than resolved.
    pub aborted: bool,
}

/// Resolve an effect end to end: record the request, enforce capabilities,
/// reuse any cassette result, otherwise run `perform`, then record the outcome.
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use sim_kernel::{DefaultFactory, NoopEvalPolicy};
/// # use sim_kernel::env::Cx;
/// # use sim_kernel::effect::{
/// #     Effect, resolve_effect, effect_tool_call_kind, effect_resume_op_key,
/// #     effect_abort_op_key,
/// # };
/// # use sim_kernel::id::Symbol;
/// # use sim_kernel::ref_id::Ref;
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let effect = Effect::new(
///     effect_tool_call_kind(),
///     Ref::Symbol(Symbol::qualified("test", "tool")),
///     Ref::Symbol(Symbol::new("input")),
///     Ref::Symbol(Symbol::qualified("core", "Any")),
///     effect_resume_op_key(),
///     effect_abort_op_key(),
/// );
/// let result = Ref::Symbol(Symbol::new("ok"));
/// let got = resolve_effect(&mut cx, effect, {
///     let result = result.clone();
///     move |_cx, _effect| Ok(result)
/// })
/// .unwrap();
/// assert_eq!(got, result);
/// assert_eq!(cx.effect_ledger().records().len(), 1);
/// ```
pub fn resolve_effect<F>(cx: &mut Cx, mut effect: Effect, perform: F) -> Result<Ref>
where
    F: FnOnce(&mut Cx, &Effect) -> Result<Ref>,
{
    let preimage = effect_replay_preimage(&effect, None);
    let replay_key = match effect.replay_key.clone() {
        Some(key) => key,
        None => cx.datum_store_mut().intern(preimage)?,
    };
    effect.replay_key = Some(replay_key.clone());

    let cassette_result = cx.with_effect_ledger(|cx, ledger| {
        ledger.record_requested(cx.datum_store_mut(), effect.clone())?;
        Ok(ledger.cassette_result(&replay_key).cloned())
    })?;

    if let Err(err) = cx.require_all(&effect.requires) {
        record_effect_failure(cx, effect.id.clone(), &err)?;
        return Err(err);
    }

    if let Some(result) = cassette_result {
        cx.with_effect_ledger(|cx, ledger| {
            ledger.record_resolved(cx.datum_store_mut(), effect.id.clone(), result.clone())?;
            Ok(())
        })?;
        return Ok(result);
    }

    match perform(cx, &effect) {
        Ok(result) => {
            cx.with_effect_ledger(|cx, ledger| {
                ledger.record_resolved(cx.datum_store_mut(), effect.id.clone(), result.clone())?;
                Ok(())
            })?;
            Ok(result)
        }
        Err(err) => {
            record_effect_failure(cx, effect.id, &err)?;
            Err(err)
        }
    }
}

/// Content-id of the effect's replay-key preimage for `implementation`.
pub fn effect_replay_key(effect: &Effect, implementation: Option<Ref>) -> Result<ContentId> {
    effect_replay_preimage(effect, implementation).content_id()
}

/// Build the canonical datum hashed into an effect's replay key.
pub fn effect_replay_preimage(effect: &Effect, implementation: Option<Ref>) -> Datum {
    let mut requires = effect.requires.clone();
    requires.sort();
    requires.dedup();
    let mut fields = vec![
        (
            Symbol::new("version"),
            Datum::String(EFFECT_REPLAY_VERSION.to_owned()),
        ),
        (Symbol::new("kind"), Datum::Symbol(effect.kind.clone())),
        (Symbol::new("subject"), ref_datum(effect.subject.clone())),
        (Symbol::new("input"), ref_datum(effect.input.clone())),
        (
            Symbol::new("result-shape"),
            ref_datum(effect.result_shape.clone()),
        ),
        (
            Symbol::new("resume-op"),
            op_key_datum(effect.resume_op.clone()),
        ),
        (
            Symbol::new("abort-op"),
            op_key_datum(effect.abort_op.clone()),
        ),
        (
            Symbol::new("requires"),
            Datum::List(
                requires
                    .into_iter()
                    .map(|capability| Datum::String(capability.as_str().to_owned()))
                    .collect(),
            ),
        ),
    ];
    if let Some(implementation) = implementation {
        fields.push((Symbol::new("implementation"), ref_datum(implementation)));
    }
    Datum::Node {
        tag: core_symbol("EffectReplayKey"),
        fields,
    }
}

/// Well-known kind symbol for tool-call effects.
pub fn effect_tool_call_kind() -> Symbol {
    effect_symbol("tool-call")
}

/// Well-known kind symbol for model-inference effects.
pub fn effect_model_infer_kind() -> Symbol {
    effect_symbol("model-infer")
}

/// Well-known kind symbol for host-process effects.
pub fn effect_host_process_kind() -> Symbol {
    effect_symbol("host-process")
}

/// Well-known kind symbol for network effects.
pub fn effect_network_kind() -> Symbol {
    effect_symbol("network")
}

/// Well-known kind symbol for filesystem effects.
pub fn effect_filesystem_kind() -> Symbol {
    effect_symbol("filesystem")
}

/// Well-known kind symbol for time effects.
pub fn effect_time_kind() -> Symbol {
    effect_symbol("time")
}

/// Well-known kind symbol for randomness effects.
pub fn effect_random_kind() -> Symbol {
    effect_symbol("random")
}

/// Well-known kind symbol for remote-realize effects.
pub fn effect_remote_realize_kind() -> Symbol {
    effect_symbol("remote-realize")
}

/// Well-known kind symbol for test-run effects.
pub fn effect_test_run_kind() -> Symbol {
    effect_symbol("test-run")
}

/// Well-known kind symbol for control-prompt effects.
pub fn effect_control_prompt_kind() -> Symbol {
    effect_symbol("control-prompt")
}

/// Well-known kind symbol for device-read effects.
pub fn effect_device_read_kind() -> Symbol {
    effect_symbol("device-read")
}

/// Well-known kind symbol for device-write effects.
pub fn effect_device_write_kind() -> Symbol {
    effect_symbol("device-write")
}

/// Well-known kind symbol for control-capture effects.
pub fn effect_control_capture_kind() -> Symbol {
    effect_symbol("control-capture")
}

/// Well-known kind symbol for control-abort effects.
pub fn effect_control_abort_kind() -> Symbol {
    effect_symbol("control-abort")
}

/// Well-known kind symbol for control-resume effects.
pub fn effect_control_resume_kind() -> Symbol {
    effect_symbol("control-resume")
}

/// Operation key effects use to resume a handler.
pub fn effect_resume_op_key() -> OpKey {
    OpKey::new(effect_symbol("control"), Symbol::new("resume"), 1)
}

/// Operation key effects use to abort a handler.
pub fn effect_abort_op_key() -> OpKey {
    OpKey::new(effect_symbol("control"), Symbol::new("abort"), 1)
}

fn record_effect_failure(cx: &mut Cx, effect: Ref, err: &Error) -> Result<()> {
    let error_ref = error_ref(cx, err)?;
    cx.with_effect_ledger(|cx, ledger| {
        ledger.record_failed(cx.datum_store_mut(), effect, error_ref)?;
        Ok(())
    })
}

fn error_ref(cx: &mut Cx, err: &Error) -> Result<Ref> {
    let id = cx
        .datum_store_mut()
        .intern(Datum::String(err.to_string()))?;
    Ok(Ref::Content(id))
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

fn op_key_datum(op: OpKey) -> Datum {
    Datum::Node {
        tag: core_symbol("op-key"),
        fields: vec![
            (Symbol::new("namespace"), Datum::Symbol(op.namespace)),
            (Symbol::new("name"), Datum::Symbol(op.name)),
            (
                Symbol::new("version"),
                Datum::Number(NumberLiteral {
                    domain: core_symbol("u16"),
                    canonical: op.version.to_string(),
                }),
            ),
        ],
    }
}

fn effect_symbol(name: &str) -> Symbol {
    Symbol::qualified("effect", name)
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::EventKind;

    use crate::testing::bare_cx as cx;

    fn effect(input: Ref) -> Effect {
        Effect::new(
            effect_tool_call_kind(),
            Ref::Symbol(Symbol::qualified("test", "tool")),
            input,
            Ref::Symbol(core_symbol("Any")),
            effect_resume_op_key(),
            effect_abort_op_key(),
        )
    }

    #[test]
    fn same_replay_preimage_gives_same_key() {
        let left = effect(Ref::Symbol(Symbol::qualified("test", "input")));
        let right = effect(Ref::Symbol(Symbol::qualified("test", "input")));

        assert_eq!(
            effect_replay_key(&left, None).unwrap(),
            effect_replay_key(&right, None).unwrap()
        );
    }

    #[test]
    fn changed_input_gives_different_key() {
        let left = effect(Ref::Symbol(Symbol::qualified("test", "left")));
        let right = effect(Ref::Symbol(Symbol::qualified("test", "right")));

        assert_ne!(
            effect_replay_key(&left, None).unwrap(),
            effect_replay_key(&right, None).unwrap()
        );
    }

    #[test]
    fn resolving_effect_emits_requested_and_resolved_events() {
        let mut cx = cx();
        let result = Ref::Symbol(Symbol::qualified("test", "result"));

        let actual = resolve_effect(&mut cx, effect(Ref::Symbol(Symbol::new("input"))), {
            let result = result.clone();
            move |_cx, _effect| Ok(result)
        })
        .unwrap();

        assert_eq!(actual, result);
        let records = cx.effect_ledger().records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].result, Some(result.clone()));
        let events = cx.effect_ledger().events_for_run();
        assert!(matches!(events[0].kind, EventKind::EffectRequested { .. }));
        assert!(matches!(events[1].kind, EventKind::EffectResolved { .. }));
    }

    #[test]
    fn missing_capability_denies_effect_before_performer_runs() {
        let mut cx = cx();
        let calls = Arc::new(AtomicUsize::new(0));
        let err = resolve_effect(
            &mut cx,
            effect(Ref::Symbol(Symbol::new("input")))
                .requiring(CapabilityName::new("test.required")),
            {
                let calls = calls.clone();
                move |_cx, _effect| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Ref::Symbol(Symbol::new("unreachable")))
                }
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, Error::CapabilityDenied { capability } if capability.as_str() == "test.required")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(cx.effect_ledger().records()[0].aborted);
    }

    #[test]
    fn cassette_result_is_used_when_replay_key_matches() {
        let mut cx = cx();
        let mut effect = effect(Ref::Symbol(Symbol::new("input")));
        let key = effect.ensure_replay_key(None).unwrap();
        let cassette = Ref::Symbol(Symbol::qualified("test", "cassette-result"));
        cx.effect_ledger_mut()
            .insert_cassette_result(key, cassette.clone());
        let calls = Arc::new(AtomicUsize::new(0));

        let actual = resolve_effect(&mut cx, effect, {
            let calls = calls.clone();
            move |_cx, _effect| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Ref::Symbol(Symbol::new("performed")))
            }
        })
        .unwrap();

        assert_eq!(actual, cassette);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
