//! The class contract: callable factories with subclass relationships.
//!
//! This is protocol the libraries implement; the kernel defines the [`Class`]
//! trait and the subclass test, not concrete class hierarchies.

use crate::{
    callable::Callable,
    env::Cx,
    error::Result,
    id::{ClassId, Symbol},
    object::{ClassRef, ReadConstructorRef, ShapeRef, TableRef},
};

/// Runtime protocol for class objects.
///
/// Classes are callable constructor values with stable identity, shape
/// metadata, read-constructor support, and browseable members.
pub trait Class: Callable {
    /// Stable identity of the class.
    fn id(&self) -> ClassId;
    /// Display symbol naming the class.
    fn symbol(&self) -> Symbol;
    /// Direct parent classes; the default reports no parents.
    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ClassRef>> {
        Ok(Vec::new())
    }
    /// Whether this class is `expected` or transitively descends from it.
    fn is_subclass_of(&self, cx: &mut Cx, expected: ClassRef) -> Result<bool>
    where
        Self: Sized,
    {
        class_is_subclass_of(cx, self, expected)
    }
    /// Shape describing the arguments accepted by the constructor.
    fn constructor_shape(&self, cx: &mut Cx) -> Result<ShapeRef>;
    /// Shape describing instances produced by the class.
    fn instance_shape(&self, cx: &mut Cx) -> Result<ShapeRef>;
    /// Optional read-constructor for decoding instances from data forms.
    fn read_constructor(&self, cx: &mut Cx) -> Result<Option<ReadConstructorRef>>;
    /// Browseable member table of the class.
    fn members(&self, cx: &mut Cx) -> Result<TableRef>;
}

/// Walk the parent chain of `child` to test whether it is a subclass of
/// `expected`, returning `false` when either side is not a class object.
pub fn class_is_subclass_of(cx: &mut Cx, child: &dyn Class, expected: ClassRef) -> Result<bool> {
    let Some(expected_class) = expected.object().as_class() else {
        return Ok(false);
    };
    if child.id() == expected_class.id() {
        return Ok(true);
    }
    for parent in child.parents(cx)? {
        let Some(parent) = parent.object().as_class() else {
            continue;
        };
        if parent.id() == expected_class.id() || class_is_subclass_of(cx, parent, expected.clone())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}
