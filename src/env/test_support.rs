use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::{ClassRef, Expr, NumberDomain, NumberLiteral, NumberValue, Object, Symbol, Value};

use super::Cx;

impl Cx {
    /// Builds a minimal context with no-op eval policy and the default factory,
    /// for use as a fixture in kernel unit tests.
    pub fn stub() -> Self {
        use crate::{eval::NoopEvalPolicy, factory::DefaultFactory};

        Self::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(0x5445_5354),
        )
    }
}

// sim-non-citizen(reason = "unit-test number-domain fixture", kind = "test-fixture", descriptor = "")
#[derive(Clone)]
struct TestNumberDomain {
    symbol: Symbol,
    priority: i32,
}

impl NumberDomain for TestNumberDomain {
    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn parse_priority(&self) -> i32 {
        self.priority
    }

    fn parse_literal(&self, _cx: &mut Cx, _text: &str) -> crate::Result<Option<Value>> {
        Ok(None)
    }

    fn encode_literal(&self, cx: &mut Cx, value: Value) -> crate::Result<Option<NumberLiteral>> {
        match value.object().as_expr(cx)? {
            Expr::Number(number) if number.domain == self.symbol => Ok(Some(number)),
            _ => Ok(None),
        }
    }
}

impl Object for TestNumberDomain {
    fn display(&self, _cx: &mut Cx) -> crate::Result<String> {
        Ok(format!("#<number-domain {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for TestNumberDomain {
    fn class(&self, cx: &mut Cx) -> crate::Result<ClassRef> {
        cx.factory().class_stub(
            crate::id::CORE_NUMBER_DOMAIN_CLASS_ID,
            Symbol::qualified("core", "NumberDomain"),
        )
    }
    fn as_number_domain(&self) -> Option<&dyn NumberDomain> {
        Some(self)
    }
}

pub(super) fn domain_value(cx: &mut Cx, symbol: Symbol, priority: i32) -> Value {
    cx.factory()
        .opaque(Arc::new(TestNumberDomain { symbol, priority }))
        .expect("test number domain should be boxable")
}

pub(super) fn promote_to_middle(
    _cx: &mut Cx,
    number: NumberLiteral,
) -> crate::Result<NumberLiteral> {
    Ok(NumberLiteral {
        domain: Symbol::qualified("numbers", "middle-test"),
        canonical: number.canonical,
    })
}

pub(super) fn promote_middle_to_target(
    _cx: &mut Cx,
    number: NumberLiteral,
) -> crate::Result<NumberLiteral> {
    Ok(NumberLiteral {
        domain: Symbol::qualified("numbers", "target-test"),
        canonical: number.canonical,
    })
}

pub(super) fn unreachable_binary_rule(
    cx: &mut Cx,
    _left: NumberLiteral,
    _right: NumberLiteral,
) -> crate::Result<Value> {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "target-test"), "0".to_owned())
}

pub(super) static HIGH_COST_BINARY_APPLIES: AtomicUsize = AtomicUsize::new(0);
pub(super) static WINNING_BINARY_APPLIES: AtomicUsize = AtomicUsize::new(0);
pub(super) static WINNING_UNARY_APPLIES: AtomicUsize = AtomicUsize::new(0);
pub(super) static WINNING_REDUCTION_APPLIES: AtomicUsize = AtomicUsize::new(0);
pub(super) static NUMBER_DISPATCH_TEST_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn reset_apply_counters() {
    HIGH_COST_BINARY_APPLIES.store(0, Ordering::SeqCst);
    WINNING_BINARY_APPLIES.store(0, Ordering::SeqCst);
    WINNING_UNARY_APPLIES.store(0, Ordering::SeqCst);
    WINNING_REDUCTION_APPLIES.store(0, Ordering::SeqCst);
}

pub(super) fn number(domain: &str, canonical: &str) -> NumberLiteral {
    NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: canonical.to_owned(),
    }
}

pub(super) fn register_test_domain(cx: &mut Cx, name: &str) {
    let symbol = Symbol::qualified("numbers", name);
    let value = domain_value(cx, symbol.clone(), 0);
    cx.registry_mut()
        .register_number_domain_value(symbol, value)
        .unwrap();
}

