//! The [`Value`] contract: the universal runtime handle for every object.
//!
//! The kernel defines the value type and the [`RuntimeObject`] marker that
//! every object satisfies; concrete object kinds are supplied by the libraries.

use std::{
    any::Any,
    hash::{Hash, Hasher},
    sync::Arc,
};

use crate::object::{Object, ObjectCompat};

/// Marker trait every concrete runtime object satisfies.
///
/// The kernel requires that an object be an [`Object`], be [`ObjectCompat`],
/// and be `Any + Send + Sync`; the blanket impl grants this trait to every
/// type that meets those bounds. Libraries supply the concrete object kinds.
pub trait RuntimeObject: Object + ObjectCompat + Any + Send + Sync {}

impl<T> RuntimeObject for T where T: Object + ObjectCompat + Any + Send + Sync {}

impl dyn RuntimeObject {
    /// Attempts to downcast the object to a concrete type `T`.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}

/// The universal runtime handle for every object.
///
/// A [`Value`] is a reference-counted handle to a [`RuntimeObject`]; it is the
/// object end of the data flow `tokens -> checked forms -> objects -> checked
/// calls -> objects -> encoded forms`. Equality and hashing are by object
/// identity (pointer), not by structural contents.
///
/// # Examples
///
/// ```
/// # use sim_kernel::Value;
/// # fn demo(value: Value) {
/// // Every value exposes its object and that object's header.
/// let header = value.header();
/// let _same = value.object().header();
/// let _ = header;
/// # }
/// ```
#[derive(Clone)]
pub struct Value(Arc<dyn RuntimeObject>);

impl Value {
    pub(crate) fn from_arc(value: Arc<dyn RuntimeObject>) -> Self {
        Self(value)
    }

    /// Borrows the underlying runtime object.
    pub fn object(&self) -> &(dyn RuntimeObject + 'static) {
        self.0.as_ref()
    }

    /// Borrows the object's [`ObjectHeader`](crate::ObjectHeader).
    pub fn header(&self) -> &crate::ObjectHeader {
        self.object().header()
    }
}

impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Value(..)")
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let ptr = Arc::as_ptr(&self.0) as *const ();
        ptr.hash(state);
    }
}
