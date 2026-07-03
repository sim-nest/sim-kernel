//! The [`Term`] contract: the checked form that bridges expressions and data.
//!
//! The kernel defines the term representation and its operation keys; it sits
//! on the path from checked forms to objects, and libraries interpret terms.

use crate::{
    datum::Datum,
    error::{Error, Result},
    expr::{Expr, NumberLiteral, QuoteMode},
    id::Symbol,
    ref_id::{ContentId, Coordinate, HandleId, Ref},
};

/// Stable operation identity used by lowered `Term::Op` nodes.
///
/// Lowered `Term::Op` nodes carry only the key value so terms can express
/// explicit op invocations. Operation specs, dispatch, adapters, and
/// capability gates are resolved by the op layer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpKey {
    /// The operation's namespace.
    pub namespace: Symbol,
    /// The operation's name within the namespace.
    pub name: Symbol,
    /// The operation's version.
    pub version: u16,
}

impl OpKey {
    /// Builds an op key from a namespace, name, and version.
    pub fn new(namespace: Symbol, name: Symbol, version: u16) -> Self {
        Self {
            namespace,
            name,
            version,
        }
    }
}

/// Lowered executable form.
///
/// Current `Expr` values lower as follows:
///
/// - scalar data -> `Term::Literal`
/// - `Expr::Symbol` -> `Term::Ref(Ref::Symbol)`
/// - `Expr::Local` -> `Term::Local`
/// - data-only list, vector, map, and set -> `Term::Literal`
/// - call -> `Term::Call`
/// - infix, prefix, and postfix -> `Term::Call` with a symbolic target
/// - block -> `Term::Seq`
/// - quote -> `Term::Quote` over pure `Datum`
/// - annotated -> `Term::Annotated` with datum annotations
/// - extension -> `Term::Extension` with a datum payload
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Term {
    /// A constant datum value.
    Literal(Datum),
    /// A reference resolved through the registry or substrate.
    Ref(Ref),
    /// A lexical local binding referenced by name.
    Local(Symbol),
    /// A local binding form: bind `name` to `value` within `body`.
    Let {
        /// The bound name.
        name: Symbol,
        /// The term producing the bound value.
        value: Box<Term>,
        /// The body evaluated with the binding in scope.
        body: Box<Term>,
    },
    /// An explicit operation invocation over one input term.
    Op {
        /// The operation key naming the op.
        op: OpKey,
        /// The input term the op consumes.
        input: Box<Term>,
    },
    /// A call of `target` with positional `args`.
    Call {
        /// The term producing the callable target.
        target: Box<Term>,
        /// The positional argument terms.
        args: Vec<Term>,
    },
    /// A sequence of terms evaluated in order.
    Seq(Vec<Term>),
    /// A quoted datum carried at a given quote mode.
    Quote {
        /// The quote mode (quote, quasiquote, ...).
        mode: QuoteMode,
        /// The quoted datum.
        datum: Datum,
    },
    /// A term carrying datum-valued annotations.
    Annotated {
        /// The annotated inner term.
        term: Box<Term>,
        /// The name/datum annotation pairs.
        annotations: Vec<(Symbol, Datum)>,
    },
    /// An open extension term with a tag and datum payload.
    Extension {
        /// The extension tag.
        tag: Symbol,
        /// The extension payload datum.
        payload: Datum,
    },
}

impl Term {
    /// Lowers a checked [`Expr`] into its executable [`Term`] form.
    ///
    /// This walks the expression graph and produces the lowered representation
    /// described on [`Term`]; it fails when a sub-expression cannot become a
    /// pure [`Datum`].
    pub fn lower(expr: Expr) -> Result<Self> {
        match expr {
            Expr::Nil => Ok(Self::Literal(Datum::Nil)),
            Expr::Bool(value) => Ok(Self::Literal(Datum::Bool(value))),
            Expr::Number(value) => Ok(Self::Literal(Datum::Number(value))),
            Expr::Symbol(symbol) => Ok(Self::Ref(Ref::Symbol(symbol))),
            Expr::Local(symbol) => Ok(Self::Local(symbol)),
            Expr::String(value) => Ok(Self::Literal(Datum::String(value))),
            Expr::Bytes(value) => Ok(Self::Literal(Datum::Bytes(value))),
            Expr::List(items) => data_literal(Expr::List(items)),
            Expr::Vector(items) => data_literal(Expr::Vector(items)),
            Expr::Map(entries) => data_literal(Expr::Map(entries)),
            Expr::Set(items) => data_literal(Expr::Set(items)),
            Expr::Call { operator, args } => Ok(Self::Call {
                target: Box::new(Self::lower(*operator)?),
                args: lower_terms(args)?,
            }),
            Expr::Infix {
                operator,
                left,
                right,
            } => Ok(Self::Call {
                target: Box::new(Self::Ref(Ref::Symbol(operator))),
                args: vec![Self::lower(*left)?, Self::lower(*right)?],
            }),
            Expr::Prefix { operator, arg } | Expr::Postfix { operator, arg } => Ok(Self::Call {
                target: Box::new(Self::Ref(Ref::Symbol(operator))),
                args: vec![Self::lower(*arg)?],
            }),
            Expr::Block(items) => Ok(Self::Seq(lower_terms(items)?)),
            Expr::Quote { mode, expr } => Ok(Self::Quote {
                mode,
                datum: Datum::try_from(*expr)?,
            }),
            Expr::Annotated { expr, annotations } => {
                let annotations = annotations
                    .into_iter()
                    .map(|(name, expr)| Ok((name, Datum::try_from(expr)?)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Annotated {
                    term: Box::new(Self::lower(*expr)?),
                    annotations,
                })
            }
            Expr::Extension { tag, payload } => Ok(Self::Extension {
                tag,
                payload: Datum::try_from(*payload)?,
            }),
        }
    }
}

impl TryFrom<Expr> for Term {
    type Error = Error;

