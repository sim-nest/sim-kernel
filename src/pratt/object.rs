use std::sync::Arc;

use crate::{
    CORE_TABLE_CLASS_ID, ClassRef, Cx, DefaultFactory, Expr, Factory, Fixity, Object, Result,
    Symbol, Value,
};

use super::types::PrattTable;

/// Runtime [`Object`] wrapper exposing a named [`PrattTable`] to the kernel as
/// a core table value.
// sim-non-citizen(reason = "parser table registry marker; reconstruct by loading the parser table lib", kind = "marker", descriptor = "")
#[derive(Clone)]
pub struct PrattTableObject {
    /// Symbol naming this operator table.
    pub symbol: Symbol,
    /// The wrapped operator table.
    pub table: PrattTable,
}

impl PrattTableObject {
    /// Wraps an operator table under the given name.
    pub fn new(symbol: Symbol, table: PrattTable) -> Self {
        Self { symbol, table }
    }
}

impl Object for PrattTableObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<pratt-table {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for PrattTableObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Table"))
        {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(CORE_TABLE_CLASS_ID, Symbol::qualified("core", "Table"))
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let operators = self
            .table
            .operators()
            .into_iter()
            .map(|operator| {
                cx.factory().table(vec![
                    (Symbol::new("symbol"), cx.factory().symbol(operator.symbol)?),
                    (
                        Symbol::new("fixity"),
                        cx.factory().string(
                            match operator.fixity {
                                Fixity::Prefix => "prefix",
                                Fixity::InfixLeft => "infix-left",
                                Fixity::InfixRight => "infix-right",
                                Fixity::Postfix => "postfix",
                                Fixity::Mixfix => "mixfix",
                            }
                            .to_owned(),
                        )?,
                    ),
                    (
                        Symbol::new("left-bp"),
                        cx.factory().number_literal(
                            Symbol::qualified("numbers", "f64"),
                            operator.left_bp.to_string(),
                        )?,
                    ),
                    (
                        Symbol::new("right-bp"),
                        cx.factory().number_literal(
                            Symbol::qualified("numbers", "f64"),
                            operator.right_bp.to_string(),
                        )?,
                    ),
                ])
            })
            .collect::<Result<Vec<_>>>()?;

        cx.factory().table(vec![
            (
                Symbol::new("symbol"),
                cx.factory().symbol(self.symbol.clone())?,
            ),
            (Symbol::new("operators"), cx.factory().list(operators)?),
        ])
    }
}

/// Boxes a named operator table into an opaque runtime [`Value`].
pub fn pratt_table_value(symbol: Symbol, table: PrattTable) -> Value {
    DefaultFactory
        .opaque(Arc::new(PrattTableObject::new(symbol, table)))
        .expect("pratt table object should always be boxable")
}
