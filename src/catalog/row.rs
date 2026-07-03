use std::{collections::BTreeMap, fmt, sync::Arc};

use crate::{
    Expr, NumberBinaryOp, NumberReductionOp, NumberUnaryOp, PromotionRule, Symbol, Test, Value,
    ValueNumberBinaryOp, ValueNumberReductionOp, ValueNumberUnaryOp, ValuePromotionRule,
};

/// A single catalog row: deterministic [`Expr`] field data plus private live
/// payload cells.
///
/// The `data` map is serializable and participates in snapshots and deltas; the
/// live cells (runtime values, tests, number ops, promotion rules) are host-only
/// payloads kept out of serialized row data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRow {
    /// Table this row belongs to.
    pub table: Symbol,
    /// Key identifying the row within its table.
    pub key: Symbol,
    /// Catalog epoch at which the row was last written.
    pub epoch: u64,
    /// Deterministic, serializable field data.
    pub data: BTreeMap<Symbol, Expr>,
    pub(crate) live: BTreeMap<Symbol, CatalogLiveCell>,
}

impl CatalogRow {
    /// Creates an empty row at epoch 0 for `table`/`key`.
    pub fn new(table: Symbol, key: Symbol) -> Self {
        Self {
            table,
            key,
            epoch: 0,
            data: BTreeMap::new(),
            live: BTreeMap::new(),
        }
    }

    /// Returns the row with one deterministic data field set.
    pub fn with_data(mut self, field: Symbol, value: Expr) -> Self {
        self.data.insert(field, value);
        self
    }

    /// Stores a live runtime [`Value`] payload in `field`.
    pub fn insert_live_value(&mut self, field: Symbol, value: Value) {
        self.live.insert(field, CatalogLiveCell::Value(value));
    }

    /// Returns the live [`Value`] payload in `field`, if present.
    pub fn live_value(&self, field: &Symbol) -> Option<&Value> {
        match self.live.get(field) {
            Some(CatalogLiveCell::Value(value)) => Some(value),
            _ => None,
        }
    }

    /// Stores a live [`Test`] payload in `field`.
    pub fn insert_live_test(&mut self, field: Symbol, test: Arc<dyn Test>) {
        self.live.insert(field, CatalogLiveCell::Test(test));
    }

    /// Returns the live [`Test`] payload in `field`, if present.
    pub fn live_test(&self, field: &Symbol) -> Option<&Arc<dyn Test>> {
        match self.live.get(field) {
            Some(CatalogLiveCell::Test(test)) => Some(test),
            _ => None,
        }
    }

    /// Stores a live [`NumberUnaryOp`] payload in `field`.
    pub fn insert_live_number_unary_op(&mut self, field: Symbol, op: NumberUnaryOp) {
        self.live.insert(field, CatalogLiveCell::NumberUnaryOp(op));
    }

    /// Stores a live [`NumberReductionOp`] payload in `field`.
    pub fn insert_live_number_reduction_op(&mut self, field: Symbol, op: NumberReductionOp) {
        self.live
            .insert(field, CatalogLiveCell::NumberReductionOp(op));
    }

    /// Stores a live [`NumberBinaryOp`] payload in `field`.
    pub fn insert_live_number_binary_op(&mut self, field: Symbol, op: NumberBinaryOp) {
        self.live.insert(field, CatalogLiveCell::NumberBinaryOp(op));
    }

    /// Stores a live [`ValueNumberUnaryOp`] payload in `field`.
    pub fn insert_live_value_number_unary_op(&mut self, field: Symbol, op: ValueNumberUnaryOp) {
        self.live
            .insert(field, CatalogLiveCell::ValueNumberUnaryOp(op));
    }

    /// Stores a live [`ValueNumberReductionOp`] payload in `field`.
    pub fn insert_live_value_number_reduction_op(
        &mut self,
        field: Symbol,
        op: ValueNumberReductionOp,
    ) {
        self.live
            .insert(field, CatalogLiveCell::ValueNumberReductionOp(op));
    }

    /// Stores a live [`ValueNumberBinaryOp`] payload in `field`.
    pub fn insert_live_value_number_binary_op(&mut self, field: Symbol, op: ValueNumberBinaryOp) {
        self.live
            .insert(field, CatalogLiveCell::ValueNumberBinaryOp(op));
    }

    /// Stores a live [`PromotionRule`] payload in `field`.
    pub fn insert_live_promotion_rule(&mut self, field: Symbol, rule: PromotionRule) {
        self.live
            .insert(field, CatalogLiveCell::PromotionRule(rule));
    }

    /// Stores a live [`ValuePromotionRule`] payload in `field`.
    pub fn insert_live_value_promotion_rule(&mut self, field: Symbol, rule: ValuePromotionRule) {
        self.live
            .insert(field, CatalogLiveCell::ValuePromotionRule(rule));
    }

    pub(crate) fn has_field(&self, field: &Symbol) -> bool {
        self.data.contains_key(field) || self.live.contains_key(field)
    }

