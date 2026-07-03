//! The list contract: the pluggable [`ListValue`] backend protocol.
//!
//! The kernel defines the list-value protocol, force bounds, and a backend
//! registry; concrete list representations are libs loaded against it. A
//! `VecList` is provided as a baseline backend, not as kernel-fixed behavior.

use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

use crate::{
    capability::list_force_unbounded_capability,
    env::Cx,
    error::{Error, Result},
    expr::Expr,
    id::{CORE_LIST_CLASS_ID, Symbol},
    object::{ClassRef, Object},
    value::{RuntimeObject, Value},
};

/// Default maximum number of elements an encoder or expr conversion will force
/// from a lazy or endless list before erroring.
pub const DEFAULT_FORCE_BOUND: usize = 1 << 20;

/// Result of asking a list for its length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthResult {
    /// The list is finite and its length is known.
    Known(usize),
    /// The list is (or may be) endless; an exact length is not available.
    Unknown,
}

/// Shared behaviour for every list backend.
pub trait ListValue: RuntimeObject {
    /// Whether the list has no elements.
    fn is_empty(&self, cx: &mut Cx) -> Result<bool>;

    /// The first element, or `Ok(None)` when empty.
    fn car(&self, cx: &mut Cx) -> Result<Option<Value>>;

    /// The tail list after the first element, or `Ok(None)` when empty.
    fn cdr(&self, cx: &mut Cx) -> Result<Option<Value>>;

    /// The length, finite or [`LengthResult::Unknown`] for endless lists.
    fn len(&self, cx: &mut Cx) -> Result<LengthResult>;

    /// Compares the spine length against `n` without fully forcing the list.
    fn len_cmp(&self, cx: &mut Cx, n: usize) -> Result<Ordering> {
        spine_len_cmp_impl(cx, self, n)
    }

    /// The element at `index`, or `Ok(None)` when out of range.
    fn get(&self, cx: &mut Cx, index: usize) -> Result<Option<Value>> {
        let mut node = self.cdr_self(cx)?;
        let mut i = index;
        while let Some(list) = node.as_ref().and_then(|value| value.object().as_list()) {
            if list.is_empty(cx)? {
                return Ok(None);
            }
            if i == 0 {
                return list.car(cx);
            }
            i -= 1;
            node = list.cdr(cx)?;
        }
        Ok(None)
    }

