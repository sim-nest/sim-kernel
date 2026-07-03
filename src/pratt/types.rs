use std::collections::BTreeMap;

use crate::{Error, Result, Symbol};

/// A single operator entry in a [`PrattTable`]: its symbol, fixity, binding
/// powers, and the expression form it produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrattOperator {
    /// Symbol that triggers this operator during parsing.
    pub symbol: Symbol,
    /// How the operator binds relative to its operands.
    pub fixity: Fixity,
    /// Left binding power, governing how tightly the operator binds to its left.
    pub left_bp: u16,
    /// Right binding power, governing how tightly the operator binds to its right.
    pub right_bp: u16,
    /// Expression form produced when this operator matches.
    pub result: PrattResult,
}

/// The grammatical position an operator occupies relative to its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fixity {
    /// Operator precedes its single operand (for example, unary minus).
    Prefix,
    /// Left-associative infix operator between two operands.
    InfixLeft,
    /// Right-associative infix operator between two operands.
    InfixRight,
    /// Operator follows its single operand (for example, a factorial mark).
    Postfix,
    /// Operator with operands interleaved among multiple fixed parts.
    Mixfix,
}

/// The expression form a matched [`PrattOperator`] produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrattResult {
    /// Build a binary expression from the operator and its two operands.
    ExprInfix,
    /// Build a unary expression from the operator and its trailing operand.
    ExprPrefix,
    /// Build a unary expression from the operator and its leading operand.
    ExprPostfix,
    /// Emit a call to the named callable with the operands as arguments.
    Call(Symbol),
    /// Emit a custom form keyed by the given symbol for the grammar to interpret.
    Custom(Symbol),
}

/// A registry of [`PrattOperator`] entries indexed by fixity for precedence
/// parsing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrattTable {
    prefix: BTreeMap<String, PrattOperator>,
    infix: BTreeMap<String, PrattOperator>,
    postfix: BTreeMap<String, PrattOperator>,
}

impl PrattTable {
    /// Creates an empty table with no registered operators.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an operator under its fixity, replacing any prior entry for
    /// the same symbol; [`Fixity::Mixfix`] operators are not stored.
    pub fn register(&mut self, operator: PrattOperator) {
        let key = operator.symbol.to_string();
        match operator.fixity {
            Fixity::Prefix => {
                self.prefix.insert(key, operator);
            }
            Fixity::InfixLeft | Fixity::InfixRight => {
                self.infix.insert(key, operator);
            }
            Fixity::Postfix => {
                self.postfix.insert(key, operator);
            }
            Fixity::Mixfix => {}
        }
    }

    /// Looks up the null-denotation (prefix) operator for a leading token, if any.
    pub fn lookup_nud(&self, token: &Token) -> Option<PrattOperator> {
        self.prefix.get(token.symbol_text()?).cloned()
    }

    /// Looks up the left-denotation operator for a token, preferring a postfix
    /// entry over an infix one, if any.
    pub fn lookup_led(&self, token: &Token) -> Option<PrattOperator> {
        self.postfix
            .get(token.symbol_text()?)
            .cloned()
            .or_else(|| self.infix.get(token.symbol_text()?).cloned())
    }

    /// Returns the infix operator for the symbol, or an error if none is registered.
    pub fn require_infix(&self, symbol: &Symbol) -> Result<&PrattOperator> {
        self.infix
            .get(&symbol.to_string())
            .ok_or_else(|| Error::Eval(format!("missing infix operator {}", symbol)))
    }

    /// Returns the prefix operator for the symbol, or an error if none is registered.
    pub fn require_prefix(&self, symbol: &Symbol) -> Result<&PrattOperator> {
        self.prefix
            .get(&symbol.to_string())
            .ok_or_else(|| Error::Eval(format!("missing prefix operator {}", symbol)))
    }

    /// Returns the postfix operator for the symbol, or an error if none is registered.
    pub fn require_postfix(&self, symbol: &Symbol) -> Result<&PrattOperator> {
        self.postfix
            .get(&symbol.to_string())
            .ok_or_else(|| Error::Eval(format!("missing postfix operator {}", symbol)))
    }

    /// Collects every registered operator across all fixities.
    pub fn operators(&self) -> Vec<PrattOperator> {
        self.prefix
            .values()
            .chain(self.infix.values())
            .chain(self.postfix.values())
            .cloned()
            .collect()
    }
}

/// A lexical token consumed by the precedence parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// An identifier token carrying its text.
    Ident(String),
    /// A numeric literal token carrying its source text.
    Number(String),
    /// A string literal token carrying its contents.
    String(String),
    /// An opening parenthesis.
    OpenParen,
    /// A closing parenthesis.
    CloseParen,
    /// An argument separator comma.
    Comma,
    /// An operator token carrying its symbol text.
    Operator(String),
}

impl Token {
    /// Returns the symbol text for operator and identifier tokens, or `None`
    /// for tokens that carry no operator-table key.
    pub fn symbol_text(&self) -> Option<&str> {
        match self {
            Self::Operator(text) | Self::Ident(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

/// Parses a raw operator string into a [`Symbol`], splitting on the first `/`
/// or `.` into a qualified namespace and name when present.
pub fn parse_symbol(raw: &str) -> Symbol {
    match raw.split_once('/').or_else(|| raw.split_once('.')) {
        Some((namespace, name)) => Symbol::qualified(namespace.to_owned(), name.to_owned()),
        None => Symbol::new(raw.to_owned()),
    }
}
