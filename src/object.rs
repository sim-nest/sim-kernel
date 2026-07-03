//! The object contract: the runtime object surface and argument forms.
//!
//! The kernel defines the [`Object`] header, the class/shape/constructor
//! reference types, and the checked [`Args`] forms passed to callables; the
//! libraries supply the concrete object implementations.

use std::any::Any;
use std::sync::OnceLock;

use crate::{
    callable::Callable,
    capability::TrustLevel,
    claim::{Claim, ClaimPattern},
    class::Class,
    datum::Datum,
    encode::{ObjectEncode, ReadConstructor},
    env::Cx,
    error::Result,
    expr::Expr,
    id::Symbol,
    number_domain::{NumberDomain, NumberValue},
    ref_id::Ref,
    term::OpKey,
    value::Value,
};

/// Runtime value carrying a class object.
pub type ClassRef = Value;
/// Runtime value carrying a read-constructor object.
pub type ReadConstructorRef = Value;
/// Runtime value carrying a shape object.
pub type ShapeRef = Value;
/// Runtime value carrying a table object.
pub type TableRef = Value;

/// Identity and trust header attached to every runtime object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectHeader {
    /// Stable reference identifying the object.
    pub id: Ref,
    /// Symbol naming the object's kind.
    pub kind: Symbol,
    /// Trust level governing the object's capabilities.
    pub trust: TrustLevel,
}

/// Sink that collects claims emitted while reflecting over an object.
pub trait ClaimSink {
    /// Accept one emitted claim.
    fn claim(&mut self, claim: Claim) -> Result<()>;
}

/// Checked, already-evaluated positional arguments passed to a callable.
#[derive(Clone, Default)]
pub struct Args {
    values: Vec<Value>,
}

impl Args {
    /// Build an argument list from evaluated values.
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Borrow the argument values in order.
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// Consume the argument list, returning the owned values.
    pub fn into_vec(self) -> Vec<Value> {
        self.values
    }
}

/// Unevaluated argument expressions passed to a raw callable.
#[derive(Clone, Default)]
pub struct RawArgs {
    exprs: Vec<Expr>,
}

impl RawArgs {
    /// Build a raw argument list from unevaluated expressions.
    pub fn new(exprs: Vec<Expr>) -> Self {
        Self { exprs }
    }

    /// Borrow the argument expressions in order.
    pub fn exprs(&self) -> &[Expr] {
        &self.exprs
    }

    /// Consume the raw argument list, returning the owned expressions.
    pub fn into_exprs(self) -> Vec<Expr> {
        self.exprs
    }
}

/// Base protocol implemented by every runtime value.
///
/// SIM uses object-style protocol views instead of a closed value enum for
/// extensible runtime behavior. The root trait stays small: behavior is
/// exposed through headers, operations, claims, snapshots, display, and Rust
/// downcasting.
pub trait Object: Send + Sync {
    /// Identity and trust header for the object; defaults to the shared
    /// anonymous header.
    fn header(&self) -> &ObjectHeader {
        default_object_header()
    }

    /// Resolve the operation registered under `key`, if any.
    fn op(&self, _key: &OpKey) -> Option<&dyn crate::op::Op> {
        None
    }

    /// Emit the object's claims matching `pattern` into `sink`.
    fn claims(
        &self,
        _cx: &mut Cx,
        _pattern: &ClaimPattern,
        _sink: &mut dyn ClaimSink,
    ) -> Result<()> {
        Ok(())
    }

    /// Optional content-addressable snapshot of the object's state.
    fn snapshot(&self, _cx: &mut Cx) -> Result<Option<Datum>> {
        Ok(None)
    }

    /// Render the object as a human-readable display string.
    fn display(&self, _cx: &mut Cx) -> Result<String>;

