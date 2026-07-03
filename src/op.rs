//! The operation contract: keyed, shape-checked calls dispatched on objects.
//!
//! The kernel defines the [`Op`] trait, operation specs and keys, and the
//! resolve/invoke surface plus the well-known core operation keys; libraries
//! implement the concrete operations.

use crate::{
    capability::CapabilityName,
    effect::Effect,
    env::Cx,
    error::{Error, Result},
    id::{ShapeId, Symbol},
    ref_id::Ref,
    term::OpKey,
    value::Value,
};

/// The declared contract of an operation: its key, shapes, effects, and
/// required capabilities.
///
/// An `OpSpec` is metadata the kernel checks before dispatch -- the
/// [`args_shape`](OpSpec::args_shape) is validated against the input and the
/// [`requires`](OpSpec::requires) capabilities are demanded from the [`Cx`] --
/// while the concrete behavior lives in the [`Op`] that carries it.
#[derive(Clone, Debug)]
pub struct OpSpec {
    /// The interned operation key (`namespace:name@vN`) this spec describes.
    pub key: OpKey,
    /// The subject the operation dispatches on.
    pub subject: Ref,
    /// The shape the input is checked against before invocation.
    pub args_shape: Ref,
    /// The shape the operation promises for its result.
    pub result_shape: Ref,
    /// The effect symbols the operation may emit.
    pub effects: Vec<Symbol>,
    /// The capabilities that must be granted before the operation runs.
    pub requires: Vec<CapabilityName>,
}

impl OpSpec {
    /// Builds a spec with no declared effects or capability requirements.
    pub fn new(key: OpKey, subject: Ref, args_shape: Ref, result_shape: Ref) -> Self {
        Self {
            key,
            subject,
            args_shape,
            result_shape,
            effects: Vec::new(),
            requires: Vec::new(),
        }
    }

    /// Returns the spec with its effect set replaced.
    pub fn with_effects(mut self, effects: Vec<Symbol>) -> Self {
        self.effects = effects;
        self
    }

    /// Returns the spec with one more required capability appended.
    pub fn requiring(mut self, capability: CapabilityName) -> Self {
        self.requires.push(capability);
        self
    }

    /// Returns the spec with its required-capability set replaced.
    pub fn with_requirements(mut self, requires: Vec<CapabilityName>) -> Self {
        self.requires = requires;
        self
    }
}

/// A keyed, shape-checked operation dispatched on an object.
///
/// The kernel defines this contract and the well-known core keys; libraries
/// implement concrete operations against it. Dispatch goes through
/// [`invoke_op`], which checks the [`OpSpec`] before calling
/// [`invoke_authorized`](Op::invoke_authorized).
pub trait Op: Send + Sync {
    /// Returns the operation's declared contract.
    fn spec(&self) -> &OpSpec;
    /// Runs the operation after the kernel has checked shapes and capabilities.
    fn invoke_authorized(&self, cx: &mut Cx, input: Value) -> Result<Step>;
}

/// The outcome of invoking an [`Op`]: a value, an event batch, or a suspension.
#[derive(Clone, Debug)]
pub enum Step {
    /// A completed value result.
    Value(Value),
    /// A batch of emitted events.
    Events(Value),
    /// A suspended computation carrying the pending [`Effect`].
    Suspended(Box<Effect>),
}

/// An operation resolved against a target, ready to be invoked.
///
/// Produced by [`resolve_op`], this hides whether the operation is a native
/// [`Op`] registered on the object or a kernel adapter synthesized from one of
/// the object's protocol surfaces.
pub struct ResolvedOp<'a> {
    inner: ResolvedOpKind<'a>,
}

impl<'a> ResolvedOp<'a> {
    /// Returns the resolved operation's contract.
    pub fn spec(&self) -> &OpSpec {
        match &self.inner {
            ResolvedOpKind::Native(op) => op.spec(),
            ResolvedOpKind::Adapter(adapter) => &adapter.spec,
        }
    }

    /// Runs the resolved operation; callers must check the spec first.
    pub fn invoke_authorized(&self, cx: &mut Cx, input: Value) -> Result<Step> {
        match &self.inner {
            ResolvedOpKind::Native(op) => op.invoke_authorized(cx, input),
            ResolvedOpKind::Adapter(adapter) => adapter.invoke(cx, input),
        }
    }
}

enum ResolvedOpKind<'a> {
    Native(&'a dyn Op),
    Adapter(Box<crate::op_adapters::AdapterOp<'a>>),
}

/// Resolves and runs an operation on `target`, enforcing its [`OpSpec`].
///
/// This is the kernel dispatch entry point: it resolves the key against the
/// target (native [`Op`] or synthesized adapter), demands the required
/// capabilities, checks the input against the declared args shape when that
/// shape is registered, and then invokes the operation.
pub fn invoke_op(cx: &mut Cx, target: Value, key: &OpKey, input: Value) -> Result<Step> {
    let resolved = resolve_op(cx, &target, key)?;
    let requires = resolved.spec().requires.clone();
    cx.require_all(&requires)?;
    let args_shape = resolved.spec().args_shape.clone();
    check_shape_if_available(cx, args_shape, input.clone())?;
    resolved.invoke_authorized(cx, input)
}

/// Resolves an operation key against a target without invoking it.
///
/// Prefers a native [`Op`] registered on the object; otherwise falls back to a
/// kernel adapter synthesized from one of the object's protocol surfaces, and
/// errors when neither is available.
pub fn resolve_op<'a>(cx: &mut Cx, target: &'a Value, key: &OpKey) -> Result<ResolvedOp<'a>> {
    if let Some(op) = target.object().op(key) {
        return Ok(ResolvedOp {
            inner: ResolvedOpKind::Native(op),
        });
    }

    let Some(adapter) = crate::op_adapters::resolve_adapter(cx, target, key)? else {
        return Err(Error::Eval(format!(
            "operation {} not available",
            format_op_key(key)
        )));
    };

    Ok(ResolvedOp {
        inner: ResolvedOpKind::Adapter(Box::new(adapter)),
    })
}