    /// Visits up to `limit` elements in order, walking the list spine.
    fn for_each(
        &self,
        cx: &mut Cx,
        limit: Option<usize>,
        visit: &mut dyn FnMut(&Value),
    ) -> Result<()> {
        if matches!(limit, Some(0)) {
            return Ok(());
        }

        let mut count = 0usize;
        let mut head = self.car(cx)?;
        let mut tail = self.cdr(cx)?;
        while let Some(value) = head {
            if matches!(limit, Some(max) if count >= max) {
                return Ok(());
            }
            visit(&value);
            count += 1;
            match tail.as_ref().and_then(|node| node.object().as_list()) {
                Some(list) if !list.is_empty(cx)? => {
                    head = list.car(cx)?;
                    tail = list.cdr(cx)?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// Collects up to `limit` elements into a vector.
    fn to_vec(&self, cx: &mut Cx, limit: Option<usize>) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        self.for_each(cx, limit, &mut |value| out.push(value.clone()))?;
        Ok(out)
    }

    /// The value to start spine walks from; the receiver itself by default.
    fn cdr_self(&self, _cx: &mut Cx) -> Result<Option<Value>> {
        Ok(self.as_self_value())
    }

    /// The receiver re-wrapped as a [`Value`], when it can produce one.
    fn as_self_value(&self) -> Option<Value> {
        None
    }
}

/// Compares the spine length of `head` against `n` without full forcing.
pub fn spine_len_cmp(cx: &mut Cx, head: &dyn ListValue, n: usize) -> Result<Ordering> {
    spine_len_cmp_impl(cx, head, n)
}

/// The force bound for the current context, or `None` if unbounded forcing
/// is permitted by capability.
pub fn force_list_bound(cx: &mut Cx) -> Option<usize> {
    if cx.require(&list_force_unbounded_capability()).is_ok() {
        None
    } else {
        Some(DEFAULT_FORCE_BOUND)
    }
}

/// Materializes a list to a vector, erroring if it exceeds the force bound.
pub fn force_list_to_vec(cx: &mut Cx, list: &dyn ListValue, context: &str) -> Result<Vec<Value>> {
    let bound = force_list_bound(cx);
    if let Some(max) = bound
        && list.len_cmp(cx, max)? == Ordering::Greater
    {
        return Err(Error::Eval(format!(
            "{context}: list exceeds force bound {max}; lazy/endless list cannot be materialized in v1"
        )));
    }
    list.to_vec(cx, bound)
}

fn spine_len_cmp_impl<T: ListValue + ?Sized>(cx: &mut Cx, head: &T, n: usize) -> Result<Ordering> {
    if head.is_empty(cx)? {
        return Ok(0usize.cmp(&n));
    }

    let mut count = 1usize;
    if count > n {
        return Ok(Ordering::Greater);
    }

    let mut node = head.cdr(cx)?;
    while let Some(value) = node {
        let Some(list) = value.object().as_list() else {
            return Err(Error::Eval("list cdr did not yield a list".to_owned()));
        };
        if list.is_empty(cx)? {
            break;
        }
        count += 1;
        if count > n {
            return Ok(Ordering::Greater);
        }
        node = list.cdr(cx)?;
    }
    Ok(count.cmp(&n))
}

/// Baseline eager list backend backed by a shared slice of values.
///
/// # Examples
///
/// ```
/// use sim_kernel::VecList;
///
/// let empty = VecList::from_vec(Vec::new());
/// assert!(empty.as_slice().is_empty());
/// ```
// sim-non-citizen(reason = "kernel list backing object; canonical form is native Expr::List data", kind = "private", descriptor = "")
// FUTURE CLEANUP: this is the kernel's built-in REFERENCE list backend (the
// bootstrap implementation). Once a default-loaded distribution lib can supply
// a `ListBackend` before any user code runs, move `VecList` out of the kernel
// and keep only the `ListValue`/`ListBackend` contracts here.
#[derive(Clone)]
pub struct VecList {
    values: Arc<[Value]>,
}

impl VecList {
    /// Builds a list from an owned vector of values.
    pub fn from_vec(values: Vec<Value>) -> Self {
        Self {
            values: Arc::from(values),
        }
    }

    /// Builds a list from an already-shared slice of values.
    pub fn from_arc(values: Arc<[Value]>) -> Self {
        Self { values }
    }

    /// Borrows the backing slice of values.
    pub fn as_slice(&self) -> &[Value] {
        &self.values
    }
}

impl Object for VecList {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("list[{}]", self.values.len()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for VecList {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = Symbol::qualified("core", "List");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory().class_stub(CORE_LIST_CLASS_ID, symbol)
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::List(
            self.values
                .iter()
                .map(|value| value.object().as_expr(cx))
                .collect::<Result<Vec<_>>>()?,
        ))
    }
    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(!self.values.is_empty())
    }
    fn as_list(&self) -> Option<&dyn ListValue> {
        Some(self)
    }
}

impl ListValue for VecList {
    fn is_empty(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(self.values.is_empty())
    }

    fn car(&self, _cx: &mut Cx) -> Result<Option<Value>> {
        Ok(self.values.first().cloned())
    }

    fn cdr(&self, cx: &mut Cx) -> Result<Option<Value>> {
        if self.values.is_empty() {
            return Ok(None);
        }

        let tail = self.values[1..].to_vec();
        Ok(Some(cx.factory().opaque(Arc::new(Self::from_vec(tail)))?))
    }

    fn len(&self, _cx: &mut Cx) -> Result<LengthResult> {
        Ok(LengthResult::Known(self.values.len()))
    }

    fn len_cmp(&self, _cx: &mut Cx, n: usize) -> Result<Ordering> {
        Ok(self.values.len().cmp(&n))
    }

    fn get(&self, _cx: &mut Cx, index: usize) -> Result<Option<Value>> {
        Ok(self.values.get(index).cloned())
    }

    fn for_each(
        &self,
        _cx: &mut Cx,
        limit: Option<usize>,
        visit: &mut dyn FnMut(&Value),
    ) -> Result<()> {
        let stop = limit.unwrap_or(self.values.len()).min(self.values.len());
        for value in &self.values[..stop] {
            visit(value);
        }
        Ok(())
    }

    fn to_vec(&self, _cx: &mut Cx, limit: Option<usize>) -> Result<Vec<Value>> {
        match limit {
            Some(max) => Ok(self.values.iter().take(max).cloned().collect()),
            None => Ok(self.values.to_vec()),
        }
    }
}

/// Factory protocol for constructing lists in a particular representation.
pub trait ListBackend: Send + Sync {
    /// Stable name the backend is registered and selected under.
    fn name(&self) -> &str;

    /// Builds a list from an ordered set of items.
    fn new_list(&self, cx: &mut Cx, items: Vec<Value>) -> Result<Value>;

    /// Builds a cons cell prepending `car` onto the list `cdr`.
    fn new_cons(&self, cx: &mut Cx, car: Value, cdr: Value) -> Result<Value>;
}

/// Registry of named list backends with one active default.
pub struct ListRegistry {
    backends: BTreeMap<String, Arc<dyn ListBackend>>,
    active: String,
}

impl ListRegistry {
    /// Builds a registry preloaded with the `vec` backend as active.
    pub fn new() -> Self {
        let mut registry = Self {
            backends: BTreeMap::new(),
            active: "vec".to_owned(),
        };
        registry.register(Arc::new(VecBackend));
        registry
    }

    /// Registers a backend under its own name, replacing any prior one.
    pub fn register(&mut self, backend: Arc<dyn ListBackend>) {
        self.backends.insert(backend.name().to_owned(), backend);
    }

    /// Selects the active backend by name, erroring if it is unknown.
    pub fn set_active(&mut self, name: &str) -> Result<()> {
        if self.backends.contains_key(name) {
            self.active = name.to_owned();
            Ok(())
        } else {
            Err(Error::Eval(format!("unknown list backend: {name}")))
        }
    }

    /// Name of the currently active backend.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Builds a list using the active backend.
    pub fn new_list(&self, cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
        self.backend()?.new_list(cx, items)
    }

    /// Builds a cons cell using the active backend.
    pub fn new_cons(&self, cx: &mut Cx, car: Value, cdr: Value) -> Result<Value> {
        self.backend()?.new_cons(cx, car, cdr)
    }

    fn backend(&self) -> Result<&Arc<dyn ListBackend>> {
        self.backends
            .get(&self.active)
            .ok_or_else(|| Error::Eval("active list backend missing".to_owned()))
    }
}

impl Default for ListRegistry {
    fn default() -> Self {
        Self::new()
    }
}

struct VecBackend;

impl ListBackend for VecBackend {
    fn name(&self) -> &str {
        "vec"
    }

    fn new_list(&self, cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
        cx.factory().opaque(Arc::new(VecList::from_vec(items)))
    }

    fn new_cons(&self, cx: &mut Cx, car: Value, cdr: Value) -> Result<Value> {
        let Some(list) = cdr.object().as_list() else {
            return Err(Error::TypeMismatch {
                expected: "list",
                found: "non-list",
            });
        };
        let mut items = vec![car];
        items.extend(list.to_vec(cx, None)?);
        cx.factory().opaque(Arc::new(VecList::from_vec(items)))
    }
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod list_tests;