// sim-non-citizen(reason = "unit-test opaque number fixture", kind = "test-fixture", descriptor = "")
#[derive(Clone)]
pub(super) struct OpaqueNumber {
    pub(super) domain: Symbol,
    pub(super) value: i64,
}

impl NumberValue for OpaqueNumber {
    fn number_domain(&self, _cx: &mut Cx) -> crate::Result<Symbol> {
        Ok(self.domain.clone())
    }
}

impl Object for OpaqueNumber {
    fn display(&self, _cx: &mut Cx) -> crate::Result<String> {
        Ok(format!("{}:{}", self.domain, self.value))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for OpaqueNumber {
    fn class(&self, cx: &mut Cx) -> crate::Result<ClassRef> {
        cx.factory().class_stub(
            crate::id::CORE_NUMBER_CLASS_ID,
            Symbol::qualified("core", "Number"),
        )
    }
    fn as_number_value(&self) -> Option<&dyn NumberValue> {
        Some(self)
    }
}

pub(super) fn opaque_number(cx: &Cx, domain: &str, value: i64) -> Value {
    cx.factory()
        .opaque(Arc::new(OpaqueNumber {
            domain: Symbol::qualified("numbers", domain),
            value,
        }))
        .unwrap()
}

pub(super) fn read_opaque_number(value: &Value) -> &OpaqueNumber {
    value.object().downcast_ref::<OpaqueNumber>().unwrap()
}

pub(super) fn promote_value_to_middle(cx: &mut Cx, value: Value) -> crate::Result<Value> {
    let value = read_opaque_number(&value);
    Ok(opaque_number(cx, "value-middle-test", value.value))
}

pub(super) fn promote_value_middle_to_target(cx: &mut Cx, value: Value) -> crate::Result<Value> {
    let value = read_opaque_number(&value);
    Ok(opaque_number(cx, "value-target-test", value.value))
}

pub(super) fn promote_value_to_alt_target(cx: &mut Cx, value: Value) -> crate::Result<Value> {
    let value = read_opaque_number(&value);
    Ok(opaque_number(cx, "value-alt-target-test", value.value))
}

pub(super) fn value_binary_rule(cx: &mut Cx, left: Value, right: Value) -> crate::Result<Value> {
    let left = read_opaque_number(&left).value;
    let right = read_opaque_number(&right).value;
    Ok(opaque_number(cx, "value-target-test", left + right))
}

pub(super) fn value_binary_alt_rule(
    cx: &mut Cx,
    left: Value,
    right: Value,
) -> crate::Result<Value> {
    let left = read_opaque_number(&left).value;
    let right = read_opaque_number(&right).value;
    Ok(opaque_number(cx, "value-alt-target-test", left + right))
}

pub(super) fn high_cost_binary_rule(
    cx: &mut Cx,
    _left: NumberLiteral,
    _right: NumberLiteral,
) -> crate::Result<Value> {
    HIGH_COST_BINARY_APPLIES.fetch_add(1, Ordering::SeqCst);
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "slow-test"), "slow".to_owned())
}

pub(super) fn winning_binary_rule(
    cx: &mut Cx,
    _left: NumberLiteral,
    _right: NumberLiteral,
) -> crate::Result<Value> {
    WINNING_BINARY_APPLIES.fetch_add(1, Ordering::SeqCst);
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "fast-test"), "fast".to_owned())
}

pub(super) fn winning_unary_rule(cx: &mut Cx, _operand: NumberLiteral) -> crate::Result<Value> {
    WINNING_UNARY_APPLIES.fetch_add(1, Ordering::SeqCst);
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "fast-test"), "neg".to_owned())
}

pub(super) fn winning_reduction_rule(
    cx: &mut Cx,
    _operands: Vec<NumberLiteral>,
) -> crate::Result<Value> {
    WINNING_REDUCTION_APPLIES.fetch_add(1, Ordering::SeqCst);
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "fast-test"), "sum".to_owned())
}
