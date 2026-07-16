//! The [`Shape`] protocol: the one shared engine for matching and binding.
//!
//! Shape is a first-class kernel protocol used across parsing, checking,
//! binding, dispatch, macro syntax, codec grammar, and overload selection; the
//! kernel defines the protocol and the match/binding contracts, while concrete
//! shapes are implemented by the libraries.

use std::{any::Any, collections::BTreeSet, sync::Arc};

use crate::{
    callable::Callable,
    env::{Cx, Env},
    error::{Diagnostic, Error, Result},
    expr::Expr,
    hint::{HintMetadata, diagnostic_hints_value},
    id::{CORE_SHAPE_CLASS_ID, ShapeId, Symbol},
    object::{Args, ClassRef, Object, RawArgs, ShapeRef},
    value::Value,
};

/// The one shared engine for matching and binding across the runtime.
///
/// `Shape` is among the kernel's most important contracts: a single protocol
/// reused for parsing, checking, binding, dispatch, macro syntax, codec
/// grammar, lambda locals, and overload selection. It is a first-class kernel
/// protocol -- object-accessible through [`as_shape`](crate::ObjectCompat::as_shape),
/// callable as a matcher (every `Shape` is a [`Callable`]), and subclassable
/// through open metadata rather than a closed enum.
///
/// The kernel defines only this protocol and the match/binding contracts
/// ([`ShapeMatch`], [`ShapeBindings`], [`MatchScore`], [`ShapeDoc`]). Concrete
/// grammars and matchers are supplied by libraries; SIM is a Rust runtime, not
/// a Lisp runtime, so no particular surface syntax is baked in here.
///
/// A type checks a value with [`check_value`](Shape::check_value) and an
/// expression with [`check_expr`](Shape::check_expr); both yield a
/// [`ShapeMatch`]. [`describe`](Shape::describe) provides the human-facing
/// [`ShapeDoc`]. The remaining methods carry optional identity and subshape
/// metadata and default to neutral answers.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Value};
/// use sim_kernel::shape::{MatchScore, Shape, ShapeDoc, ShapeMatch};
///
/// struct AnyShape;
/// impl Shape for AnyShape {
///     fn check_value(&self, _cx: &mut Cx, _v: Value) -> sim_kernel::Result<ShapeMatch> {
///         Ok(ShapeMatch::accept(MatchScore::exact(1)))
///     }
///     fn check_expr(&self, _cx: &mut Cx, _e: &sim_kernel::Expr) -> sim_kernel::Result<ShapeMatch> {
///         Ok(ShapeMatch::accept(MatchScore::exact(1)))
///     }
///     fn describe(&self, _cx: &mut Cx) -> sim_kernel::Result<ShapeDoc> {
///         Ok(ShapeDoc::new("any"))
///     }
/// }
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let value = cx.factory().string("ok".to_owned()).unwrap();
/// let matched = AnyShape.check_value(&mut cx, value).unwrap();
/// assert!(matched.accepted);
/// ```
pub trait Shape: Callable {
    /// Stable [`ShapeId`] when this shape has runtime identity, else `None`.
    fn id(&self) -> Option<ShapeId> {
        None
    }

    /// Symbol naming this shape when it has one, else `None`.
    fn symbol(&self) -> Option<Symbol> {
        None
    }

    /// Parent shapes for the subshape walk; empty by default.
    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        Ok(Vec::new())
    }

    /// Whether matching this shape may run effects; `false` by default.
    fn is_effectful(&self) -> bool {
        false
    }

    /// Whether this shape accepts every input in its domain; `false` by default.
    fn is_total(&self) -> bool {
        false
    }

    /// Return a concrete implication answer when this shape owns the semantics.
    ///
    /// `None` keeps the kernel on the generic id, symbol, Any, and parent walk
    /// path instead of requiring a closed enum of every concrete shape kind.
    fn is_subshape_of(&self, _cx: &mut Cx, _parent: &dyn Shape) -> Result<Option<bool>> {
        Ok(None)
    }

    /// Check a [`Value`] against this shape, yielding a [`ShapeMatch`].
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch>;
    /// Check an [`Expr`] against this shape, yielding a [`ShapeMatch`].
    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch>;
    /// Produce the human-facing [`ShapeDoc`] for this shape.
    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc>;
}

