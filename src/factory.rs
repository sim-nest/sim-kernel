//! The factory contract: constructing runtime objects from data.
//!
//! This is protocol the libraries implement; the kernel defines the [`Factory`]
//! trait and a default factory, not the concrete construction behavior.

use std::sync::Arc;

use crate::{
    callable::Callable,
    class::Class,
    datum::Datum,
    env::Cx,
    error::{Error, Result},
    expr::{Expr, NumberLiteral},
    id::{
        CORE_BOOL_CLASS_ID, CORE_BYTES_CLASS_ID, CORE_CLASS_CLASS_ID, CORE_EXPR_CLASS_ID,
        CORE_NIL_CLASS_ID, CORE_NUMBER_CLASS_ID, CORE_STRING_CLASS_ID, CORE_SYMBOL_CLASS_ID,
        ClassId, Symbol,
    },
    list::VecList,
    number_domain::NumberValue,
    object::{Args, ClassRef, Object, ShapeRef, TableRef},
    table::AssocTable,
    value::{RuntimeObject, Value},
};

/// Constructs runtime [`Value`]s from kernel data.
///
/// The kernel defines this contract and threads a `&dyn Factory` through every
/// [`Cx`]; libraries supply the concrete object representations the
/// factory hands back. [`DefaultFactory`] provides a minimal built-in
/// implementation for scalars and core collections.
pub trait Factory: Send + Sync {
    /// Wraps an arbitrary runtime object as an opaque value.
    fn opaque(&self, value: Arc<dyn RuntimeObject>) -> Result<Value>;
    /// Builds a class value from a class id and symbol.
    fn class_stub(&self, id: ClassId, symbol: Symbol) -> Result<Value>;
    /// Builds the nil value.
    fn nil(&self) -> Result<Value>;
    /// Builds a boolean value.
    fn bool(&self, value: bool) -> Result<Value>;
    /// Builds a symbol value.
    fn symbol(&self, value: Symbol) -> Result<Value>;
    /// Builds a string value.
    fn string(&self, value: String) -> Result<Value>;
    /// Builds a byte-string value.
    fn bytes(&self, value: Vec<u8>) -> Result<Value>;
    /// Builds a list value from its items.
    fn list(&self, items: Vec<Value>) -> Result<Value>;
    /// Builds a table value from its entries.
    fn table(&self, entries: Vec<(Symbol, Value)>) -> Result<Value>;
    /// Builds a value carrying an [`Expr`].
    fn expr(&self, expr: Expr) -> Result<Value>;
    /// Builds a number value from a domain symbol and canonical text.
    fn number_literal(&self, domain: Symbol, canonical: String) -> Result<Value>;

    /// Builds a single-entry table; convenience over [`table`](Factory::table).
    fn table_one(&self, key: Symbol, value: Value) -> Result<Value> {
        self.table(vec![(key, value)])
    }
}

/// The built-in [`Factory`]: scalars, core collections, and class stubs.
#[derive(Default)]
pub struct DefaultFactory;

impl DefaultFactory {
    fn class_value(id: ClassId, symbol: Symbol) -> Value {
        Value::from_arc(Arc::new(BasicClass { id, symbol }))
    }
}

impl Factory for DefaultFactory {
    fn opaque(&self, value: Arc<dyn RuntimeObject>) -> Result<Value> {
        Ok(Value::from_arc(value))
    }

    fn class_stub(&self, id: ClassId, symbol: Symbol) -> Result<Value> {
        Ok(Self::class_value(id, symbol))
    }

    fn nil(&self) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::Nil)))
    }

    fn bool(&self, value: bool) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::Bool(value))))
    }

    fn symbol(&self, value: Symbol) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::Symbol(value))))
    }

    fn string(&self, value: String) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::String(value))))
    }

    fn bytes(&self, value: Vec<u8>) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::Bytes(value))))
    }

    fn list(&self, items: Vec<Value>) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(VecList::from_vec(items))))
    }

    fn table(&self, entries: Vec<(Symbol, Value)>) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(AssocTable::with_entries(entries))))
    }

    fn expr(&self, expr: Expr) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::Expr(expr))))
    }

    fn number_literal(&self, domain: Symbol, canonical: String) -> Result<Value> {
        Ok(Value::from_arc(Arc::new(BasicObject::Number(
            NumberLiteral { domain, canonical },
        ))))
    }
}

// sim-non-citizen(reason = "private kernel scalar carrier; canonical form is native Expr data", kind = "private", descriptor = "")
#[derive(Clone)]
enum BasicObject {
    Nil,
    Bool(bool),
    Number(NumberLiteral),
    Symbol(Symbol),
    String(String),
    Bytes(Vec<u8>),
    Expr(Expr),
}

