//! The [`Expr`] graph: the checked-form representation of source.
//!
//! The kernel defines the expression graph, number literals, quote modes, and
//! source-origin tracking; general-purpose codecs in library crates round-trip
//! every expression through this shared graph.

use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

use crate::id::{CodecId, Symbol};

/// The codec-neutral expression graph: the checked form of source.
///
/// The kernel defines this graph; general-purpose codecs in library crates
/// round-trip every expression through it. It is the `checked forms` stage of
/// the data flow `tokens -> checked forms -> objects -> checked calls ->
/// objects -> encoded forms`. Equality and hashing are canonical (see
/// [`Expr::canonical_eq`]): map entries and set members compare order-insensitively.
///
/// # Examples
///
/// ```
/// # use sim_kernel::expr::Expr;
/// # use sim_kernel::id::Symbol;
/// let call = Expr::Call {
///     operator: Box::new(Expr::Symbol(Symbol::new("add"))),
///     args: vec![Expr::Bool(true), Expr::Nil],
/// };
/// // Maps compare canonically regardless of entry order.
/// let a = Expr::Set(vec![Expr::Bool(true), Expr::Nil]);
/// let b = Expr::Set(vec![Expr::Nil, Expr::Bool(true)]);
/// assert!(a.canonical_eq(&b));
/// let _ = call;
/// ```
#[derive(Clone, Debug)]
pub enum Expr {
    /// The nil literal.
    Nil,
    /// A boolean literal.
    Bool(bool),
    /// A number literal in some domain.
    Number(NumberLiteral),
    /// A symbol reference.
    Symbol(Symbol),
    /// A lexical local reference.
    Local(Symbol),
    /// A string literal.
    String(String),
    /// A byte-string literal.
    Bytes(Vec<u8>),
    /// An ordered list form.
    List(Vec<Expr>),
    /// An ordered vector form.
    Vector(Vec<Expr>),
    /// A map form of key/value pairs (order-insensitive when compared).
    Map(Vec<(Expr, Expr)>),
    /// A set form (order-insensitive when compared).
    Set(Vec<Expr>),
    /// A call of `operator` with positional `args`.
    Call {
        /// The expression producing the callable operator.
        operator: Box<Expr>,
        /// The positional argument expressions.
        args: Vec<Expr>,
    },
    /// An infix operator application.
    Infix {
        /// The operator symbol.
        operator: Symbol,
        /// The left operand.
        left: Box<Expr>,
        /// The right operand.
        right: Box<Expr>,
    },
    /// A prefix operator application.
    Prefix {
        /// The operator symbol.
        operator: Symbol,
        /// The operand.
        arg: Box<Expr>,
    },
    /// A postfix operator application.
    Postfix {
        /// The operator symbol.
        operator: Symbol,
        /// The operand.
        arg: Box<Expr>,
    },
    /// A sequenced block of expressions.
    Block(Vec<Expr>),
    /// A quoted expression carried at a given quote mode.
    Quote {
        /// The quote mode (quote, quasiquote, ...).
        mode: QuoteMode,
        /// The quoted expression.
        expr: Box<Expr>,
    },
    /// An expression carrying named annotations.
    Annotated {
        /// The annotated inner expression.
        expr: Box<Expr>,
        /// The name/value annotation pairs.
        annotations: Vec<(Symbol, Expr)>,
    },
    /// An open extension form with a tag and payload.
    Extension {
        /// The extension tag.
        tag: Symbol,
        /// The extension payload expression.
        payload: Box<Expr>,
    },
}

/// A number literal: a domain symbol plus its canonical textual form.
///
/// The kernel carries the literal verbatim; concrete number domains and
/// arithmetic are supplied by libraries.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NumberLiteral {
    /// The number domain naming how `canonical` is interpreted.
    pub domain: Symbol,
    /// The canonical textual representation of the number.
    pub canonical: String,
}

/// The quoting mode attached to a [`Expr::Quote`] form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuoteMode {
    /// Plain quotation.
    Quote,
    /// Quasiquotation, allowing nested unquotes.
    QuasiQuote,
    /// Unquote within a quasiquote.
    Unquote,
    /// Splicing unquote within a quasiquote.
    Splice,
    /// Hygienic syntax quotation.
    Syntax,
}

/// An [`Expr`] paired with its optional source [`Origin`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocatedExpr {
    /// The expression.
    pub expr: Expr,
    /// The optional source origin.
    pub origin: Option<Origin>,
}

impl LocatedExpr {
    /// Compares two located expressions canonically, ignoring origin.
    pub fn canonical_eq(&self, other: &Self) -> bool {
        self.expr.canonical_eq(&other.expr)
    }
}

