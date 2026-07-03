//! The number-domain contract: the pluggable numeric backend protocol.
//!
//! The kernel defines the [`NumberDomain`] protocol, value and operation
//! contracts, and promotion rules; concrete number domains and arithmetic are
//! libs loaded against it, never kernel-fixed behavior.

use crate::{
    env::Cx,
    error::Result,
    expr::NumberLiteral,
    id::Symbol,
    value::{RuntimeObject, Value},
};

/// Bounds on the promotion-path search between number domains.
///
/// # Examples
///
/// ```
/// use sim_kernel::PromotionSearchLimits;
///
/// let limits = PromotionSearchLimits::default();
/// assert_eq!(limits.max_depth, 8);
/// assert_eq!(limits.max_states, 256);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromotionSearchLimits {
    /// Maximum number of conversion hops to chain.
    pub max_depth: usize,
    /// Maximum number of intermediate domain states to explore.
    pub max_states: usize,
}

impl Default for PromotionSearchLimits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_states: 256,
        }
    }
}

/// A literal-level conversion edge from one number domain to another.
#[derive(Clone)]
pub struct PromotionRule {
    /// Domain the rule converts from.
    pub from_domain: Symbol,
    /// Domain the rule converts to.
    pub to_domain: Symbol,
    /// Relative cost, used to rank competing promotion paths.
    pub cost: u16,
    /// Converts a literal in `from_domain` to one in `to_domain`.
    pub convert: fn(&mut Cx, NumberLiteral) -> Result<NumberLiteral>,
}

/// A literal-level binary operator keyed on both operand domains.
#[derive(Clone)]
pub struct NumberBinaryOp {
    /// Operator symbol this rule implements.
    pub operator: Symbol,
    /// Required domain of the left operand.
    pub left_domain: Symbol,
    /// Required domain of the right operand.
    pub right_domain: Symbol,
    /// Relative cost, used to rank competing implementations.
    pub cost: u16,
    /// Applies the operator to two domain literals.
    pub apply: fn(&mut Cx, NumberLiteral, NumberLiteral) -> Result<Value>,
}

/// A literal-level unary operator keyed on its operand domain.
#[derive(Clone)]
pub struct NumberUnaryOp {
    /// Operator symbol this rule implements.
    pub operator: Symbol,
    /// Required domain of the operand.
    pub operand_domain: Symbol,
    /// Relative cost, used to rank competing implementations.
    pub cost: u16,
    /// Applies the operator to a single domain literal.
    pub apply: fn(&mut Cx, NumberLiteral) -> Result<Value>,
}

/// A literal-level reduction over a homogeneous list of operands.
#[derive(Clone)]
pub struct NumberReductionOp {
    /// Operator symbol this rule implements.
    pub operator: Symbol,
    /// Required domain of every operand.
    pub operand_domain: Symbol,
    /// Relative cost, used to rank competing implementations.
    pub cost: u16,
    /// Folds the operator over a vector of domain literals.
    pub apply: fn(&mut Cx, Vec<NumberLiteral>) -> Result<Value>,
}

/// A runtime number value paired with its domain and optional literal form.
#[derive(Clone)]
pub struct NumberValueRef {
    /// Domain symbol the value belongs to.
    pub domain: Symbol,
    /// The runtime value.
    pub value: Value,
    /// Canonical literal form, when one is available.
    pub literal: Option<NumberLiteral>,
}

/// Protocol for runtime objects that present themselves as numbers.
pub trait NumberValue: Send + Sync {
    /// The number domain this value reports membership in.
    fn number_domain(&self, cx: &mut Cx) -> Result<Symbol>;

    /// Canonical literal form, or `Ok(None)` when not representable.
    fn number_literal(&self, _cx: &mut Cx) -> Result<Option<NumberLiteral>> {
        Ok(None)
    }
}

/// A value-level conversion edge from one number domain to another.
#[derive(Clone)]
pub struct ValuePromotionRule {
    /// Domain the rule converts from.
    pub from_domain: Symbol,
    /// Domain the rule converts to.
    pub to_domain: Symbol,
    /// Relative cost, used to rank competing promotion paths.
    pub cost: u16,
    /// Converts a value in `from_domain` to one in `to_domain`.
    pub convert: fn(&mut Cx, Value) -> Result<Value>,
}

/// A value-level binary operator keyed on both operand domains.
#[derive(Clone)]
pub struct ValueNumberBinaryOp {
    /// Operator symbol this rule implements.
    pub operator: Symbol,
    /// Required domain of the left operand.
    pub left_domain: Symbol,
    /// Required domain of the right operand.
    pub right_domain: Symbol,
    /// Relative cost, used to rank competing implementations.
    pub cost: u16,
    /// Applies the operator to two domain values.
    pub apply: fn(&mut Cx, Value, Value) -> Result<Value>,
}

/// A value-level unary operator keyed on its operand domain.
#[derive(Clone)]
pub struct ValueNumberUnaryOp {
    /// Operator symbol this rule implements.
    pub operator: Symbol,
    /// Required domain of the operand.
    pub operand_domain: Symbol,
    /// Relative cost, used to rank competing implementations.
    pub cost: u16,
    /// Applies the operator to a single domain value.
    pub apply: fn(&mut Cx, Value) -> Result<Value>,
}

/// A value-level reduction over a homogeneous list of operands.
#[derive(Clone)]
pub struct ValueNumberReductionOp {
    /// Operator symbol this rule implements.
    pub operator: Symbol,
    /// Required domain of every operand.
    pub operand_domain: Symbol,
    /// Relative cost, used to rank competing implementations.
    pub cost: u16,
    /// Folds the operator over a vector of domain values.
    pub apply: fn(&mut Cx, Vec<Value>) -> Result<Value>,
}

/// Runtime protocol for a numeric semantic domain.
///
/// A number domain owns parsing, canonical encoding, and promotion rules for a
/// family of number literals. Domains are runtime objects so they can be
/// registered, reflected, browsed, and exported like other SIM values.
pub trait NumberDomain: RuntimeObject {
    /// Symbol uniquely identifying this domain.
    fn symbol(&self) -> Symbol;

    /// Tie-breaking priority when several domains can parse the same text.
    fn parse_priority(&self) -> i32 {
        0
    }

    /// Parses literal text into a domain value, or `Ok(None)` if not ours.
    fn parse_literal(&self, cx: &mut Cx, text: &str) -> Result<Option<Value>>;

    /// Encodes a value back to a canonical literal, or `Ok(None)` if not ours.
    fn encode_literal(&self, cx: &mut Cx, value: Value) -> Result<Option<NumberLiteral>>;

    /// Promotion rules originating from this domain; empty by default.
    fn promotions(&self) -> Vec<PromotionRule> {
        Vec::new()
    }
}
