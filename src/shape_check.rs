use crate::{
    env::Cx,
    error::{Error, Result},
    id::{ShapeId, Symbol},
    object::ShapeRef,
    op::core_symbol,
    ref_id::Ref,
    ref_resolver::{RefResolver, ResolvedRef, TemporaryRefResolver, value_from_datum},
    value::Value,
};

pub(crate) fn check_shape_id(cx: &mut Cx, expected: ShapeId, value: Value) -> Result<Value> {
    let shape_value = cx
        .registry()
        .shape_value(expected)
        .cloned()
        .ok_or(Error::MissingShape(expected))?;
    check_shape_value(cx, &shape_value, Some(expected), value.clone())?;
    Ok(value)
}

pub(crate) fn check_shape_ref(cx: &mut Cx, shape_ref: &Ref, value: Value) -> Result<()> {
    if matches!(shape_ref, Ref::Symbol(symbol) if is_any_shape_symbol(symbol)) {
        return Ok(());
    }
    let (shape_value, expected) = shape_from_ref(cx, shape_ref)?;
    check_shape_value(cx, &shape_value, expected, value)
}

pub(crate) fn check_shape_value(
    cx: &mut Cx,
    shape_value: &ShapeRef,
    expected: Option<ShapeId>,
    value: Value,
) -> Result<()> {
    let shape = shape_value.object().as_shape().ok_or(Error::TypeMismatch {
        expected: "shape",
        found: "non-shape",
    })?;
    let matched = shape.check_value(cx, value)?;
    if matched.accepted {
        Ok(())
    } else {
        Err(Error::WrongShape {
            expected: expected.or_else(|| shape.id()).unwrap_or(ShapeId(0)),
            diagnostics: matched.diagnostics,
        })
    }
}

fn shape_from_ref(cx: &mut Cx, reference: &Ref) -> Result<(ShapeRef, Option<ShapeId>)> {
    match TemporaryRefResolver::new().resolve_ref(cx, reference)? {
        ResolvedRef::Symbol(symbol) => shape_from_symbol(cx, symbol),
        ResolvedRef::Datum(datum) => {
            let value = value_from_datum(cx, datum)?;
            shape_from_value(cx, value, None)
        }
        ResolvedRef::Value(value) => shape_from_value(cx, value, None),
        ResolvedRef::Coordinate(_) | ResolvedRef::Missing(_) => Err(Error::UnresolvedShapeRef {
            reference: Box::new(reference.clone()),
        }),
    }
}

fn shape_from_symbol(cx: &mut Cx, symbol: Symbol) -> Result<(ShapeRef, Option<ShapeId>)> {
    let expected = cx.registry().shapes().get(&symbol).copied();
    let value = cx
        .registry()
        .shape_by_symbol(&symbol)
        .cloned()
        .ok_or(Error::UnknownSymbol { symbol })?;
    shape_from_value(cx, value, expected)
}

fn shape_from_value(
    cx: &mut Cx,
    value: Value,
    expected: Option<ShapeId>,
) -> Result<(ShapeRef, Option<ShapeId>)> {
    if value.object().as_shape().is_some() {
        return Ok((value, expected));
    }
    if let Some(class) = value.object().as_class() {
        return Ok((class.instance_shape(cx)?, expected));
    }
    Err(Error::TypeMismatch {
        expected: "shape",
        found: "non-shape",
    })
}

fn is_any_shape_symbol(symbol: &Symbol) -> bool {
    *symbol == core_symbol("Any") || *symbol == core_symbol("AnyShape")
}
