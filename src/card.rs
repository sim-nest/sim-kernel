//! Card records: the contract for structured metadata about runtime subjects.
//!
//! The kernel defines the [`Card`] record and its well-known predicate keys
//! (subject, kind, help, args, result, tests, ops); libraries publish and
//! consume cards as claims.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    claim::{Claim, ClaimPattern},
    datum_store::DatumStore,
    env::Cx,
    error::Result,
    expr::Expr,
    force_list_to_vec,
    handle_store::HandleStore,
    id::{CORE_CARD_CLASS_ID, Symbol},
    object::{ClassRef, Object},
    ref_id::{ContentId, Coordinate, HandleId, Ref},
    ref_resolver::{RefResolver, TemporaryRefResolver, value_from_datum},
    value::Value,
};

// sim-non-citizen(reason = "browse/help Card projection", kind = "marker", descriptor = "")
/// Structured, machine-readable record describing a runtime subject.
///
/// A `Card` is an ordinary runtime [`Object`]: a `subject` [`Ref`] plus ordered
/// `(Symbol, Value)` entries projected from claims and fallback table data. The
/// kernel fixes the Card schema and well-known predicate keys (see the README
/// section "Contract: Card records and the browse/help/test schema"); libraries
/// implement browse over these records. Browse output is ordinary runtime data;
/// consumers must not parse display strings.
#[derive(Clone)]
pub struct Card {
    subject: Ref,
    entries: Vec<(Symbol, Value)>,
}

impl Card {
    /// Builds a card for `subject` from already-projected `entries`.
    pub fn new(subject: Ref, entries: Vec<(Symbol, Value)>) -> Self {
        Self { subject, entries }
    }

    /// Returns the subject this card describes.
    pub fn subject(&self) -> &Ref {
        &self.subject
    }

    /// Returns the card's ordered `(predicate, value)` entries.
    pub fn entries(&self) -> &[(Symbol, Value)] {
        &self.entries
    }
}

impl Object for Card {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<card {:?}>", self.subject))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for Card {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = Symbol::qualified("core", "Card");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory().class_stub(CORE_CARD_CLASS_ID, symbol)
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().table(self.entries.clone())
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table(cx)?.object().as_expr(cx)
    }
}

/// Returns the `subject` Card predicate symbol.
pub fn card_subject_predicate() -> Symbol {
    core_symbol("subject")
}

/// Returns the `kind` Card predicate symbol.
pub fn card_kind_predicate() -> Symbol {
    core_symbol("kind")
}

/// Returns the `help` Card predicate symbol.
pub fn card_help_predicate() -> Symbol {
    core_symbol("help")
}

/// Returns the `args` Card predicate symbol.
pub fn card_args_predicate() -> Symbol {
    core_symbol("args")
}

/// Returns the `result` Card predicate symbol.
pub fn card_result_predicate() -> Symbol {
    core_symbol("result")
}

/// Returns the `tests` Card predicate symbol.
pub fn card_tests_predicate() -> Symbol {
    core_symbol("tests")
}

/// Returns the `ops` Card predicate symbol.
pub fn card_ops_predicate() -> Symbol {
    core_symbol("ops")
}

/// Returns the `requires` Card predicate symbol.
pub fn card_requires_predicate() -> Symbol {
    core_symbol("requires")
}

/// Returns the `see-also` Card predicate symbol.
pub fn card_see_also_predicate() -> Symbol {
    core_symbol("see-also")
}

/// Returns the `shape-known` Card predicate symbol.
pub fn card_shape_known_predicate() -> Symbol {
    core_symbol("shape-known")
}

/// Returns the fixed Card predicate symbols in their stable schema order.
pub fn card_fixed_predicates() -> Vec<Symbol> {
    vec![
        card_subject_predicate(),
        card_kind_predicate(),
        card_help_predicate(),
        card_args_predicate(),
        card_result_predicate(),
        card_tests_predicate(),
        card_ops_predicate(),
        card_requires_predicate(),
        card_see_also_predicate(),
        card_shape_known_predicate(),
    ]
}

/// Builds a card carrying only the fixed schema fields with default values.
pub fn minimal_card(cx: &mut Cx, subject: Ref) -> Result<Value> {
    let entries = minimal_entries(cx, &subject)?;
    cx.factory().opaque(Arc::new(Card::new(subject, entries)))
}

/// Builds a card for `subject` by projecting its claims, with no fallback data.
pub fn card_for_ref(cx: &mut Cx, subject: Ref) -> Result<Value> {
    card_from_parts(cx, subject, None, None)
}