impl BasicObject {
    fn class_info(&self) -> (ClassId, Symbol) {
        match self {
            Self::Nil => (CORE_NIL_CLASS_ID, Symbol::qualified("core", "Nil")),
            Self::Bool(_) => (CORE_BOOL_CLASS_ID, Symbol::qualified("core", "Bool")),
            Self::Number(_) => (CORE_NUMBER_CLASS_ID, Symbol::qualified("core", "Number")),
            Self::Symbol(_) => (CORE_SYMBOL_CLASS_ID, Symbol::qualified("core", "Symbol")),
            Self::String(_) => (CORE_STRING_CLASS_ID, Symbol::qualified("core", "String")),
            Self::Bytes(_) => (CORE_BYTES_CLASS_ID, Symbol::qualified("core", "Bytes")),
            Self::Expr(_) => (CORE_EXPR_CLASS_ID, Symbol::qualified("core", "Expr")),
        }
    }
}

impl Object for BasicObject {
    fn snapshot(&self, _cx: &mut Cx) -> Result<Option<Datum>> {
        let datum = match self {
            Self::Nil => Datum::Nil,
            Self::Bool(value) => Datum::Bool(*value),
            Self::Number(value) => Datum::Number(value.clone()),
            Self::Symbol(value) => Datum::Symbol(value.clone()),
            Self::String(value) => Datum::String(value.clone()),
            Self::Bytes(value) => Datum::Bytes(value.clone()),
            Self::Expr(expr) => match Datum::try_from(expr.clone()) {
                Ok(datum) => datum,
                Err(_) => return Ok(None),
            },
        };
        Ok(Some(datum))
    }

    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(match self {
            Self::Nil => "nil".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.canonical.clone(),
            Self::Symbol(value) => value.to_string(),
            Self::String(value) => value.clone(),
            Self::Bytes(value) => format!("{value:?}"),
            Self::Expr(expr) => format!("{expr:?}"),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for BasicObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let (id, symbol) = self.class_info();
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        Ok(DefaultFactory::class_value(id, symbol))
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(match self {
            Self::Nil => Expr::Nil,
            Self::Bool(value) => Expr::Bool(*value),
            Self::Number(value) => Expr::Number(value.clone()),
            Self::Symbol(value) => Expr::Symbol(value.clone()),
            Self::String(value) => Expr::String(value.clone()),
            Self::Bytes(value) => Expr::Bytes(value.clone()),
            Self::Expr(expr) => expr.clone(),
        })
    }
    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(!matches!(self, Self::Nil | Self::Bool(false)))
    }
    fn as_number_value(&self) -> Option<&dyn NumberValue> {
        matches!(self, Self::Number(_)).then_some(self)
    }
}

impl NumberValue for BasicObject {
    fn number_domain(&self, _cx: &mut Cx) -> Result<Symbol> {
        match self {
            Self::Number(number) => Ok(number.domain.clone()),
            _ => Err(Error::TypeMismatch {
                expected: "number",
                found: "non-number",
            }),
        }
    }

    fn number_literal(&self, _cx: &mut Cx) -> Result<Option<NumberLiteral>> {
        Ok(match self {
            Self::Number(number) => Some(number.clone()),
            _ => None,
        })
    }
}

struct BasicClass {
    id: ClassId,
    symbol: Symbol,
}

impl Object for BasicClass {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<class {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for BasicClass {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Class"))
        {
            return Ok(value.clone());
        }
        Ok(DefaultFactory::class_value(
            CORE_CLASS_CLASS_ID,
            Symbol::qualified("core", "Class"),
        ))
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }
}

impl Callable for BasicClass {
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(Error::NonConstructibleClass {
            class: self.symbol.clone(),
        })
    }
}

impl Class for BasicClass {
    fn id(&self) -> ClassId {
        self.id
    }

    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn constructor_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
        Ok(Value::from_arc(Arc::new(BasicObject::Nil)))
    }

    fn instance_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
        Ok(Value::from_arc(Arc::new(BasicObject::Nil)))
    }

    fn read_constructor(&self, _cx: &mut Cx) -> Result<Option<crate::object::ReadConstructorRef>> {
        Ok(None)
    }

    fn members(&self, _cx: &mut Cx) -> Result<TableRef> {
        Ok(Value::from_arc(Arc::new(AssocTable::new())))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{CORE_CLASS_CLASS_ID, DefaultFactory, Error, NoopEvalPolicy, Symbol, object::Args};

    #[test]
    fn class_stub_reports_non_constructible_constructor() {
        let mut cx = crate::Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let symbol = Symbol::qualified("core", "Class");
        let class = cx
            .factory()
            .class_stub(CORE_CLASS_CLASS_ID, symbol.clone())
            .unwrap();

        assert!(class.object().as_class().is_some());
        assert!(class.object().as_callable().is_some());

        let error = cx.call_value(class, Args::new(Vec::new())).unwrap_err();
        assert!(matches!(
            error,
            Error::NonConstructibleClass { class } if class == symbol
        ));
    }
}
