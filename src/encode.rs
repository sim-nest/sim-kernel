//! The encode contract: turning objects back into forms at a known position.
//!
//! The kernel defines encode options, output positions, and the object-encoding
//! contract; concrete codecs that render forms live in library crates.

use crate::{
    env::Cx,
    error::Result,
    expr::Expr,
    id::{CodecId, Symbol},
    object::ShapeRef,
    value::Value,
};

/// The output position an encoder is targeting.
///
/// Encoders render differently depending on where the form will land; the
/// kernel defines the positions, concrete codecs honor them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodePosition {
    /// Output will be evaluated.
    Eval,
    /// Output sits inside a quotation.
    Quote,
    /// Output is plain data.
    Data,
    /// Output is a pattern.
    Pattern,
}

/// How a constructed object is surfaced when encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructorSurface {
    /// A bare constructor call.
    BareCall,
    /// A read-time construct form.
    ReadConstruct,
    /// Tagged data.
    TaggedData,
}

/// Whether read-time construct forms may be emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadConstructEncodePolicy {
    /// Forbid read-construct output.
    Forbid,
    /// Allow read-construct output.
    Allow,
}

/// Whether read-eval output may be emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadEvalEncodePolicy {
    /// Forbid read-eval output.
    Forbid,
    /// Allow broad read-eval output.
    AllowBroad,
}

/// Whether output is canonicalized or preserves the original input form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalPolicy {
    /// Emit a canonical form.
    Canonical,
    /// Preserve the input form as read.
    PreserveInput,
}

/// The full set of options controlling an encode pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeOptions {
    /// The target output position.
    pub position: EncodePosition,
    /// The canonicalization policy.
    pub canonical: CanonicalPolicy,
    /// Whether to preserve lossless source origin.
    pub lossless_origin: bool,
    /// The read-construct emission policy.
    pub read_construct: ReadConstructEncodePolicy,
    /// The read-eval emission policy.
    pub read_eval: ReadEvalEncodePolicy,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            position: EncodePosition::Data,
            canonical: CanonicalPolicy::Canonical,
            lossless_origin: false,
            read_construct: ReadConstructEncodePolicy::Allow,
            read_eval: ReadEvalEncodePolicy::Forbid,
        }
    }
}

/// The mutable context passed to an encoder while it renders a form.
pub struct WriteCx<'a> {
    /// The runtime context.
    pub cx: &'a mut Cx,
    /// The codec performing the encode.
    pub codec: CodecId,
    /// The active encode options.
    pub options: EncodeOptions,
}

impl<'a> WriteCx<'a> {
    /// Reborrows this context with the encode position overridden.
    pub fn with_position<'b>(&'b mut self, position: EncodePosition) -> WriteCx<'b> {
        let mut options = self.options.clone();
        options.position = position;
        WriteCx {
            cx: &mut *self.cx,
            codec: self.codec,
            options,
        }
    }
}

/// How an object presents itself to a codec for encoding.
///
/// The kernel defines these encoding shapes; a codec turns the chosen shape
/// into concrete output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectEncoding {
    /// Encode as a constructor call of `class` with argument expressions.
    Constructor {
        /// The constructing class.
        class: Symbol,
        /// The constructor argument expressions.
        args: Vec<Expr>,
    },
    /// Encode as tagged data with named fields.
    TaggedData {
        /// The data tag.
        tag: Symbol,
        /// The named field expressions.
        fields: Vec<(Symbol, Expr)>,
    },
    /// Encode as an opaque reference identified by a stable id.
    Opaque {
        /// The object's class.
        class: Symbol,
        /// A stable, codec-renderable identifier for the object.
        stable_id: String,
    },
}

/// Contract: an object that can describe its own encoding.
///
/// The kernel defines this trait; objects implement it to choose how codecs
/// render them back into forms.
pub trait ObjectEncode: Send + Sync {
    /// Returns the [`ObjectEncoding`] this object should be rendered as.
    fn object_encoding(&self, cx: &mut Cx) -> Result<ObjectEncoding>;
}

/// Contract: a read-time constructor that builds an object from read args.
///
/// The kernel defines the contract; libraries register concrete constructors
/// that the reader invokes to build values from forms.
pub trait ReadConstructor: Send + Sync {
    /// The symbol naming this constructor.
    fn symbol(&self) -> Symbol;
    /// The shape the constructor's arguments must match.
    fn args_shape(&self, cx: &mut Cx) -> Result<ShapeRef>;
    /// Constructs the value from matched arguments at read time.
    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> Result<Value>;
}