impl<T> Object for T
where
    T: Shape + Any,
{
    fn display(&self, cx: &mut Cx) -> Result<String> {
        let doc = self.describe(cx)?;
        match self.symbol() {
            Some(symbol) => Ok(format!("#<shape {} {}>", symbol, doc.name)),
            None => Ok(format!("#<shape {}>", doc.name)),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T> crate::ObjectCompat for T
where
    T: Shape + Any,
{
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = Symbol::qualified("core", "Shape");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory().class_stub(CORE_SHAPE_CLASS_ID, symbol)
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let doc = self.describe(cx)?;
        let mut entries = vec![
            (Symbol::new("name"), cx.factory().string(doc.name)?),
            (
                Symbol::new("effectful"),
                cx.factory().bool(self.is_effectful())?,
            ),
            (Symbol::new("total"), cx.factory().bool(self.is_total())?),
        ];
        if let Some(symbol) = self.symbol() {
            entries.push((
                Symbol::new("symbol"),
                cx.factory().string(symbol.to_string())?,
            ));
        }
        for (index, detail) in doc.details.into_iter().enumerate() {
            entries.push((
                Symbol::qualified("detail", index.to_string()),
                cx.factory().string(detail)?,
            ));
        }
        cx.factory().table(entries)
    }
    fn as_shape(&self) -> Option<&dyn Shape> {
        Some(self)
    }
}

impl<T> Callable for T
where
    T: Shape,
{
    /// Calls a shape as a matcher over exactly one already-evaluated value.
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let [value] = args.values() else {
            return Err(Error::Eval("shape call expects 1 argument".to_owned()));
        };
        call_shape(cx, self, ShapeCallTarget::Value(value.clone()))
    }

    /// Calls a shape as a matcher over exactly one unevaluated expression.
    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let [expr] = args.exprs() else {
            return Err(Error::Eval("shape call expects 1 expression".to_owned()));
        };
        call_shape(cx, self, ShapeCallTarget::Expr(expr.clone()))
    }
}

/// Human-facing description of a [`Shape`]: a name plus optional detail lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeDoc {
    /// Short name of the shape.
    pub name: String,
    /// Additional detail lines describing the shape.
    pub details: Vec<String>,
}

impl ShapeDoc {
    /// Create a [`ShapeDoc`] with the given name and no details.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            details: Vec::new(),
        }
    }

    /// Append a detail line, returning the updated doc (builder style).
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.details.push(detail.into());
        self
    }
}

/// A match quality score used to rank shapes during overload selection.
///
/// Higher scores are preferred; [`reject`](MatchScore::reject) marks a
/// non-match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchScore(i32);

impl MatchScore {
    /// Build a score from an explicit integer weight.
    pub fn exact(value: i32) -> Self {
        Self(value)
    }

    /// The score that marks a rejected match (far below any accept).
    pub fn reject() -> Self {
        Self(i32::MIN / 2)
    }

    /// The underlying integer weight.
    pub fn value(self) -> i32 {
        self.0
    }
}

impl core::ops::AddAssign for MatchScore {
    fn add_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

/// Captures produced by a successful match: named values and named exprs.
///
/// A match binds names to either runtime [`Value`]s or unevaluated [`Expr`]s;
/// the bindings can later be projected into an [`Env`].
#[derive(Clone, Debug, Default)]
pub struct ShapeBindings {
    values: Vec<(Symbol, Value)>,
    exprs: Vec<(Symbol, Expr)>,
}

impl ShapeBindings {
    /// Create an empty set of bindings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a name to a runtime [`Value`].
    pub fn bind_value(&mut self, name: Symbol, value: Value) {
        self.values.push((name, value));
    }

    /// Bind a name to an unevaluated [`Expr`].
    pub fn bind_expr(&mut self, name: Symbol, expr: Expr) {
        self.exprs.push((name, expr));
    }

    /// Append all bindings from `other` into this set.
    pub fn extend(&mut self, other: ShapeBindings) {
        self.values.extend(other.values);
        self.exprs.extend(other.exprs);
    }

    /// The value bindings, in insertion order.
    pub fn values(&self) -> &[(Symbol, Value)] {
        &self.values
    }