    pub(crate) fn set_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
    }
}

#[derive(Clone)]
pub(crate) enum CatalogLiveCell {
    Value(Value),
    Test(Arc<dyn Test>),
    NumberUnaryOp(NumberUnaryOp),
    NumberReductionOp(NumberReductionOp),
    NumberBinaryOp(NumberBinaryOp),
    ValueNumberUnaryOp(ValueNumberUnaryOp),
    ValueNumberReductionOp(ValueNumberReductionOp),
    ValueNumberBinaryOp(ValueNumberBinaryOp),
    PromotionRule(PromotionRule),
    ValuePromotionRule(ValuePromotionRule),
}

impl fmt::Debug for CatalogLiveCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(value) => f.debug_tuple("Value").field(value).finish(),
            Self::Test(_) => f.write_str("Test(..)"),
            Self::NumberUnaryOp(_) => f.write_str("NumberUnaryOp(..)"),
            Self::NumberReductionOp(_) => f.write_str("NumberReductionOp(..)"),
            Self::NumberBinaryOp(_) => f.write_str("NumberBinaryOp(..)"),
            Self::ValueNumberUnaryOp(_) => f.write_str("ValueNumberUnaryOp(..)"),
            Self::ValueNumberReductionOp(_) => f.write_str("ValueNumberReductionOp(..)"),
            Self::ValueNumberBinaryOp(_) => f.write_str("ValueNumberBinaryOp(..)"),
            Self::PromotionRule(_) => f.write_str("PromotionRule(..)"),
            Self::ValuePromotionRule(_) => f.write_str("ValuePromotionRule(..)"),
        }
    }
}

impl PartialEq for CatalogLiveCell {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Value(left), Self::Value(right)) => left == right,
            (Self::Test(left), Self::Test(right)) => Arc::ptr_eq(left, right),
            (Self::NumberUnaryOp(left), Self::NumberUnaryOp(right)) => {
                number_unary_op_eq(left, right)
            }
            (Self::NumberReductionOp(left), Self::NumberReductionOp(right)) => {
                number_reduction_op_eq(left, right)
            }
            (Self::NumberBinaryOp(left), Self::NumberBinaryOp(right)) => {
                number_binary_op_eq(left, right)
            }
            (Self::ValueNumberUnaryOp(left), Self::ValueNumberUnaryOp(right)) => {
                value_number_unary_op_eq(left, right)
            }
            (Self::ValueNumberReductionOp(left), Self::ValueNumberReductionOp(right)) => {
                value_number_reduction_op_eq(left, right)
            }
            (Self::ValueNumberBinaryOp(left), Self::ValueNumberBinaryOp(right)) => {
                value_number_binary_op_eq(left, right)
            }
            (Self::PromotionRule(left), Self::PromotionRule(right)) => {
                promotion_rule_eq(left, right)
            }
            (Self::ValuePromotionRule(left), Self::ValuePromotionRule(right)) => {
                value_promotion_rule_eq(left, right)
            }
            _ => false,
        }
    }
}

impl Eq for CatalogLiveCell {}

fn number_unary_op_eq(left: &NumberUnaryOp, right: &NumberUnaryOp) -> bool {
    left.operator == right.operator
        && left.operand_domain == right.operand_domain
        && left.cost == right.cost
}

fn number_reduction_op_eq(left: &NumberReductionOp, right: &NumberReductionOp) -> bool {
    left.operator == right.operator
        && left.operand_domain == right.operand_domain
        && left.cost == right.cost
}

fn number_binary_op_eq(left: &NumberBinaryOp, right: &NumberBinaryOp) -> bool {
    left.operator == right.operator
        && left.left_domain == right.left_domain
        && left.right_domain == right.right_domain
        && left.cost == right.cost
}

fn value_number_unary_op_eq(left: &ValueNumberUnaryOp, right: &ValueNumberUnaryOp) -> bool {
    left.operator == right.operator
        && left.operand_domain == right.operand_domain
        && left.cost == right.cost
}

fn value_number_reduction_op_eq(
    left: &ValueNumberReductionOp,
    right: &ValueNumberReductionOp,
) -> bool {
    left.operator == right.operator
        && left.operand_domain == right.operand_domain
        && left.cost == right.cost
}

fn value_number_binary_op_eq(left: &ValueNumberBinaryOp, right: &ValueNumberBinaryOp) -> bool {
    left.operator == right.operator
        && left.left_domain == right.left_domain
        && left.right_domain == right.right_domain
        && left.cost == right.cost
}

fn promotion_rule_eq(left: &PromotionRule, right: &PromotionRule) -> bool {
    left.from_domain == right.from_domain
        && left.to_domain == right.to_domain
        && left.cost == right.cost
}

fn value_promotion_rule_eq(left: &ValuePromotionRule, right: &ValuePromotionRule) -> bool {
    left.from_domain == right.from_domain
        && left.to_domain == right.to_domain
        && left.cost == right.cost
}
