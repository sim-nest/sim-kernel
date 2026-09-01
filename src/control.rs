//! Control policy: the contract for delimited prompts, capture, and resume.
//!
//! The kernel defines the prompt/capture/abort/resume records and the
//! [`ControlPolicy`] trait; libraries implement the concrete control behavior.

use std::sync::Arc;

use crate::{
    capability::{
        CapabilityName, control_capture_capability, control_multishot_capability,
        control_prompt_capability, control_resume_capability,
    },
    datum::Datum,
    datum_store::DatumStore,
    effect::{
        Effect, effect_abort_op_key, effect_control_abort_kind, effect_control_capture_kind,
        effect_control_prompt_kind, effect_control_resume_kind, effect_resume_op_key,
        resolve_effect,
    },
    env::Cx,
    error::{Diagnostic, Result, Severity},
    id::Symbol,
    op::core_any_ref,
    ref_id::{ContentId, Coordinate, HandleId, Ref},
};

/// Record describing a delimited control prompt to enter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlPrompt {
    /// Reference identifying the prompt boundary.
    pub prompt: Ref,
    /// Input value supplied to the prompt body.
    pub input: Ref,
    /// Shape the prompt result must satisfy.
    pub result_shape: Ref,
}

impl ControlPrompt {
    /// Build a prompt record from its boundary, input, and result shape.
    pub fn new(prompt: Ref, input: Ref, result_shape: Ref) -> Self {
        Self {
            prompt,
            input,
            result_shape,
        }
    }
}

/// Record describing a capture of the continuation up to a prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlCapture {
    /// Reference identifying the prompt boundary captured up to.
    pub prompt: Ref,
    /// Reference to the captured continuation.
    pub continuation: Ref,
    /// Value delivered to the captured continuation.
    pub value: Ref,
    /// Shape the resumed result must satisfy.
    pub result_shape: Ref,
    /// Whether the continuation may be resumed more than once.
    pub multishot: bool,
}

impl ControlCapture {
    /// Build a single-shot capture record.
    pub fn new(prompt: Ref, continuation: Ref, value: Ref, result_shape: Ref) -> Self {
        Self {
            prompt,
            continuation,
            value,
            result_shape,
            multishot: false,
        }
    }

    /// Mark the capture as resumable more than once.
    pub fn multishot(mut self) -> Self {
        self.multishot = true;
        self
    }
}

/// Record describing an abort that unwinds to a prompt with a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlAbort {
    /// Reference identifying the prompt boundary to unwind to.
    pub prompt: Ref,
    /// Value delivered as the prompt result.
    pub value: Ref,
    /// Shape the prompt result must satisfy.
    pub result_shape: Ref,
}

impl ControlAbort {
    /// Build an abort record from its prompt, value, and result shape.
    pub fn new(prompt: Ref, value: Ref, result_shape: Ref) -> Self {
        Self {
            prompt,
            value,
            result_shape,
        }
    }
}

/// Record describing a resume of a captured continuation with a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlResume {
    /// Reference to the continuation being resumed.
    pub continuation: Ref,
    /// Value delivered to the resumed continuation.
    pub value: Ref,
    /// Shape the resumed result must satisfy.
    pub result_shape: Ref,
}

impl ControlResume {
    /// Build a resume record from its continuation, value, and result shape.
    pub fn new(continuation: Ref, value: Ref, result_shape: Ref) -> Self {
        Self {
            continuation,
            value,
            result_shape,
        }
    }
}

/// Policy implementing delimited control: prompts, capture, abort, and resume.
///
/// The kernel defines this contract and the records it consumes; libraries
/// supply the concrete continuation machinery. Unsupported operations report an
/// "unsupported" control result rather than failing hard.
pub trait ControlPolicy: Send + Sync {
    /// Stable name identifying the policy in diagnostics.
    fn name(&self) -> &'static str;

    /// Enter a prompt boundary; the default is a no-op.
    fn enter_prompt(&self, _cx: &mut Cx, _prompt: &ControlPrompt) -> Result<()> {
        Ok(())
    }

    /// Capture the continuation up to a prompt; defaults to unsupported.
    fn capture(&self, cx: &mut Cx, _capture: &ControlCapture) -> Result<Ref> {
        unsupported_control_result(cx, self.name(), effect_control_capture_kind())
    }

    /// Abort to a prompt with a value; defaults to unsupported.
    fn abort(&self, cx: &mut Cx, _abort: &ControlAbort) -> Result<Ref> {
        unsupported_control_result(cx, self.name(), effect_control_abort_kind())
    }

    /// Resume a captured continuation with a value; defaults to unsupported.
    fn resume(&self, cx: &mut Cx, _resume: &ControlResume) -> Result<Ref> {
        unsupported_control_result(cx, self.name(), effect_control_resume_kind())
    }
}

/// Shared, reference-counted handle to a [`ControlPolicy`].
pub type ControlPolicyRef = Arc<dyn ControlPolicy>;