    /// The expr bindings, in insertion order.
    pub fn exprs(&self) -> &[(Symbol, Expr)] {
        &self.exprs
    }

    /// Install these bindings as a fresh child of the context's current env.
    pub fn into_env(self, cx: &mut Cx) -> Result<()> {
        let env = self.into_child_env(cx)?;
        *cx.env_mut() = env;
        Ok(())
    }

    /// Build a child [`Env`] from the context's env populated with these
    /// bindings, without installing it.
    pub fn into_child_env(self, cx: &mut Cx) -> Result<Env> {
        let mut env = Env::child(Arc::new(cx.env().clone()));
        for (name, value) in self.values {
            env.define(name, value);
        }
        for (name, expr) in self.exprs {
            let value = cx.factory().expr(expr)?;
            env.define(name, value);
        }
        Ok(env)
    }
}

/// The outcome of checking a value or expr against a [`Shape`].
///
/// Carries acceptance, captured [`ShapeBindings`], a [`MatchScore`] for
/// ranking, and any [`Diagnostic`]s gathered during the check.
///
/// # Examples
///
/// ```
/// use sim_kernel::shape::{MatchScore, ShapeMatch};
///
/// let ok = ShapeMatch::accept(MatchScore::exact(3));
/// assert!(ok.accepted);
/// assert_eq!(ok.score.value(), 3);
///
/// let no = ShapeMatch::reject("expected a string");
/// assert!(!no.accepted);
/// assert_eq!(no.diagnostics.len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct ShapeMatch {
    /// Whether the input satisfied the shape.
    pub accepted: bool,
    /// Names captured by the match.
    pub captures: ShapeBindings,
    /// Ranking score for overload selection.
    pub score: MatchScore,
    /// Diagnostics gathered while matching.
    pub diagnostics: Vec<Diagnostic>,
}

impl ShapeMatch {
    /// An accepted match with the given score and no captures or diagnostics.
    pub fn accept(score: MatchScore) -> Self {
        Self {
            accepted: true,
            captures: ShapeBindings::new(),
            score,
            diagnostics: Vec::new(),
        }
    }

    /// A rejected match carrying a single error diagnostic.
    pub fn reject(message: impl Into<String>) -> Self {
        Self {
            accepted: false,
            captures: ShapeBindings::new(),
            score: MatchScore::reject(),
            diagnostics: vec![Diagnostic::error(message)],
        }
    }

    /// A rejected match carrying one already-built diagnostic.
    pub fn reject_with_diagnostic(diagnostic: Diagnostic) -> Self {
        Self {
            accepted: false,
            captures: ShapeBindings::new(),
            score: MatchScore::reject(),
            diagnostics: vec![diagnostic],
        }
    }
}

// sim-non-citizen(reason = "shape match result projection; canonical data is exposed as a table", kind = "marker", descriptor = "")
/// Object wrapper exposing a [`ShapeMatch`] to the runtime as a table.
#[derive(Clone, Debug)]
pub struct ShapeMatchObject {
    matched: ShapeMatch,
}

impl ShapeMatchObject {
    /// Wrap a [`ShapeMatch`] as a runtime object.
    pub fn new(matched: ShapeMatch) -> Self {
        Self { matched }
    }

    /// Borrow the wrapped [`ShapeMatch`].
    pub fn matched(&self) -> &ShapeMatch {
        &self.matched
    }
}

impl Object for ShapeMatchObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<shape-match {} score={}>",
            if self.matched.accepted {
                "accepted"
            } else {
                "rejected"
            },
            self.matched.score.value()
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl crate::ObjectCompat for ShapeMatchObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = Symbol::qualified("core", "ShapeMatch");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(crate::id::CORE_SHAPE_MATCH_CLASS_ID, symbol)
    }
    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(self.matched.accepted)
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        shape_match_table(cx, &self.matched)
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table(cx)?.object().as_expr(cx)
    }
}

