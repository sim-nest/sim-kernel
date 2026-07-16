//! Shape reports: the contract for diagnostics from [`shape`](crate::shape)
//! checks.
//!
//! The kernel defines the report record built from a shape match and the
//! satisfaction-claim it publishes; libraries produce reports by running the
//! shape protocol.

use crate::{
    capability::fact_private_capability,
    claim::{Claim, ClaimKind, Visibility},
    datum::Datum,
    datum_store::DatumStore,
    env::Cx,
    error::{Diagnostic, Error, Result, Severity},
    expr::{Expr, NumberLiteral, Span},
    hint::HintMetadata,
    id::Symbol,
    object::ShapeRef,
    ref_id::{ContentId, Coordinate, HandleId, Ref},
    ref_resolver::{RefResolver, TemporaryRefResolver},
    shape::{MatchScore, ShapeBindings, ShapeMatch},
    term::Term,
    value::Value,
};

/// The canonical record of a shape check: who was checked against what, the
/// outcome, and the diagnostics.
///
/// A report is interned content (the `id`) and is the evidence backing the
/// `satisfies-shape` satisfaction claim published when a match is accepted.
/// The kernel defines this record and that claim; libraries produce reports by
/// running the [`shape`](crate::shape) protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeReport {
    /// Content reference identifying this report.
    pub id: Ref,
    /// Reference to the shape that was checked.
    pub shape: Ref,
    /// Reference to the value or expr that was checked.
    pub target: Ref,
    /// Whether the target satisfied the shape.
    pub accepted: bool,
    /// The match score.
    pub score: MatchScore,
    /// Reference to the interned captures datum.
    pub captures: Ref,
    /// Diagnostics gathered during the check.
    pub diagnostics: Vec<Diagnostic>,
}

impl ShapeReport {
    /// Build the canonical [`Datum`] for this report (excluding its own id).
    pub fn canonical_datum(&self) -> Datum {
        shape_report_datum(
            &self.shape,
            &self.target,
            self.accepted,
            self.score,
            &self.captures,
            &self.diagnostics,
        )
    }
}

/// Check `value` against `shape_value`, building a [`ShapeReport`] and
/// publishing the satisfaction claim when accepted.
///
/// Returns a [`TypeMismatch`](crate::Error::TypeMismatch) error when
/// `shape_value` is not a shape.
pub fn check_value_report(
    cx: &mut Cx,
    shape_value: &ShapeRef,
    value: Value,
) -> Result<ShapeReport> {
    let shape_ref = ref_for_shape(cx, shape_value)?;
    let mut resolver = TemporaryRefResolver::new();
    let target_ref = resolver.ref_for_value(cx, &value)?;
    let visibility = satisfaction_visibility(cx, &target_ref, &shape_ref, &value)?;

    let Some(shape) = shape_value.object().as_shape() else {
        return Err(Error::TypeMismatch {
            expected: "shape",
            found: "non-shape",
        });
    };
    let matched = shape.check_value(cx, value)?;
    let report = shape_report_from_match(cx, shape_ref, target_ref, matched)?;
    insert_shape_satisfaction_claim(cx, &report, visibility)?;
    Ok(report)
}

/// Intern a [`ShapeMatch`] into a [`ShapeReport`] for the given shape and
/// target references.
pub fn shape_report_from_match(
    cx: &mut Cx,
    shape: Ref,
    target: Ref,
    matched: ShapeMatch,
) -> Result<ShapeReport> {
    let captures = captures_ref(cx, &matched.captures)?;
    let datum = shape_report_datum(
        &shape,
        &target,
        matched.accepted,
        matched.score,
        &captures,
        &matched.diagnostics,
    );
    let id = cx.datum_store_mut().intern(datum)?;
    Ok(ShapeReport {
        id: Ref::Content(id),
        shape,
        target,
        accepted: matched.accepted,
        score: matched.score,
        captures,
        diagnostics: matched.diagnostics,
    })
}

/// Publish the `satisfies-shape` claim for an accepted report.
///
/// Returns `Ok(None)` when the report was rejected. Private visibility inserts
/// the claim under the fact-private capability.
pub fn insert_shape_satisfaction_claim(
    cx: &mut Cx,
    report: &ShapeReport,
    visibility: Visibility,
) -> Result<Option<Ref>> {
    if !report.accepted {
        return Ok(None);
    }

    let claim = Claim::public(
        report.target.clone(),
        satisfies_shape_predicate(),
        report.shape.clone(),
    )
    .with_kind(ClaimKind::Observed)
    .with_evidence(vec![report.id.clone()])
    .with_visibility(visibility);

    if visibility == Visibility::Private {
        let mut capabilities = cx.capabilities().clone();
        capabilities.insert(fact_private_capability());
        cx.with_capabilities(capabilities, |cx| cx.insert_fact(claim))
            .map(Some)
    } else {
        cx.insert_fact(claim).map(Some)
    }
}

/// The predicate symbol (`core/satisfies-shape`) used by satisfaction claims.
pub fn satisfies_shape_predicate() -> Symbol {
    core_symbol("satisfies-shape")
}

fn ref_for_shape(cx: &mut Cx, shape_value: &ShapeRef) -> Result<Ref> {
    if let Some(symbol) = shape_value
        .object()
        .as_shape()
        .and_then(|shape| shape.symbol())
    {
        return Ok(Ref::Symbol(symbol));
    }
    TemporaryRefResolver::new().ref_for_value(cx, shape_value)
}

fn satisfaction_visibility(
    cx: &mut Cx,
    target: &Ref,
    shape: &Ref,
    value: &Value,
) -> Result<Visibility> {
    if matches!(target, Ref::Handle(_))
        && !value
            .object()
            .publish_shape_satisfaction_claims(cx, shape)?
    {
        return Ok(Visibility::Private);
    }
    Ok(Visibility::Public)
}