/// Builds a card for a runtime `value`, using the value itself as fallback data.
pub fn card_for_value(cx: &mut Cx, value: Value) -> Result<Value> {
    let mut resolver = TemporaryRefResolver::new();
    let subject = resolver.ref_for_value(cx, &value)?;
    card_from_parts(cx, subject, Some(value), None)
}

/// Builds a card for `subject` with optional `fallback` table data and a
/// `default_kind` used when no `kind` claim or fallback is present.
pub fn card_for_ref_with_fallback(
    cx: &mut Cx,
    subject: Ref,
    fallback: Option<Value>,
    default_kind: Option<Symbol>,
) -> Result<Value> {
    card_from_parts(cx, subject, fallback, default_kind)
}

fn card_from_parts(
    cx: &mut Cx,
    subject: Ref,
    fallback: Option<Value>,
    default_kind: Option<Symbol>,
) -> Result<Value> {
    let fallback = fallback_entries(cx, fallback.as_ref())?;
    insert_compatibility_claims(cx, &subject, &fallback, default_kind.as_ref())?;
    let entries = card_entries(cx, &subject, fallback, default_kind)?;
    cx.factory().opaque(Arc::new(Card::new(subject, entries)))
}

fn card_entries(
    cx: &mut Cx,
    subject: &Ref,
    fallback: BTreeMap<Symbol, Value>,
    default_kind: Option<Symbol>,
) -> Result<Vec<(Symbol, Value)>> {
    let args = claim_scalar(cx, subject, card_args_predicate())?
        .or_else(|| fallback.get(&field_symbol("args")).cloned());
    let result = claim_scalar(cx, subject, card_result_predicate())?
        .or_else(|| fallback.get(&field_symbol("result")).cloned());
    let shape_known = claim_scalar(cx, subject, card_shape_known_predicate())?
        .or_else(|| fallback.get(&field_symbol("shape-known")).cloned());

    let kind = match claim_scalar(cx, subject, card_kind_predicate())?
        .or_else(|| fallback.get(&field_symbol("kind")).cloned())
    {
        Some(value) => value,
        None => match default_kind {
            Some(kind) => cx.factory().symbol(kind)?,
            None => core_symbol_value(cx, "unknown")?,
        },
    };
    let help = claim_scalar(cx, subject, card_help_predicate())?
        .or_else(|| fallback.get(&field_symbol("help")).cloned())
        .or_else(|| fallback.get(&field_symbol("purpose")).cloned())
        .unwrap_or(cx.factory().string(String::new())?);
    let tests = claim_list(cx, subject, card_tests_predicate())?
        .or_else(|| fallback.get(&field_symbol("tests")).cloned())
        .unwrap_or(empty_list(cx)?);
    let ops = claim_list(cx, subject, card_ops_predicate())?
        .or_else(|| fallback.get(&field_symbol("ops")).cloned())
        .unwrap_or(empty_list(cx)?);
    let requires = claim_list(cx, subject, card_requires_predicate())?
        .or_else(|| fallback.get(&field_symbol("requires")).cloned())
        .unwrap_or(empty_list(cx)?);
    let see_also = claim_list(cx, subject, card_see_also_predicate())?
        .or_else(|| fallback.get(&field_symbol("see-also")).cloned())
        .unwrap_or(empty_list(cx)?);

    let mut entries = vec![
        (field_symbol("subject"), ref_value(cx, subject)?),
        (field_symbol("kind"), kind),
        (field_symbol("help"), help),
        (
            field_symbol("args"),
            args.clone().unwrap_or(core_symbol_value(cx, "Any")?),
        ),
        (
            field_symbol("result"),
            result.clone().unwrap_or(core_symbol_value(cx, "Any")?),
        ),
        (field_symbol("tests"), tests),
        (field_symbol("ops"), ops),
        (field_symbol("requires"), requires),
        (field_symbol("see-also"), see_also),
        (
            field_symbol("shape-known"),
            shape_known.unwrap_or(cx.factory().bool(args.is_some() || result.is_some())?),
        ),
    ];

    if let Ref::Symbol(symbol) = subject
        && !fallback.contains_key(&field_symbol("symbol"))
    {
        entries.push((
            field_symbol("symbol"),
            cx.factory().string(symbol.to_string())?,
        ));
    }

    for (key, value) in fallback {
        if !entries.iter().any(|(existing, _)| existing == &key) {
            entries.push((key, value));
        }
    }
    Ok(entries)
}