/// A located expression together with its located children.
///
/// Carries per-node [`Origin`] for a whole expression tree, so codecs can
/// preserve source spans down to each sub-form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocatedExprTree {
    /// The expression at this node.
    pub expr: Expr,
    /// The optional source origin of this node.
    pub origin: Option<Origin>,
    /// The located child nodes.
    pub children: Vec<LocatedExprTree>,
}

impl LocatedExprTree {
    /// Compares two trees canonically, ignoring origin, including children.
    pub fn canonical_eq(&self, other: &Self) -> bool {
        self.expr.canonical_eq(&other.expr)
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(other.children.iter())
                .all(|(left, right)| left.canonical_eq(right))
    }

    /// Projects this node to a flat [`LocatedExpr`], dropping children.
    pub fn located(&self) -> LocatedExpr {
        LocatedExpr {
            expr: self.expr.clone(),
            origin: self.origin.clone(),
        }
    }

    /// Builds a leaf tree node with no children.
    pub fn without_children(expr: Expr, origin: Option<Origin>) -> Self {
        Self {
            expr,
            origin,
            children: Vec::new(),
        }
    }

    /// Lifts a flat [`LocatedExpr`] into a leaf tree node.
    pub fn from_located(located: LocatedExpr) -> Self {
        Self {
            expr: located.expr,
            origin: located.origin,
            children: Vec::new(),
        }
    }

    /// Builds a tree from `expr` by recursively wrapping its children, with no origins.
    pub fn from_expr_recursive(expr: Expr) -> Self {
        let children = expr_children(&expr)
            .into_iter()
            .map(Self::from_expr_recursive)
            .collect();
        Self {
            expr,
            origin: None,
            children,
        }
    }
}

fn expr_children(expr: &Expr) -> Vec<Expr> {
    match expr {
        Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Symbol(_)
        | Expr::Local(_)
        | Expr::String(_)
        | Expr::Bytes(_) => Vec::new(),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.clone()
        }
        Expr::Map(entries) => entries
            .iter()
            .flat_map(|(key, value)| [key.clone(), value.clone()])
            .collect(),
        Expr::Call { operator, args } => std::iter::once(operator.as_ref().clone())
            .chain(args.iter().cloned())
            .collect(),
        Expr::Infix { left, right, .. } => vec![left.as_ref().clone(), right.as_ref().clone()],
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => vec![arg.as_ref().clone()],
        Expr::Quote { expr, .. } => vec![expr.as_ref().clone()],
        Expr::Annotated { expr, annotations } => std::iter::once(expr.as_ref().clone())
            .chain(annotations.iter().map(|(_, value)| value.clone()))
            .collect(),
        Expr::Extension { payload, .. } => vec![payload.as_ref().clone()],
    }
}

/// Source provenance of an expression: which codec, source, span, and trivia.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    /// The codec that read the expression.
    pub codec: CodecId,
    /// The source the expression was read from.
    pub source: SourceId,
    /// The byte span within the source.
    pub span: Span,
    /// The surrounding whitespace and comment trivia.
    pub trivia: Vec<Trivia>,
}

/// Opaque identifier of a registered source.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub String);

/// A store mapping each [`SourceId`] to its raw source bytes.
///
/// Lets diagnostics and lossless codecs recover original text by [`Origin`].
#[derive(Clone, Debug, Default)]
pub struct SourceRegistry {
    sources: BTreeMap<SourceId, Arc<[u8]>>,
}

impl SourceRegistry {
    /// Inserts source bytes, returning any previous bytes for the same id.
    pub fn insert(&mut self, source: SourceId, bytes: Arc<[u8]>) -> Option<Arc<[u8]>> {
        self.sources.insert(source, bytes)
    }

    /// Interns raw bytes under `source`, replacing any prior entry.
    pub fn intern_bytes(&mut self, source: SourceId, bytes: impl Into<Arc<[u8]>>) {
        self.sources.insert(source, bytes.into());
    }

    /// Interns text under `source` as its UTF-8 bytes.
    pub fn intern_text(&mut self, source: SourceId, text: &str) {
        self.intern_bytes(source, Arc::<[u8]>::from(text.as_bytes()));
    }