/// Control policy that supports prompts but rejects capture, abort, and resume.
#[derive(Default)]
pub struct NoopControlPolicy;

impl ControlPolicy for NoopControlPolicy {
    fn name(&self) -> &'static str {
        "noop-control"
    }
}

/// Run `body` inside a control prompt, emitting the prompt effect first.
pub fn prompt<F>(cx: &mut Cx, prompt: ControlPrompt, body: F) -> Result<Ref>
where
    F: FnOnce(&mut Cx) -> Result<Ref>,
{
    let effect = prompt_effect(cx.fresh_handle(), &prompt);
    resolve_effect(cx, effect, |cx, _effect| {
        let policy = cx.control_policy_ref();
        policy.enter_prompt(cx, &prompt)?;
        body(cx)
    })
}

/// Capture the continuation up to a prompt via the active control policy.
pub fn capture(cx: &mut Cx, capture: ControlCapture) -> Result<Ref> {
    let effect = capture_effect(cx, &capture)?;
    resolve_effect(cx, effect, |cx, _effect| {
        let policy = cx.control_policy_ref();
        policy.capture(cx, &capture)
    })
}

/// Abort to a prompt with a value via the active control policy.
pub fn abort(cx: &mut Cx, abort: ControlAbort) -> Result<Ref> {
    let effect = abort_effect(cx.fresh_handle(), &abort);
    resolve_effect(cx, effect, |cx, _effect| {
        let policy = cx.control_policy_ref();
        policy.abort(cx, &abort)
    })
}

/// Resume a captured continuation with a value via the active control policy.
pub fn resume(cx: &mut Cx, resume: ControlResume) -> Result<Ref> {
    let effect = resume_effect(cx.fresh_handle(), &resume);
    resolve_effect(cx, effect, |cx, _effect| {
        let policy = cx.control_policy_ref();
        policy.resume(cx, &resume)
    })
}

/// Build the capability-gated effect that requests a control prompt.
pub fn prompt_effect(id: crate::HandleId, prompt: &ControlPrompt) -> Effect {
    Effect::new(
        id,
        effect_control_prompt_kind(),
        prompt.prompt.clone(),
        prompt.input.clone(),
        prompt.result_shape.clone(),
        effect_resume_op_key(),
        effect_abort_op_key(),
    )
    .requiring(control_prompt_capability())
}

/// Build the capability-gated effect that requests a continuation capture.
pub fn capture_effect(cx: &mut Cx, capture: &ControlCapture) -> Result<Effect> {
    let input = intern_control_input(
        cx,
        control_capture_status(),
        vec![
            (
                Symbol::new("continuation"),
                ref_datum(capture.continuation.clone()),
            ),
            (Symbol::new("value"), ref_datum(capture.value.clone())),
            (Symbol::new("multishot"), Datum::Bool(capture.multishot)),
        ],
    )?;
    Ok(Effect::new(
        cx.fresh_handle(),
        effect_control_capture_kind(),
        capture.prompt.clone(),
        input,
        capture.result_shape.clone(),
        effect_resume_op_key(),
        effect_abort_op_key(),
    )
    .with_requirements(control_requirements(
        control_capture_capability(),
        capture.multishot,
    )))
}

/// Build the capability-gated effect that requests an abort.
pub fn abort_effect(id: crate::HandleId, abort: &ControlAbort) -> Effect {
    Effect::new(
        id,
        effect_control_abort_kind(),
        abort.prompt.clone(),
        abort.value.clone(),
        abort.result_shape.clone(),
        effect_resume_op_key(),
        effect_abort_op_key(),
    )
    .requiring(control_capture_capability())
}

/// Build the capability-gated effect that requests a resume.
pub fn resume_effect(id: crate::HandleId, resume: &ControlResume) -> Effect {
    Effect::new(
        id,
        effect_control_resume_kind(),
        resume.continuation.clone(),
        resume.value.clone(),
        resume.result_shape.clone(),
        effect_resume_op_key(),
        effect_abort_op_key(),
    )
    .requiring(control_resume_capability())
}

/// Intern a control result recording a captured continuation and value.
pub fn captured_control_result(cx: &mut Cx, continuation: Ref, value: Ref) -> Result<Ref> {
    intern_control_result(
        cx,
        control_captured_status(),
        vec![
            (Symbol::new("continuation"), ref_datum(continuation)),
            (Symbol::new("value"), ref_datum(value)),
        ],
    )
}

/// Intern a control result recording an abort to a prompt with a value.
pub fn aborted_control_result(cx: &mut Cx, prompt: Ref, value: Ref) -> Result<Ref> {
    intern_control_result(
        cx,
        control_aborted_status(),
        vec![
            (Symbol::new("prompt"), ref_datum(prompt)),
            (Symbol::new("value"), ref_datum(value)),
        ],
    )
}