fn minimal_entries(cx: &mut Cx, subject: &Ref) -> Result<Vec<(Symbol, Value)>> {
    Ok(vec![
        (field_symbol("subject"), ref_value(cx, subject)?),
        (field_symbol("kind"), core_symbol_value(cx, "unknown")?),
        (field_symbol("help"), cx.factory().string(String::new())?),
        (field_symbol("args"), core_symbol_value(cx, "Any")?),
        (field_symbol("result"), core_symbol_value(cx, "Any")?),
        (field_symbol("tests"), empty_list(cx)?),
        (field_symbol("ops"), empty_list(cx)?),
        (field_symbol("requires"), empty_list(cx)?),
        (field_symbol("see-also"), empty_list(cx)?),
        (field_symbol("shape-known"), cx.factory().bool(false)?),
    ])
}

fn insert_compatibility_claims(
    cx: &mut Cx,
    subject: &Ref,
    fallback: &BTreeMap<Symbol, Value>,
    default_kind: Option<&Symbol>,
) -> Result<()> {
    let kind = fallback.get(&field_symbol("kind"));
    if let Some(value) = kind {
        insert_value_claim_if_missing(cx, subject, card_kind_predicate(), value)?;
    } else if let Some(kind) = default_kind {
        insert_ref_claim_if_missing(
            cx,
            subject,
            card_kind_predicate(),
            Ref::Symbol(kind.clone()),
        )?;
    }

    if let Some(value) = fallback
        .get(&field_symbol("help"))
        .or_else(|| fallback.get(&field_symbol("purpose")))
    {
        insert_value_claim_if_missing(cx, subject, card_help_predicate(), value)?;
    }
    for (field, predicate) in [
        ("args", card_args_predicate()),
        ("result", card_result_predicate()),
        ("shape-known", card_shape_known_predicate()),
    ] {
        if let Some(value) = fallback.get(&field_symbol(field)) {
            insert_value_claim_if_missing(cx, subject, predicate, value)?;
        }
    }
    for (field, predicate) in [
        ("tests", card_tests_predicate()),
        ("ops", card_ops_predicate()),
        ("requires", card_requires_predicate()),
        ("see-also", card_see_also_predicate()),
    ] {
        if let Some(value) = fallback.get(&field_symbol(field)) {
            insert_list_claims_if_missing(cx, subject, predicate, value)?;
        }
    }
    Ok(())
}

fn insert_value_claim_if_missing(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    value: &Value,
) -> Result<()> {
    let Some(object) = stable_ref_for_value(cx, value)? else {
        return Ok(());
    };
    insert_ref_claim_if_missing(cx, subject, predicate, object)
}

fn insert_list_claims_if_missing(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    value: &Value,
) -> Result<()> {
    if !claims_for(cx, subject, predicate.clone())?.is_empty() {
        return Ok(());
    }
    let Some(list) = value.object().as_list() else {
        return insert_value_claim_if_missing(cx, subject, predicate, value);
    };
    for item in force_list_to_vec(cx, list, "card compatibility claims")? {
        let Some(object) = stable_ref_for_value(cx, &item)? else {
            continue;
        };
        cx.insert_fact(Claim::public(subject.clone(), predicate.clone(), object))?;
    }
    Ok(())
}

fn insert_ref_claim_if_missing(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    object: Ref,
) -> Result<()> {
    if claims_for(cx, subject, predicate.clone())?.is_empty() {
        cx.insert_fact(Claim::public(subject.clone(), predicate, object))?;
    }
    Ok(())
}

fn stable_ref_for_value(cx: &mut Cx, value: &Value) -> Result<Option<Ref>> {
    if let Expr::Symbol(symbol) = value.object().as_expr(cx)? {
        return Ok(Some(Ref::Symbol(symbol)));
    }
    let mut resolver = TemporaryRefResolver::new();
    match resolver.ref_for_value(cx, value)? {
        Ref::Handle(_) => Ok(None),
        reference => Ok(Some(reference)),
    }
}

fn claim_scalar(cx: &mut Cx, subject: &Ref, predicate: Symbol) -> Result<Option<Value>> {
    let claims = claims_for(cx, subject, predicate)?;
    claims
        .first()
        .map(|reference| claim_object_value(cx, reference))
        .transpose()
}