/// A coarse classifier over [`Expr`] variants, used by grammar shapes.
///
/// Each variant names a structural kind of expression; [`matches`](ExprKind::matches)
/// tests an [`Expr`] against the kind and [`name`](ExprKind::name) gives its
/// stable lowercase tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprKind {
    /// The nil expression.
    Nil,
    /// A boolean literal.
    Bool,
    /// A number literal.
    Number,
    /// A symbol.
    Symbol,
    /// A string literal.
    String,
    /// A byte-string literal.
    Bytes,
    /// A list form.
    List,
    /// A vector form.
    Vector,
    /// A map form.
    Map,
    /// A set form.
    Set,
    /// A call form.
    Call,
    /// An infix operator form.
    Infix,
    /// A prefix operator form.
    Prefix,
    /// A postfix operator form.
    Postfix,
    /// A block form.
    Block,
    /// A quote form.
    Quote,
    /// An annotated form.
    Annotated,
    /// An extension form.
    Extension,
}

impl ExprKind {
    /// Whether `expr` is an instance of this structural kind.
    pub fn matches(&self, expr: &Expr) -> bool {
        matches!(
            (self, expr),
            (Self::Nil, Expr::Nil)
                | (Self::Bool, Expr::Bool(_))
                | (Self::Number, Expr::Number(_))
                | (Self::Symbol, Expr::Symbol(_))
                | (Self::String, Expr::String(_))
                | (Self::Bytes, Expr::Bytes(_))
                | (Self::List, Expr::List(_))
                | (Self::Vector, Expr::Vector(_))
                | (Self::Map, Expr::Map(_))
                | (Self::Set, Expr::Set(_))
                | (Self::Call, Expr::Call { .. })
                | (Self::Infix, Expr::Infix { .. })
                | (Self::Prefix, Expr::Prefix { .. })
                | (Self::Postfix, Expr::Postfix { .. })
                | (Self::Block, Expr::Block(_))
                | (Self::Quote, Expr::Quote { .. })
                | (Self::Annotated, Expr::Annotated { .. })
                | (Self::Extension, Expr::Extension { .. })
        )
    }

    /// The stable lowercase tag for this kind (e.g. `"string"`).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Bool => "bool",
            Self::Number => "number",
            Self::Symbol => "symbol",
            Self::String => "string",
            Self::Bytes => "bytes",
            Self::List => "list",
            Self::Vector => "vector",
            Self::Map => "map",
            Self::Set => "set",
            Self::Call => "call",
            Self::Infix => "infix",
            Self::Prefix => "prefix",
            Self::Postfix => "postfix",
            Self::Block => "block",
            Self::Quote => "quote",
            Self::Annotated => "annotated",
            Self::Extension => "extension",
        }
    }
}

/// What a [`call_shape`] invocation checks: a runtime value or an expr.
#[derive(Clone, Debug)]
pub enum ShapeCallTarget {
    /// Check a runtime [`Value`].
    Value(Value),
    /// Check an [`Expr`].
    Expr(Expr),
}

/// Run a shape against a [`ShapeCallTarget`] and return the match as a value.
///
/// This is the matcher-call path: it dispatches to
/// [`check_value`](Shape::check_value) or [`check_expr`](Shape::check_expr) and
/// wraps the [`ShapeMatch`] via [`shape_match_value`].
pub fn call_shape(cx: &mut Cx, shape: &dyn Shape, target: ShapeCallTarget) -> Result<Value> {
    let matched = match target {
        ShapeCallTarget::Value(value) => shape.check_value(cx, value)?,
        ShapeCallTarget::Expr(expr) => shape.check_expr(cx, &expr)?,
    };
    shape_match_value(cx, matched)
}

/// Wrap a [`ShapeMatch`] as an opaque [`ShapeMatchObject`] runtime value.
pub fn shape_match_value(cx: &mut Cx, matched: ShapeMatch) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(ShapeMatchObject::new(matched)))
}

/// Decide whether `child` is a subshape of `parent`.
///
/// The generic walk compares ids and symbols, consults the shape's own
/// [`is_subshape_of`](Shape::is_subshape_of) override, treats the core `Any`
/// and `AnyShape` symbols as a top for non-effectful shapes, and otherwise
/// recurses through the declared [`parents`](Shape::parents).
pub fn shape_is_subshape_of(cx: &mut Cx, child: &dyn Shape, parent: &dyn Shape) -> Result<bool> {
    let mut seen = BTreeSet::new();
    shape_is_subshape_of_inner(cx, child, parent, &mut seen)
}