/// Intern a control result recording a resumed continuation and value.
pub fn resumed_control_result(cx: &mut Cx, continuation: Ref, value: Ref) -> Result<Ref> {
    intern_control_result(
        cx,
        control_resumed_status(),
        vec![
            (Symbol::new("continuation"), ref_datum(continuation)),
            (Symbol::new("value"), ref_datum(value)),
        ],
    )
}

/// Intern an "unsupported" control result and push its diagnostic, for
/// policies that do not implement `operation`.
pub fn unsupported_control_result(
    cx: &mut Cx,
    policy: &'static str,
    operation: Symbol,
) -> Result<Ref> {
    let diagnostic = unsupported_control_diagnostic(policy, operation);
    cx.push_diagnostic(diagnostic.clone());
    intern_control_result(
        cx,
        control_unsupported_status(),
        vec![(Symbol::new("diagnostic"), diagnostic_datum(diagnostic))],
    )
}

/// Build the diagnostic reported when `policy` cannot perform `operation`.
pub fn unsupported_control_diagnostic(policy: &'static str, operation: Symbol) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(format!(
        "control policy {policy} does not support {operation}"
    ));
    diagnostic.code = Some(control_unsupported_status());
    diagnostic
}

/// Read the status symbol of an interned control result, if `result` is one.
pub fn control_result_status(cx: &Cx, result: &Ref) -> Result<Option<Symbol>> {
    let Ref::Content(id) = result else {
        return Ok(None);
    };
    let Some(Datum::Node { tag, fields }) = cx.datum_store().get(id)? else {
        return Ok(None);
    };
    if tag != &control_result_tag() {
        return Ok(None);
    }
    Ok(fields.iter().find_map(|(field, value)| {
        if field == &Symbol::new("status")
            && let Datum::Symbol(status) = value
        {
            return Some(status.clone());
        }
        None
    }))
}

/// Status symbol naming a prompt-entry control operation.
pub fn control_prompt_status() -> Symbol {
    control_symbol("prompt")
}

/// Status symbol naming a capture control operation.
pub fn control_capture_status() -> Symbol {
    control_symbol("capture")
}

/// Status symbol for a control result that captured a continuation.
pub fn control_captured_status() -> Symbol {
    control_symbol("captured")
}

/// Status symbol for a control result that aborted to a prompt.
pub fn control_aborted_status() -> Symbol {
    control_symbol("aborted")
}

/// Status symbol for a control result that resumed a continuation.
pub fn control_resumed_status() -> Symbol {
    control_symbol("resumed")
}

/// Status symbol for a control result the policy did not support.
pub fn control_unsupported_status() -> Symbol {
    control_symbol("unsupported")
}

/// The default prompt boundary reference.
pub fn default_control_prompt() -> Ref {
    Ref::Symbol(control_symbol("default-prompt"))
}

/// The default control result shape (the open `core` any shape).
pub fn default_control_result_shape() -> Ref {
    core_any_ref()
}

fn control_requirements(primary: CapabilityName, multishot: bool) -> Vec<CapabilityName> {
    let mut requires = vec![primary];
    if multishot {
        requires.push(control_multishot_capability());
    }
    requires
}

fn intern_control_input(
    cx: &mut Cx,
    operation: Symbol,
    mut fields: Vec<(Symbol, Datum)>,
) -> Result<Ref> {
    fields.insert(0, (Symbol::new("operation"), Datum::Symbol(operation)));
    let id = cx.datum_store_mut().intern(Datum::Node {
        tag: control_input_tag(),
        fields,
    })?;
    Ok(Ref::Content(id))
}

fn intern_control_result(
    cx: &mut Cx,
    status: Symbol,
    mut fields: Vec<(Symbol, Datum)>,
) -> Result<Ref> {
    fields.insert(0, (Symbol::new("status"), Datum::Symbol(status)));
    let id = cx.datum_store_mut().intern(Datum::Node {
        tag: control_result_tag(),
        fields,
    })?;
    Ok(Ref::Content(id))
}

fn diagnostic_datum(diagnostic: Diagnostic) -> Datum {
    Datum::Node {
        tag: core_symbol("Diagnostic"),
        fields: vec![
            (
                Symbol::new("severity"),
                Datum::Symbol(severity_symbol(diagnostic.severity)),
            ),
            (Symbol::new("message"), Datum::String(diagnostic.message)),
            (
                Symbol::new("code"),
                diagnostic.code.map_or(Datum::Nil, Datum::Symbol),
            ),
        ],
    }
}

fn severity_symbol(severity: Severity) -> Symbol {
    match severity {
        Severity::Error => core_symbol("error"),
        Severity::Warning => core_symbol("warning"),
        Severity::Info => core_symbol("info"),
        Severity::Note => core_symbol("note"),
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

fn control_input_tag() -> Symbol {
    core_symbol("ControlInput")
}

fn control_result_tag() -> Symbol {
    core_symbol("ControlResult")
}

fn control_symbol(name: &str) -> Symbol {
    Symbol::qualified("control", name)
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}

#[cfg(test)]
mod tests;