    /// Expose the object for Rust downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Bridge that exposes typed protocol views (class, callable, shape, encoder,
/// number, fabric, stream, and related accessors) for objects that do not
/// resolve them through operations.
///
/// Callers should resolve operations, claims, refs, or snapshots instead of
/// calling this trait. It is kept outside [`Object`] so the root object trait
/// stays minimal while library objects expose this behavior through
/// compatibility adapters.
pub trait ObjectCompat: Object {
    /// Class object this value belongs to; defaults to nil.
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().nil()
    }

    /// Callable view, if the object can be invoked.
    fn as_callable(&self) -> Option<&dyn Callable> {
        None
    }

    /// Class view, if the object is a class.
    fn as_class(&self) -> Option<&dyn Class> {
        None
    }

    /// Shape view, if the object is a shape.
    fn as_shape(&self) -> Option<&dyn crate::shape::Shape> {
        None
    }

    /// Object-encoder view, if the object encodes other objects.
    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        None
    }

    /// Read-constructor view, if the object decodes data forms.
    fn as_read_constructor(&self) -> Option<&dyn ReadConstructor> {
        None
    }

    /// Number-domain view, if the object is a number domain.
    fn as_number_domain(&self) -> Option<&dyn NumberDomain> {
        None
    }

    /// Number-value view, if the object is a domain number.
    fn as_number_value(&self) -> Option<&dyn NumberValue> {
        None
    }

    /// Eval-fabric view, if the object is a distributed eval surface.
    fn as_eval_fabric(&self) -> Option<&dyn crate::eval::EvalFabric> {
        None
    }

    /// Stream view, if the object is a stream.
    fn as_stream(&self) -> Option<&dyn crate::stream::Stream> {
        None
    }

    /// Sequence view, if the object is a sequence.
    fn as_sequence(&self) -> Option<&dyn crate::seq::Sequence> {
        None
    }

    /// Thunk view, if the object is a deferred computation.
    fn as_thunk(&self) -> Option<&dyn crate::eval::Thunk> {
        None
    }

    /// List view, if the object is a list value.
    fn as_list(&self) -> Option<&dyn crate::list::ListValue> {
        None
    }

    /// Table-implementation view, if the object is a table.
    fn as_table_impl(&self) -> Option<&dyn crate::table::Table> {
        None
    }

    /// Directory view, if the object is a directory.
    fn as_dir(&self) -> Option<&dyn crate::table::Dir> {
        None
    }

    /// Expression form of the object; defaults to an opaque extension node.
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Extension {
            tag: Symbol::qualified("core", "opaque-object"),
            payload: Box::new(Expr::String(Object::display(self, cx)?)),
        })
    }

    /// Truthiness of the object; defaults to true.
    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(true)
    }

    /// Publish claims asserting that the object satisfies `shape`; returns
    /// whether any were published.
    fn publish_shape_satisfaction_claims(&self, _cx: &mut Cx, _shape: &Ref) -> Result<bool> {
        Ok(false)
    }

    /// Project the object into a table value; the default exposes its display.
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let display = Object::display(self, cx)?;
        cx.factory().table(vec![(
            Symbol::new("display"),
            cx.factory().string(display)?,
        )])
    }
}

impl dyn Object {
    /// Downcast the object to a concrete type `T`, if it is that type.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}

impl ClaimSink for Vec<Claim> {
    /// Collect the claim by pushing it onto the vector.
    fn claim(&mut self, claim: Claim) -> Result<()> {
        self.push(claim);
        Ok(())
    }
}

/// The shared anonymous [`ObjectHeader`] used by objects without their own.
pub fn default_object_header() -> &'static ObjectHeader {
    static HEADER: OnceLock<ObjectHeader> = OnceLock::new();
    HEADER.get_or_init(|| ObjectHeader {
        id: default_object_ref(),
        kind: Symbol::qualified("core", "Object"),
        trust: TrustLevel::HostInternal,
    })
}

/// Whether `header` is the shared anonymous [`default_object_header`].
pub fn is_default_object_header(header: &ObjectHeader) -> bool {
    header == default_object_header()
}

fn default_object_ref() -> Ref {
    Ref::Symbol(Symbol::qualified("core", "anonymous-object"))
}