    fn try_from(expr: Expr) -> Result<Self> {
        Self::lower(expr)
    }
}

impl From<Term> for Expr {
    fn from(term: Term) -> Self {
        match term {
            Term::Literal(datum) => Self::from(datum),
            Term::Ref(reference) => ref_to_expr(reference),
            Term::Local(symbol) => Self::Local(symbol),
            Term::Let { name, value, body } => Self::Extension {
                tag: core_symbol("term-let"),
                payload: Box::new(Self::Map(vec![
                    (Self::Symbol(Symbol::new("name")), Self::Symbol(name)),
                    (Self::Symbol(Symbol::new("value")), Self::from(*value)),
                    (Self::Symbol(Symbol::new("body")), Self::from(*body)),
                ])),
            },
            Term::Op { op, input } => Self::Extension {
                tag: core_symbol("term-op"),
                payload: Box::new(Self::Map(vec![
                    (
                        Self::Symbol(Symbol::new("op")),
                        Self::from(op_key_datum(op)),
                    ),
                    (Self::Symbol(Symbol::new("input")), Self::from(*input)),
                ])),
            },
            Term::Call { target, args } => Self::Call {
                operator: Box::new(Self::from(*target)),
                args: args.into_iter().map(Self::from).collect(),
            },
            Term::Seq(items) => Self::Block(items.into_iter().map(Self::from).collect()),
            Term::Quote { mode, datum } => Self::Quote {
                mode,
                expr: Box::new(Self::from(datum)),
            },
            Term::Annotated { term, annotations } => Self::Annotated {
                expr: Box::new(Self::from(*term)),
                annotations: annotations
                    .into_iter()
                    .map(|(name, datum)| (name, Self::from(datum)))
                    .collect(),
            },
            Term::Extension { tag, payload } => Self::Extension {
                tag,
                payload: Box::new(Self::from(payload)),
            },
        }
    }
}

fn lower_terms(exprs: Vec<Expr>) -> Result<Vec<Term>> {
    exprs.into_iter().map(Term::lower).collect()
}

fn data_literal(expr: Expr) -> Result<Term> {
    Datum::try_from(expr).map(Term::Literal)
}

fn ref_to_expr(reference: Ref) -> Expr {
    match reference {
        Ref::Symbol(symbol) => Expr::Symbol(symbol),
        other => Expr::from(ref_datum(other)),
    }
}

fn ref_datum(reference: Ref) -> Datum {
    match reference {
        Ref::Symbol(symbol) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("symbol"))),
                (Symbol::new("symbol"), Datum::Symbol(symbol)),
            ],
        },
        Ref::Content(content) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("content"))),
                (Symbol::new("content"), content_id_datum(content)),
            ],
        },
        Ref::Handle(handle) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("handle"))),
                (Symbol::new("id"), handle_id_datum(handle)),
            ],
        },
        Ref::Coord(coordinate) => coordinate_datum(coordinate),
    }
}

fn coordinate_datum(coordinate: Coordinate) -> Datum {
    Datum::Node {
        tag: core_symbol("ref"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(core_symbol("coord"))),
            (Symbol::new("space"), Datum::Symbol(coordinate.space)),
            (Symbol::new("ordinal"), content_id_datum(coordinate.ordinal)),
        ],
    }
}

fn content_id_datum(content: ContentId) -> Datum {
    Datum::Node {
        tag: core_symbol("content-id"),
        fields: vec![
            (Symbol::new("algorithm"), Datum::Symbol(content.algorithm)),
            (Symbol::new("bytes"), Datum::Bytes(content.bytes.to_vec())),
        ],
    }
}

fn handle_id_datum(handle: HandleId) -> Datum {
    Datum::Bytes(handle.0.to_be_bytes().to_vec())
}

fn op_key_datum(op: OpKey) -> Datum {
    Datum::Node {
        tag: core_symbol("op-key"),
        fields: vec![
            (Symbol::new("namespace"), Datum::Symbol(op.namespace)),
            (Symbol::new("name"), Datum::Symbol(op.name)),
            (
                Symbol::new("version"),
                Datum::Number(NumberLiteral {
                    domain: core_symbol("u16"),
                    canonical: op.version.to_string(),
                }),
            ),
        ],
    }
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}