fn claim_list(cx: &mut Cx, subject: &Ref, predicate: Symbol) -> Result<Option<Value>> {
    let claims = claims_for(cx, subject, predicate)?;
    if claims.is_empty() {
        return Ok(None);
    }
    let values = claims
        .iter()
        .map(|reference| claim_object_value(cx, reference))
        .collect::<Result<Vec<_>>>()?;
    cx.factory().list(values).map(Some)
}

fn claims_for(cx: &mut Cx, subject: &Ref, predicate: Symbol) -> Result<Vec<Ref>> {
    let claims = cx.query_facts(ClaimPattern {
        subject: Some(subject.clone()),
        predicate: Some(predicate),
        object: None,
        include_revoked: false,
    })?;
    Ok(claims.into_iter().map(|claim| claim.object).collect())
}

fn claim_object_value(cx: &mut Cx, reference: &Ref) -> Result<Value> {
    match reference {
        Ref::Symbol(symbol) => cx.factory().symbol(symbol.clone()),
        Ref::Content(id) => {
            let datum = cx.datum_store().get(id)?.cloned();
            match datum {
                Some(datum) => value_from_datum(cx, datum),
                None => ref_value(cx, reference),
            }
        }
        Ref::Handle(handle) => match cx.handles().get(handle).cloned() {
            Some(value) => Ok(value),
            None => ref_value(cx, reference),
        },
        Ref::Coord(_) => ref_value(cx, reference),
    }
}

fn fallback_entries(cx: &mut Cx, value: Option<&Value>) -> Result<BTreeMap<Symbol, Value>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };

    let entries = if let Some(table) = value.object().as_table_impl() {
        table.entries(cx)?
    } else {
        let table = value.object().as_table(cx)?;
        match table.object().as_table_impl() {
            Some(table) => table.entries(cx)?,
            None => Vec::new(),
        }
    };

    Ok(entries.into_iter().collect())
}

/// Projects a [`Ref`] into its runtime [`Value`] form: symbols become symbol
/// values; content, handle, and coordinate refs become `core/ref` extension
/// expressions.
pub fn ref_value(cx: &mut Cx, reference: &Ref) -> Result<Value> {
    match reference {
        Ref::Symbol(symbol) => cx.factory().symbol(symbol.clone()),
        Ref::Content(id) => cx.factory().expr(content_ref_expr(id)),
        Ref::Handle(handle) => cx.factory().expr(handle_ref_expr(*handle)),
        Ref::Coord(coordinate) => cx.factory().expr(coord_ref_expr(coordinate)),
    }
}

fn content_ref_expr(id: &ContentId) -> Expr {
    ref_expr(vec![
        (
            Expr::Symbol(field_symbol("kind")),
            Expr::Symbol(core_symbol("content")),
        ),
        (
            Expr::Symbol(field_symbol("algorithm")),
            Expr::Symbol(id.algorithm.clone()),
        ),
        (
            Expr::Symbol(field_symbol("bytes")),
            Expr::Bytes(id.bytes.to_vec()),
        ),
    ])
}

fn handle_ref_expr(handle: HandleId) -> Expr {
    ref_expr(vec![
        (
            Expr::Symbol(field_symbol("kind")),
            Expr::Symbol(core_symbol("handle")),
        ),
        (
            Expr::Symbol(field_symbol("id")),
            Expr::Bytes(handle.0.to_be_bytes().to_vec()),
        ),
    ])
}

fn coord_ref_expr(coordinate: &Coordinate) -> Expr {
    ref_expr(vec![
        (
            Expr::Symbol(field_symbol("kind")),
            Expr::Symbol(core_symbol("coord")),
        ),
        (
            Expr::Symbol(field_symbol("space")),
            Expr::Symbol(coordinate.space.clone()),
        ),
        (
            Expr::Symbol(field_symbol("ordinal")),
            content_ref_expr(&coordinate.ordinal),
        ),
    ])
}

fn ref_expr(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::Extension {
        tag: core_symbol("ref"),
        payload: Box::new(Expr::Map(entries)),
    }
}

fn empty_list(cx: &mut Cx) -> Result<Value> {
    cx.factory().list(Vec::new())
}

fn core_symbol_value(cx: &mut Cx, name: &'static str) -> Result<Value> {
    cx.factory().symbol(core_symbol(name))
}

fn field_symbol(name: &'static str) -> Symbol {
    Symbol::new(name)
}

fn core_symbol(name: &'static str) -> Symbol {
    Symbol::qualified("core", name)
}

#[cfg(test)]
#[path = "card/tests.rs"]
mod tests;