    /// Splices `bytes` into the stored source at the origin's span.
    pub fn intern_span(&mut self, origin: &Origin, bytes: &[u8]) {
        let required_len = origin.span.end.max(bytes.len());
        let mut merged = self
            .sources
            .get(&origin.source)
            .map(|existing| existing.as_ref().to_vec())
            .unwrap_or_default();
        if merged.len() < required_len {
            merged.resize(required_len, 0);
        }
        let end = origin.span.start.saturating_add(bytes.len());
        if end <= merged.len() {
            merged[origin.span.start..end].copy_from_slice(bytes);
            self.sources
                .insert(origin.source.clone(), Arc::<[u8]>::from(merged));
        }
    }

    /// Returns the stored bytes for `source`, if any.
    pub fn get(&self, source: &SourceId) -> Option<&Arc<[u8]>> {
        self.sources.get(source)
    }

    /// Returns the byte slice covered by `origin`'s span, if available.
    pub fn slice(&self, origin: &Origin) -> Option<&[u8]> {
        self.sources
            .get(&origin.source)
            .and_then(|bytes| bytes.get(origin.span.start..origin.span.end))
    }
}

/// A half-open byte range `[start, end)` within a source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    /// The inclusive start byte offset.
    pub start: usize,
    /// The exclusive end byte offset.
    pub end: usize,
}

/// Non-semantic source trivia preserved alongside an expression.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Trivia {
    /// Whitespace text.
    Whitespace(String),
    /// A single-line comment.
    LineComment(String),
    /// A block comment.
    BlockComment(String),
}

