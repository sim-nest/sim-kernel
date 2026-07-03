//! The [`RefResolver`] contract: turning references back into values.
//!
//! This is protocol the libraries implement; the kernel defines the resolver
//! trait and a temporary in-memory resolver, not the durable resolution policy.

use crate::{
    datum::Datum,
    datum_store::DatumStore,
    env::Cx,
    error::{Error, Result},
    expr::Expr,
    handle_store::HandleStore,
    id::Symbol,
    object::is_default_object_header,
    ref_id::{Coordinate, Ref},
    value::Value,
};

/// Outcome of resolving a [`Ref`] back toward a value or datum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedRef {
    /// The reference named a symbol.
    Symbol(Symbol),
    /// The reference resolved to interned content data.
    Datum(Datum),
    /// The reference resolved to a live runtime value.
    Value(Value),
    /// The reference named a coordinate in some space.
    Coordinate(Coordinate),
    /// The reference could not be resolved in the current context.
    Missing(Ref),
}

/// Contract for assigning references to values and resolving them back.
///
/// This is protocol the libraries implement; the kernel defines the trait and a
/// [`TemporaryRefResolver`], not the durable resolution policy.
pub trait RefResolver {
    /// Returns a stable [`Ref`] for `value`, interning it if needed.
    fn ref_for_value(&mut self, cx: &mut Cx, value: &Value) -> Result<Ref>;
    /// Resolves `reference` to a [`ResolvedRef`] under the current context.
    fn resolve_ref(&self, cx: &mut Cx, reference: &Ref) -> Result<ResolvedRef>;
}

/// In-context [`RefResolver`] backed by the registry, datum store, and handle
/// store; holds no state of its own.
#[derive(Default)]
pub struct TemporaryRefResolver;

impl TemporaryRefResolver {
    /// Creates a resolver.
    pub fn new() -> Self {
        Self
    }
}

impl RefResolver for TemporaryRefResolver {
    fn ref_for_value(&mut self, cx: &mut Cx, value: &Value) -> Result<Ref> {
        if let Some(symbol) = cx.registry().export_symbol_for_value(value) {
            return Ok(Ref::Symbol(symbol));
        }

        if let Some(datum) = value.object().snapshot(cx)? {
            let id = cx.datum_store_mut().intern(datum)?;
            return Ok(Ref::Content(id));
        }

        let header = value.object().header();
        if !is_default_object_header(header) {
            return Ok(header.id.clone());
        }

        let handle = cx.handles_mut().intern(value.clone());
        Ok(Ref::Handle(handle))
    }

    fn resolve_ref(&self, cx: &mut Cx, reference: &Ref) -> Result<ResolvedRef> {
        Ok(match reference {
            Ref::Symbol(symbol) => ResolvedRef::Symbol(symbol.clone()),
            Ref::Coord(coordinate) => ResolvedRef::Coordinate(coordinate.clone()),
            Ref::Handle(handle) => cx.handles().get(handle).cloned().map_or_else(
                || ResolvedRef::Missing(reference.clone()),
                ResolvedRef::Value,
            ),
            Ref::Content(id) => cx.datum_store().get(id)?.cloned().map_or_else(
                || ResolvedRef::Missing(reference.clone()),
                ResolvedRef::Datum,
            ),
        })
    }
}

/// Resolves `reference` to a runtime [`Value`], erroring when it cannot be
/// turned into one (a missing or coordinate reference, or an unresolved symbol).
pub fn value_from_ref(cx: &mut Cx, reference: &Ref) -> Result<Value> {
    match TemporaryRefResolver::new().resolve_ref(cx, reference)? {
        ResolvedRef::Symbol(symbol) => value_from_symbol(cx, &symbol)
            .ok_or_else(|| Error::Eval(format!("unresolved symbol ref {symbol}"))),
        ResolvedRef::Datum(datum) => value_from_datum(cx, datum),
        ResolvedRef::Value(value) => Ok(value),
        ResolvedRef::Coordinate(_) | ResolvedRef::Missing(_) => {
            Err(Error::Eval(format!("unresolved value ref {reference:?}")))
        }
    }
}

/// Builds a runtime [`Value`] from interned content `datum`, mapping symbol-keyed
/// maps to tables and other maps to map expressions.
pub fn value_from_datum(cx: &mut Cx, datum: Datum) -> Result<Value> {
    match datum {
        Datum::Nil => cx.factory().nil(),
        Datum::Bool(value) => cx.factory().bool(value),
        Datum::Number(number) => cx.factory().number_literal(number.domain, number.canonical),
        Datum::Symbol(symbol) => cx.factory().symbol(symbol),
        Datum::String(value) => cx.factory().string(value),
        Datum::Bytes(value) => cx.factory().bytes(value),
        Datum::List(items) | Datum::Vector(items) | Datum::Set(items) => {
            let values = items
                .into_iter()
                .map(|item| value_from_datum(cx, item))
                .collect::<Result<Vec<_>>>()?;
            cx.factory().list(values)
        }
        Datum::Map(entries) => table_from_datum_entries(cx, entries),
        Datum::Node { tag, fields } => cx.factory().expr(Expr::from(Datum::Node { tag, fields })),
    }
}

fn value_from_symbol(cx: &Cx, symbol: &Symbol) -> Option<Value> {
    cx.registry()
        .value_by_symbol(symbol)
        .or_else(|| cx.registry().function_by_symbol(symbol))
        .or_else(|| cx.registry().class_by_symbol(symbol))
        .or_else(|| cx.registry().shape_by_symbol(symbol))
        .or_else(|| cx.registry().codec_by_symbol(symbol))
        .or_else(|| cx.registry().number_domain_by_symbol(symbol))
        .cloned()
}