fn captures_ref(cx: &mut Cx, captures: &ShapeBindings) -> Result<Ref> {
    let datum = captures_datum(cx, captures)?;
    cx.datum_store_mut().intern(datum).map(Ref::Content)
}

fn captures_datum(cx: &mut Cx, captures: &ShapeBindings) -> Result<Datum> {
    let mut resolver = TemporaryRefResolver::new();
    let values = captures
        .values()
        .iter()
        .map(|(name, value)| {
            let value_ref = resolver.ref_for_value(cx, value)?;
            Ok(binding_datum(name, "value", ref_datum(&value_ref)))
        })
        .collect::<Result<Vec<_>>>()?;
    let exprs = captures
        .exprs()
        .iter()
        .map(|(name, expr)| Ok(binding_datum(name, "expr", expr_datum(expr)?)))
        .collect::<Result<Vec<_>>>()?;

    Ok(Datum::Node {
        tag: core_symbol("ShapeCaptures"),
        fields: vec![
            (Symbol::new("values"), Datum::Vector(values)),
            (Symbol::new("exprs"), Datum::Vector(exprs)),
        ],
    })
}

fn binding_datum(name: &Symbol, kind: &str, value: Datum) -> Datum {
    Datum::Node {
        tag: core_symbol("shape-binding"),
        fields: vec![
            (Symbol::new("name"), Datum::Symbol(name.clone())),
            (Symbol::new("kind"), Datum::Symbol(core_symbol(kind))),
            (Symbol::new("value"), value),
        ],
    }
}

fn expr_datum(expr: &Expr) -> Result<Datum> {
    match Datum::try_from(expr.clone()) {
        Ok(datum) => Ok(datum),
        Err(_) => Ok(Term::lower(expr.clone())?.to_datum()),
    }
}

fn shape_report_datum(
    shape: &Ref,
    target: &Ref,
    accepted: bool,
    score: MatchScore,
    captures: &Ref,
    diagnostics: &[Diagnostic],
) -> Datum {
    Datum::Node {
        tag: core_symbol("ShapeReport"),
        fields: vec![
            (Symbol::new("shape"), ref_datum(shape)),
            (Symbol::new("target"), ref_datum(target)),
            (Symbol::new("accepted"), Datum::Bool(accepted)),
            (Symbol::new("score"), score_datum(score)),
            (Symbol::new("captures"), ref_datum(captures)),
            (
                Symbol::new("diagnostics"),
                Datum::Vector(diagnostics.iter().map(diagnostic_datum).collect()),
            ),
        ],
    }
}

fn diagnostic_datum(diagnostic: &Diagnostic) -> Datum {
    let hints = HintMetadata::collect_from_diagnostic(diagnostic)
        .into_iter()
        .map(|hint| hint.as_datum())
        .collect();
    let mut fields = vec![
        (
            Symbol::new("severity"),
            Datum::Symbol(severity_symbol(diagnostic.severity)),
        ),
        (
            Symbol::new("message"),
            Datum::String(diagnostic.message.clone()),
        ),
        (
            Symbol::new("related"),
            Datum::Vector(
                diagnostic
                    .related
                    .iter()
                    .filter(|related| !HintMetadata::is_hint_diagnostic(related))
                    .map(diagnostic_datum)
                    .collect(),
            ),
        ),
        (Symbol::new("hints"), Datum::Vector(hints)),
    ];
    if let Some(source) = &diagnostic.source {
        fields.push((Symbol::new("source"), Datum::String(source.0.clone())));
    }
    if let Some(span) = &diagnostic.span {
        fields.push((Symbol::new("span"), span_datum(span)));
    }
    if let Some(code) = &diagnostic.code {
        fields.push((Symbol::new("code"), Datum::Symbol(code.clone())));
    }
    Datum::Node {
        tag: core_symbol("diagnostic"),
        fields,
    }
}

fn span_datum(span: &Span) -> Datum {
    Datum::Node {
        tag: core_symbol("span"),
        fields: vec![
            (
                Symbol::new("start"),
                Datum::Number(usize_number(span.start)),
            ),
            (Symbol::new("end"), Datum::Number(usize_number(span.end))),
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

fn score_datum(score: MatchScore) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: score.value().to_string(),
    })
}

fn usize_number(value: usize) -> NumberLiteral {
    NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    }
}

fn ref_datum(reference: &Ref) -> Datum {
    match reference {
        Ref::Symbol(symbol) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("symbol"))),
                (Symbol::new("symbol"), Datum::Symbol(symbol.clone())),
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
                (Symbol::new("id"), handle_id_datum(*handle)),
            ],
        },
        Ref::Coord(coordinate) => coordinate_datum(coordinate),
    }
}

fn coordinate_datum(coordinate: &Coordinate) -> Datum {
    Datum::Node {
        tag: core_symbol("ref"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(core_symbol("coord"))),
            (
                Symbol::new("space"),
                Datum::Symbol(coordinate.space.clone()),
            ),
            (
                Symbol::new("ordinal"),
                content_id_datum(&coordinate.ordinal),
            ),
        ],
    }
}

fn content_id_datum(content: &ContentId) -> Datum {
    Datum::Node {
        tag: core_symbol("content-id"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(content.algorithm.clone()),
            ),
            (Symbol::new("bytes"), Datum::Bytes(content.bytes.to_vec())),
        ],
    }
}

fn handle_id_datum(handle: HandleId) -> Datum {
    Datum::Bytes(handle.0.to_be_bytes().to_vec())
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}

#[cfg(test)]
#[path = "shape_report/tests.rs"]
mod tests;