/// A normalized, comparable key derived from an [`Expr`].
///
/// Backs canonical equality and hashing: structurally equal expressions
/// (modulo map/set ordering) produce equal keys. Each variant tags its shape
/// with a static label so unlike shapes never collide.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanonicalKey {
    /// A tagged atom with no payload.
    Atom(&'static str),
    /// A tagged boolean.
    Bool(&'static str, bool),
    /// Tagged raw bytes.
    Bytes(&'static str, Vec<u8>),
    /// A tagged string.
    String(&'static str, String),
    /// A tagged symbol.
    Symbol(&'static str, Symbol),
    /// A tagged pair of strings.
    Pair(&'static str, String, String),
    /// A tagged ordered sequence of sub-keys.
    Compound(&'static str, Vec<CanonicalKey>),
    /// A tagged sequence of name/sub-key pairs.
    CompoundNamed(&'static str, Vec<(String, CanonicalKey)>),
}

impl CanonicalKey {
    fn tag(tag: &'static str) -> Self {
        Self::Atom(tag)
    }

    fn with_bool(tag: &'static str, value: bool) -> Self {
        Self::Bool(tag, value)
    }

    fn with_bytes(tag: &'static str, value: &[u8]) -> Self {
        Self::Bytes(tag, value.to_vec())
    }

    fn with_string(tag: &'static str, value: &str) -> Self {
        Self::String(tag, value.to_owned())
    }

    fn with_symbol(tag: &'static str, symbol: &Symbol) -> Self {
        Self::Symbol(tag, symbol.clone())
    }

    fn with_pair(tag: &'static str, left: String, right: String) -> Self {
        Self::Pair(tag, left, right)
    }

    fn compound(tag: &'static str, items: Vec<CanonicalKey>) -> Self {
        Self::Compound(tag, items)
    }

    fn compound_named(tag: &'static str, items: Vec<(String, CanonicalKey)>) -> Self {
        Self::CompoundNamed(tag, items)
    }
}

impl Expr {
    /// Computes the [`CanonicalKey`] backing canonical equality and hashing.
    pub fn canonical_key(&self) -> CanonicalKey {
        match self {
            Self::Nil => CanonicalKey::tag("nil"),
            Self::Bool(value) => CanonicalKey::with_bool("bool", *value),
            Self::Number(value) => {
                CanonicalKey::with_pair("number", value.domain.to_string(), value.canonical.clone())
            }
            Self::Symbol(symbol) => CanonicalKey::with_symbol("symbol", symbol),
            Self::Local(symbol) => CanonicalKey::with_symbol("local", symbol),
            Self::String(value) => CanonicalKey::with_string("string", value),
            Self::Bytes(value) => CanonicalKey::with_bytes("bytes", value),
            Self::List(items) => {
                CanonicalKey::compound("list", items.iter().map(Self::canonical_key).collect())
            }
            Self::Vector(items) => {
                CanonicalKey::compound("vector", items.iter().map(Self::canonical_key).collect())
            }
            Self::Map(entries) => {
                let mut items = entries
                    .iter()
                    .map(|(key, value)| {
                        CanonicalKey::compound(
                            "entry",
                            vec![key.canonical_key(), value.canonical_key()],
                        )
                    })
                    .collect::<Vec<_>>();
                items.sort();
                CanonicalKey::compound("map", items)
            }
            Self::Set(items) => {
                let mut items = items.iter().map(Self::canonical_key).collect::<Vec<_>>();
                items.sort();
                CanonicalKey::compound("set", items)
            }
            Self::Call { operator, args } => CanonicalKey::compound(
                "call",
                std::iter::once(operator.canonical_key())
                    .chain(args.iter().map(Self::canonical_key))
                    .collect(),
            ),
            Self::Infix {
                operator,
                left,
                right,
            } => CanonicalKey::compound_named(
                "infix",
                vec![
                    (
                        "operator".to_owned(),
                        CanonicalKey::with_symbol("symbol", operator),
                    ),
                    ("left".to_owned(), left.canonical_key()),
                    ("right".to_owned(), right.canonical_key()),
                ],
            ),
            Self::Prefix { operator, arg } => CanonicalKey::compound_named(
                "prefix",
                vec![
                    (
                        "operator".to_owned(),
                        CanonicalKey::with_symbol("symbol", operator),
                    ),
                    ("arg".to_owned(), arg.canonical_key()),
                ],
            ),
            Self::Postfix { operator, arg } => CanonicalKey::compound_named(
                "postfix",
                vec![
                    (
                        "operator".to_owned(),
                        CanonicalKey::with_symbol("symbol", operator),
                    ),
                    ("arg".to_owned(), arg.canonical_key()),
                ],
            ),
            Self::Block(items) => {
                CanonicalKey::compound("block", items.iter().map(Self::canonical_key).collect())
            }
            Self::Quote { mode, expr } => CanonicalKey::compound_named(
                "quote",
                vec![
                    (
                        "mode".to_owned(),
                        CanonicalKey::with_string("quote-mode", &format!("{mode:?}")),
                    ),
                    ("expr".to_owned(), expr.canonical_key()),
                ],
            ),
            Self::Annotated { expr, annotations } => CanonicalKey::compound_named(
                "annotated",
                std::iter::once(("expr".to_owned(), expr.canonical_key()))
                    .chain(
                        annotations
                            .iter()
                            .map(|(symbol, expr)| (symbol.to_string(), expr.canonical_key())),
                    )
                    .collect(),
            ),
            Self::Extension { tag, payload } => CanonicalKey::compound_named(
                "extension",
                vec![
                    ("tag".to_owned(), CanonicalKey::with_symbol("symbol", tag)),
                    ("payload".to_owned(), payload.canonical_key()),
                ],
            ),
        }
    }

    /// Returns whether two expressions are canonically equal.
    pub fn canonical_eq(&self, other: &Self) -> bool {
        self.canonical_key() == other.canonical_key()
    }
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_key() == other.canonical_key()
    }
}

impl Eq for Expr {}

impl Hash for Expr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.canonical_key().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use crate::id::Symbol;

    use super::{Expr, NumberLiteral};

    #[test]
    fn map_and_set_canonical_order_is_normalized() {
        let key_a = Expr::Symbol(Symbol::new("a"));
        let key_b = Expr::Symbol(Symbol::new("b"));
        let value = Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1.0".to_owned(),
        });

        let left = Expr::Map(vec![
            (key_a.clone(), value.clone()),
            (key_b.clone(), Expr::Nil),
        ]);
        let right = Expr::Map(vec![(key_b, Expr::Nil), (key_a, value)]);
        assert!(left.canonical_eq(&right));

        let left = Expr::Set(vec![Expr::String("x".to_owned()), Expr::Bool(true)]);
        let right = Expr::Set(vec![Expr::Bool(true), Expr::String("x".to_owned())]);
        assert!(left.canonical_eq(&right));
    }

    #[test]
    fn expr_hash_matches_canonical_map_equality() {
        let left = Expr::Map(vec![
            (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
            (Expr::Symbol(Symbol::new("b")), Expr::Nil),
        ]);
        let right = Expr::Map(vec![
            (Expr::Symbol(Symbol::new("b")), Expr::Nil),
            (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
        ]);

        assert_eq!(hash_expr(&left), hash_expr(&right));
        assert_eq!(left, right);
    }

    #[test]
    fn expr_hash_matches_canonical_set_equality() {
        let left = Expr::Set(vec![Expr::String("x".to_owned()), Expr::Bool(true)]);
        let right = Expr::Set(vec![Expr::Bool(true), Expr::String("x".to_owned())]);

        assert_eq!(hash_expr(&left), hash_expr(&right));
        assert_eq!(left, right);
    }

    fn hash_expr(expr: &Expr) -> u64 {
        let mut hasher = DefaultHasher::new();
        expr.hash(&mut hasher);
        hasher.finish()
    }
}