/// Checks `input` against a shape reference, passing silently when the shape is
/// `Any` or not registered.
///
/// Used by [`invoke_op`] to validate operation arguments; an unresolved or
/// any-matching shape is treated as accepting all input.
pub fn check_shape_if_available(cx: &mut Cx, shape_ref: Ref, input: Value) -> Result<()> {
    let Ref::Symbol(symbol) = shape_ref else {
        return Ok(());
    };
    if is_any_shape_symbol(&symbol) {
        return Ok(());
    }

    let Some(shape_value) = cx.registry().shape_by_symbol(&symbol).cloned() else {
        return Ok(());
    };
    let Some(shape) = shape_value.object().as_shape() else {
        return Ok(());
    };
    let matched = shape.check_value(cx, input)?;
    if matched.accepted {
        Ok(())
    } else {
        Err(Error::WrongShape {
            expected: shape.id().unwrap_or(ShapeId(0)),
            diagnostics: matched.diagnostics,
        })
    }
}

/// The well-known key for the core call operation (`core:call@v1`).
///
/// # Examples
///
/// ```
/// # use sim_kernel::op::core_call_op_key;
/// let key = core_call_op_key();
/// assert_eq!(key.namespace.to_string(), "core");
/// assert_eq!(key.name.to_string(), "call");
/// assert_eq!(key.version, 1);
/// ```
pub fn core_call_op_key() -> OpKey {
    core_op_key("call")
}

/// The well-known key for checking a value against a shape.
pub fn core_shape_check_value_op_key() -> OpKey {
    core_op_key("shape-check-value")
}

/// The well-known key for checking a term against a shape.
pub fn core_shape_check_term_op_key() -> OpKey {
    core_op_key("shape-check-term")
}

/// The well-known key for describing a shape.
pub fn core_shape_describe_op_key() -> OpKey {
    core_op_key("shape-describe")
}

/// The well-known key for reading a class's symbol.
pub fn core_class_symbol_op_key() -> OpKey {
    core_op_key("class-symbol")
}

/// The well-known key for reading an object's encoding.
pub fn core_object_encoding_op_key() -> OpKey {
    core_op_key("object-encoding")
}

/// The well-known key for read-time construction.
pub fn core_read_construct_op_key() -> OpKey {
    core_op_key("read-construct")
}

/// The well-known key for reading a number's domain symbol.
pub fn core_number_domain_symbol_op_key() -> OpKey {
    core_op_key("number-domain-symbol")
}

/// The well-known key for reading a number value.
pub fn core_number_value_op_key() -> OpKey {
    core_op_key("number-value")
}

/// The well-known key for starting a distributed eval realization.
pub fn core_realize_start_op_key() -> OpKey {
    core_op_key("realize-start")
}

/// The well-known key for forcing a thunk.
pub fn core_force_op_key() -> OpKey {
    core_op_key("force")
}

/// The well-known key for advancing a sequence.
pub fn core_seq_next_op_key() -> OpKey {
    core_op_key("seq-next")
}

/// The well-known key for closing a sequence.
pub fn core_seq_close_op_key() -> OpKey {
    core_op_key("seq-close")
}

/// The well-known key for reading a list's items.
pub fn core_list_items_op_key() -> OpKey {
    core_op_key("list-items")
}

/// The well-known key for reading a table's entries.
pub fn core_table_entries_op_key() -> OpKey {
    core_op_key("table-entries")
}

/// The well-known key for testing whether a directory entry is a directory.
pub fn core_dir_is_dir_op_key() -> OpKey {
    core_op_key("dir-is-dir")
}

/// The well-known key for snapshotting an object as an [`Expr`](crate::Expr).
pub fn core_expr_snapshot_op_key() -> OpKey {
    core_op_key("expr-snapshot")
}

/// The shape reference that accepts any value (`core:Any`).
pub fn core_any_ref() -> Ref {
    core_ref("Any")
}

fn is_any_shape_symbol(symbol: &Symbol) -> bool {
    *symbol == core_symbol("Any") || *symbol == core_symbol("AnyShape")
}

fn core_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("core"), Symbol::new(name), 1)
}

pub(crate) fn core_ref(name: &str) -> Ref {
    Ref::Symbol(core_symbol(name))
}

pub(crate) fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}

fn format_op_key(key: &OpKey) -> String {
    format!("{}:{}@v{}", key.namespace, key.name, key.version)
}
