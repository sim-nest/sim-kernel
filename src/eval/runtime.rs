use crate::{
    env::Cx,
    error::{Error, Result},
    eval::{Demand, Phase},
    expr::Expr,
    id::ClassId,
    value::Value,
};

fn class_id_of(cx: &mut Cx, value: &Value) -> Result<Option<ClassId>> {
    let class = value.object().class(cx)?;
    Ok(class.object().as_class().map(|class| class.id()))
}

/// Forces `value` to satisfy `demand` using the kernel's default rules.
///
/// This is the reference [`force`](crate::EvalPolicy::force) implementation:
/// it unwraps thunks, coerces to expression or boolean forms, and checks
/// class membership as the [`Demand`] requires. The standard eval policies
/// delegate to it; libraries may reuse or replace it.
pub fn force_default(cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
    match demand {
        Demand::Never => Ok(value),
        Demand::Expr => {
            let expr = value.object().as_expr(cx)?;
            cx.factory().expr(expr)
        }
        Demand::Value => {
            if let Some(thunk) = value.object().as_thunk() {
                thunk.force(cx, demand)
            } else {
                Ok(value)
            }
        }
        Demand::Bool => {
            let forced = if let Some(thunk) = value.object().as_thunk() {
                thunk.force(cx, demand)?
            } else {
                value
            };
            let truth = forced.object().truth(cx)?;
            cx.factory().bool(truth)
        }
        Demand::Class(expected) => {
            let forced = if let Some(thunk) = value.object().as_thunk() {
                thunk.force(cx, demand)?
            } else {
                value
            };
            let found = class_id_of(cx, &forced)?.ok_or(Error::TypeMismatch {
                expected: "class-backed value",
                found: "non-class-backed value",
            })?;
            if found == expected {
                Ok(forced)
            } else {
                Err(Error::WrongClass { expected, found })
            }
        }
        Demand::Shape(_) => {
            if let Some(thunk) = value.object().as_thunk() {
                thunk.force(cx, demand)
            } else {
                Ok(value)
            }
        }
    }
}

/// Evaluates `expr` to a value using the kernel's default tree-walk.
///
/// This is the reference [`eval_expr`](crate::EvalPolicy::eval_expr)
/// implementation: it expands macros for [`Phase::Eval`](crate::Phase::Eval),
/// then walks the [`Expr`] graph, resolving symbols and dispatching calls.
/// The standard eval policies delegate to it; libraries may reuse or replace
/// it with their own evaluation semantics.
pub fn eval_expr_default(cx: &mut Cx, expr: Expr) -> Result<Value> {
    let expr = cx.expand_macros(Phase::Eval, expr)?;
    match expr {
        Expr::Nil => cx.factory().nil(),
        Expr::Bool(value) => cx.factory().bool(value),
        Expr::Number(number) => cx.factory().number_literal(number.domain, number.canonical),
        Expr::Symbol(symbol) => {
            if let Some(value) = cx.env().get(&symbol) {
                Ok(value.clone())
            } else if let Ok(function) = cx.resolve_function(&symbol) {
                Ok(function)
            } else if let Ok(class) = cx.resolve_class(&symbol) {
                Ok(class)
            } else if let Ok(shape) = cx.resolve_shape(&symbol) {
                Ok(shape)
            } else if let Ok(value) = cx.resolve_value(&symbol) {
                Ok(value)
            } else {
                cx.resolve_unbound_symbol(symbol)
            }
        }
        Expr::Local(symbol) => cx.factory().expr(Expr::Local(symbol)),
        Expr::String(value) => cx.factory().string(value),
        Expr::Bytes(value) => cx.factory().bytes(value),
        Expr::List(items) => {
            let values = items
                .into_iter()
                .map(|item| eval_expr_default(cx, item))
                .collect::<Result<Vec<_>>>()?;
            cx.factory().list(values)
        }
        Expr::Vector(items) => {
            let values = items
                .into_iter()
                .map(|item| eval_expr_default(cx, item))
                .collect::<Result<Vec<_>>>()?;
            cx.factory().list(values)
        }
        Expr::Map(entries) => {
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    let key_expr = eval_expr_default(cx, key)?.object().as_expr(cx)?;
                    let Expr::Symbol(key_symbol) = key_expr else {
                        return Err(Error::TypeMismatch {
                            expected: "symbol key",
                            found: "non-symbol key",
                        });
                    };
                    Ok((key_symbol, eval_expr_default(cx, value)?))
                })
                .collect::<Result<Vec<_>>>()?;
            cx.factory().table(entries)
        }
        Expr::Set(items) => {
            let values = items
                .into_iter()
                .map(|item| eval_expr_default(cx, item))
                .collect::<Result<Vec<_>>>()?;
            cx.factory().list(values)
        }
        Expr::Call { operator, args } => {
            if let Expr::Symbol(name) = operator.as_ref()
                && !cx.symbol_is_bound(name)
            {
                cx.resolve_unbound_call(name.clone(), args)
            } else {
                let callable = cx.eval_expr(*operator)?;
                cx.call_exprs(callable, args)
            }
        }
        Expr::Block(items) => {
            let mut last = cx.factory().nil()?;
            for item in items {
                last = eval_expr_default(cx, item)?;
            }
            Ok(last)
        }
        Expr::Quote { expr, .. } => cx.factory().expr(*expr),
        Expr::Annotated { expr, .. } => eval_expr_default(cx, *expr),
        Expr::Extension { .. }
        | Expr::Infix { .. }
        | Expr::Prefix { .. }
        | Expr::Postfix { .. } => cx.factory().expr(expr),
    }
}