fn shape_is_subshape_of_inner(
    cx: &mut Cx,
    child: &dyn Shape,
    parent: &dyn Shape,
    seen: &mut BTreeSet<ShapeIdentity>,
) -> Result<bool> {
    if let (Some(child_id), Some(parent_id)) = (child.id(), parent.id())
        && child_id == parent_id
    {
        return Ok(true);
    }
    if let (Some(child_symbol), Some(parent_symbol)) = (child.symbol(), parent.symbol())
        && child_symbol == parent_symbol
    {
        return Ok(true);
    }
    if let Some(answer) = child.is_subshape_of(cx, parent)? {
        return Ok(answer);
    }
    if !seen.insert(shape_identity(child)) {
        return Ok(false);
    }
    if matches!(
        parent.symbol(),
        Some(symbol)
            if symbol == Symbol::qualified("core", "Any")
                || symbol == Symbol::qualified("core", "AnyShape")
    ) && !child.is_effectful()
    {
        return Ok(true);
    }
    for candidate in child.parents(cx)? {
        let Some(candidate) = candidate.object().as_shape() else {
            continue;
        };
        if shape_is_subshape_of_inner(cx, candidate, parent, seen)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ShapeIdentity {
    Id(ShapeId),
    Symbol(Symbol),
    Pointer(usize),
}

fn shape_identity(shape: &dyn Shape) -> ShapeIdentity {
    if let Some(id) = shape.id() {
        return ShapeIdentity::Id(id);
    }
    if let Some(symbol) = shape.symbol() {
        return ShapeIdentity::Symbol(symbol);
    }
    ShapeIdentity::Pointer(shape as *const dyn Shape as *const () as usize)
}

fn shape_match_table(cx: &mut Cx, matched: &ShapeMatch) -> Result<Value> {
    let value_captures = cx.factory().table(matched.captures.values().to_vec())?;
    let expr_captures = cx.factory().table(
        matched
            .captures
            .exprs()
            .iter()
            .map(|(symbol, expr)| Ok((symbol.clone(), cx.factory().expr(expr.clone())?)))
            .collect::<Result<Vec<_>>>()?,
    )?;
    let diagnostics = matched
        .diagnostics
        .clone()
        .into_iter()
        .map(|diagnostic| diagnostic_value(cx, diagnostic))
        .collect::<Result<Vec<_>>>()?;
    let diagnostics = cx.factory().list(diagnostics)?;
    cx.factory().table(vec![
        (
            Symbol::new("accepted"),
            cx.factory().bool(matched.accepted)?,
        ),
        (
            Symbol::new("score"),
            cx.factory().number_literal(
                Symbol::qualified("numbers", "f64"),
                matched.score.value().to_string(),
            )?,
        ),
        (Symbol::qualified("captures", "value"), value_captures),
        (Symbol::qualified("captures", "expr"), expr_captures),
        (Symbol::new("diagnostics"), diagnostics),
    ])
}

fn diagnostic_value(cx: &mut Cx, diagnostic: Diagnostic) -> Result<Value> {
    let hints = diagnostic_hints_value(cx, &diagnostic)?;
    let severity = match diagnostic.severity {
        crate::error::Severity::Error => "error",
        crate::error::Severity::Warning => "warning",
        crate::error::Severity::Info => "info",
        crate::error::Severity::Note => "note",
    };
    let related = diagnostic
        .related
        .into_iter()
        .filter(|related| !HintMetadata::is_hint_diagnostic(related))
        .map(|related| diagnostic_value(cx, related))
        .collect::<Result<Vec<_>>>()?;
    let related = cx.factory().list(related)?;
    let mut entries = vec![
        (
            Symbol::new("severity"),
            cx.factory().symbol(Symbol::new(severity))?,
        ),
        (
            Symbol::new("message"),
            cx.factory().string(diagnostic.message)?,
        ),
        (Symbol::new("related"), related),
        (Symbol::new("hints"), hints),
    ];
    if let Some(code) = diagnostic.code {
        entries.push((Symbol::new("code"), cx.factory().symbol(code)?));
    }
    cx.factory().table(entries)
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
