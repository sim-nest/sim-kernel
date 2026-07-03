use super::*;
use crate::{
    Error, EventKind,
    capability::{
        control_capture_capability, control_multishot_capability, control_prompt_capability,
    },
};

use crate::testing::bare_cx as cx;

fn symbol_ref(namespace: &str, name: &str) -> Ref {
    Ref::Symbol(Symbol::qualified(namespace, name))
}

#[test]
fn prompt_body_can_return_normally() {
    let mut cx = cx();
    cx.grant(control_prompt_capability());
    let expected = symbol_ref("test", "prompt-result");

    let actual = prompt(
        &mut cx,
        ControlPrompt::new(
            default_control_prompt(),
            symbol_ref("test", "input"),
            default_control_result_shape(),
        ),
        {
            let expected = expected.clone();
            move |_cx| Ok(expected)
        },
    )
    .unwrap();

    assert_eq!(actual, expected);
    let events = cx.effect_ledger().events_for_run();
    assert!(matches!(events[0].kind, EventKind::EffectRequested { .. }));
    assert!(matches!(events[1].kind, EventKind::EffectResolved { .. }));
}

#[test]
fn capture_without_capability_is_denied() {
    let mut cx = cx();

    let err = capture(
        &mut cx,
        ControlCapture::new(
            default_control_prompt(),
            symbol_ref("test", "continuation"),
            symbol_ref("test", "value"),
            default_control_result_shape(),
        ),
    )
    .unwrap_err();

    assert!(
        matches!(err, Error::CapabilityDenied { capability } if capability == control_capture_capability())
    );
}

#[test]
fn noop_policy_returns_structured_unsupported_result() {
    let mut cx = cx();
    cx.grant(control_capture_capability());

    let result = capture(
        &mut cx,
        ControlCapture::new(
            default_control_prompt(),
            symbol_ref("test", "continuation"),
            symbol_ref("test", "value"),
            default_control_result_shape(),
        ),
    )
    .unwrap();

    assert_eq!(
        control_result_status(&cx, &result).unwrap(),
        Some(control_unsupported_status())
    );
    assert_eq!(cx.diagnostics().messages().len(), 1);
    assert_eq!(
        cx.diagnostics().messages()[0].code,
        Some(control_unsupported_status())
    );
}

#[test]
fn multishot_capture_requires_multishot_capability() {
    let mut cx = cx();
    cx.grant(control_capture_capability());

    let err = capture(
        &mut cx,
        ControlCapture::new(
            default_control_prompt(),
            symbol_ref("test", "continuation"),
            symbol_ref("test", "value"),
            default_control_result_shape(),
        )
        .multishot(),
    )
    .unwrap_err();

    assert!(
        matches!(err, Error::CapabilityDenied { capability } if capability == control_multishot_capability())
    );
}