fn table_from_datum_entries(cx: &mut Cx, entries: Vec<(Datum, Datum)>) -> Result<Value> {
    if entries
        .iter()
        .any(|(key, _)| !matches!(key, Datum::Symbol(_)))
    {
        return cx.factory().expr(Expr::from(Datum::Map(entries)));
    }
    let table_entries = entries
        .into_iter()
        .map(|(key, value)| {
            let Datum::Symbol(symbol) = key else {
                return Err(Error::Eval(
                    "non-symbol map key reached table conversion".to_owned(),
                ));
            };
            Ok((symbol, value_from_datum(cx, value)?))
        })
        .collect::<Result<Vec<_>>>()?;
    cx.factory().table(table_entries)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{ClassRef, Object};

    use crate::testing::bare_cx as cx;

    // sim-non-citizen(reason = "unit-test opaque value fixture", kind = "test-fixture", descriptor = "")
    struct OpaqueValue(&'static str);

    impl Object for OpaqueValue {
        fn display(&self, _cx: &mut Cx) -> Result<String> {
            Ok(format!("#<opaque {}>", self.0))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl crate::ObjectCompat for OpaqueValue {
        fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
            cx.factory().nil()
        }
    }

    #[test]
    fn ref_resolver_returns_symbol_ref_for_registered_function_value() {
        let mut cx = cx();
        let symbol = Symbol::qualified("test", "registered-fn");
        let value = cx.factory().string("registered".to_owned()).unwrap();
        cx.registry_mut()
            .register_function_value(symbol.clone(), value.clone())
            .unwrap();
        let mut resolver = TemporaryRefResolver::new();

        let reference = resolver.ref_for_value(&mut cx, &value).unwrap();

        assert_eq!(reference, Ref::Symbol(symbol));
    }

    #[test]
    fn ref_resolver_returns_content_ref_for_pure_value() {
        let mut cx = cx();
        let value = cx.factory().string("pure".to_owned()).unwrap();
        let mut resolver = TemporaryRefResolver::new();

        let reference = resolver.ref_for_value(&mut cx, &value).unwrap();

        assert!(matches!(reference, Ref::Content(_)), "expected content ref");
        let Ref::Content(id) = reference else {
            return;
        };
        assert!(cx.datum_store().contains(&id));
        let resolved = resolver.resolve_ref(&mut cx, &Ref::Content(id)).unwrap();
        assert_eq!(
            resolved,
            ResolvedRef::Datum(Datum::String("pure".to_owned()))
        );
    }

    #[test]
    fn ref_resolver_returns_handle_ref_for_opaque_value() {
        let mut cx = cx();
        let value = cx.factory().opaque(Arc::new(OpaqueValue("one"))).unwrap();
        let mut resolver = TemporaryRefResolver::new();

        let reference = resolver.ref_for_value(&mut cx, &value).unwrap();

        assert!(matches!(reference, Ref::Handle(_)));
    }

    #[test]
    fn ref_resolver_reuses_handle_for_same_value() {
        let mut cx = cx();
        let value = cx.factory().opaque(Arc::new(OpaqueValue("same"))).unwrap();
        let mut resolver = TemporaryRefResolver::new();

        let first = resolver.ref_for_value(&mut cx, &value).unwrap();
        let second = resolver.ref_for_value(&mut cx, &value).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn ref_resolver_resolves_allocated_handle_to_value() {
        let mut cx = cx();
        let value = cx.factory().opaque(Arc::new(OpaqueValue("value"))).unwrap();
        let mut resolver = TemporaryRefResolver::new();
        let reference = resolver.ref_for_value(&mut cx, &value).unwrap();

        let resolved = resolver.resolve_ref(&mut cx, &reference).unwrap();

        assert_eq!(resolved, ResolvedRef::Value(value));
    }

    #[test]
    fn ref_resolver_assigns_distinct_handles_to_distinct_opaque_values() {
        let mut cx = cx();
        let first_value = cx.factory().opaque(Arc::new(OpaqueValue("first"))).unwrap();
        let second_value = cx
            .factory()
            .opaque(Arc::new(OpaqueValue("second")))
            .unwrap();
        let mut resolver = TemporaryRefResolver::new();

        let first = resolver.ref_for_value(&mut cx, &first_value).unwrap();
        let second = resolver.ref_for_value(&mut cx, &second_value).unwrap();

        assert_ne!(first, second);
        assert!(matches!(first, Ref::Handle(_)));
        assert!(matches!(second, Ref::Handle(_)));
    }

    #[test]
    fn ref_resolver_reports_unknown_content_as_missing() {
        let mut cx = cx();
        let resolver = TemporaryRefResolver::new();
        let reference = Ref::Content(crate::ContentId::from_bytes(
            Symbol::qualified("core", "sha256"),
            [4; 32],
        ));

        let resolved = resolver.resolve_ref(&mut cx, &reference).unwrap();

        assert_eq!(resolved, ResolvedRef::Missing(reference));
    }

    #[test]
    fn ref_resolver_resolves_interned_content_as_datum() {
        let mut cx = cx();
        let datum = Datum::String("stored".to_owned());
        let id = cx.datum_store_mut().intern(datum.clone()).unwrap();
        let resolver = TemporaryRefResolver::new();

        let resolved = resolver.resolve_ref(&mut cx, &Ref::Content(id)).unwrap();

        assert_eq!(resolved, ResolvedRef::Datum(datum));
    }
}
