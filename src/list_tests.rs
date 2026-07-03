use std::{
    any::Any,
    cmp::Ordering,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
    },
};

use crate::{
    CORE_LIST_CLASS_ID, ClassRef, Cx, Error, LengthResult, ListRegistry, ListValue, Object, Result,
    Symbol, Value, VecList, force_list_bound, force_list_to_vec, list_force_unbounded_capability,
};

struct GuardList {
    len_cmp_result: Ordering,
    to_vec_calls: Arc<AtomicUsize>,
    seen_limit: Arc<Mutex<Option<Option<usize>>>>,
}

impl GuardList {
    fn new(len_cmp_result: Ordering) -> Self {
        Self {
            len_cmp_result,
            to_vec_calls: Arc::new(AtomicUsize::new(0)),
            seen_limit: Arc::new(Mutex::new(None)),
        }
    }
}

impl Object for GuardList {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("guard-list".to_owned())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl crate::ObjectCompat for GuardList {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(CORE_LIST_CLASS_ID, Symbol::qualified("core", "List"))
    }
    fn as_list(&self) -> Option<&dyn ListValue> {
        Some(self)
    }
}

impl ListValue for GuardList {
    fn is_empty(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(false)
    }

    fn car(&self, cx: &mut Cx) -> Result<Option<Value>> {
        cx.factory().bool(true).map(Some)
    }

    fn cdr(&self, _cx: &mut Cx) -> Result<Option<Value>> {
        Ok(None)
    }

    fn len(&self, _cx: &mut Cx) -> Result<LengthResult> {
        Ok(LengthResult::Unknown)
    }

    fn len_cmp(&self, _cx: &mut Cx, _n: usize) -> Result<Ordering> {
        Ok(self.len_cmp_result)
    }

    fn to_vec(&self, cx: &mut Cx, limit: Option<usize>) -> Result<Vec<Value>> {
        self.to_vec_calls.fetch_add(1, AtomicOrdering::SeqCst);
        *self.seen_limit.lock().unwrap() = Some(limit);
        Ok(vec![cx.factory().bool(true)?])
    }
}

#[test]
fn vec_list_basic() {
    let mut cx = Cx::stub();
    let items = vec![
        cx.factory().bool(true).unwrap(),
        cx.factory().bool(false).unwrap(),
        cx.factory().bool(true).unwrap(),
    ];
    let list = VecList::from_vec(items);

    assert!(!list.is_empty(&mut cx).unwrap());
    assert_eq!(list.len(&mut cx).unwrap(), LengthResult::Known(3));
    assert_eq!(list.len_cmp(&mut cx, 2).unwrap(), Ordering::Greater);
    assert_eq!(list.len_cmp(&mut cx, 3).unwrap(), Ordering::Equal);
    assert_eq!(list.len_cmp(&mut cx, 9).unwrap(), Ordering::Less);
    assert!(list.car(&mut cx).unwrap().is_some());
}

#[test]
fn vec_list_cdr_is_a_list() {
    let mut cx = Cx::stub();
    let items = vec![
        cx.factory().bool(true).unwrap(),
        cx.factory().bool(false).unwrap(),
    ];
    let list = VecList::from_vec(items);
    let tail = list.cdr(&mut cx).unwrap().unwrap();
    let tail_list = tail.object().as_list().unwrap();
    assert_eq!(tail_list.len(&mut cx).unwrap(), LengthResult::Known(1));
}

#[test]
fn list_registry_defaults_to_vec_backend() {
    let registry = ListRegistry::new();
    assert_eq!(registry.active(), "vec");
}

#[test]
fn force_bound_is_bounded_by_default() {
    let mut cx = Cx::stub();
    assert_eq!(force_list_bound(&mut cx), Some(crate::DEFAULT_FORCE_BOUND));
}

#[test]
fn force_bound_becomes_unbounded_with_capability() {
    let mut cx = Cx::stub();
    cx.grant(list_force_unbounded_capability());
    assert_eq!(force_list_bound(&mut cx), None);
}

#[test]
fn bounded_force_rejects_oversized_list_without_materializing_it() {
    let mut cx = Cx::stub();
    let list = GuardList::new(Ordering::Greater);

    let err = force_list_to_vec(&mut cx, &list, "encode").unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message.contains("force bound")));
    assert_eq!(list.to_vec_calls.load(AtomicOrdering::SeqCst), 0);
    assert_eq!(*list.seen_limit.lock().unwrap(), None);
}

#[test]
fn unbounded_force_capability_allows_materialization() {
    let mut cx = Cx::stub();
    cx.grant(list_force_unbounded_capability());
    let list = GuardList::new(Ordering::Greater);

    let values = force_list_to_vec(&mut cx, &list, "encode").unwrap();

    assert_eq!(values.len(), 1);
    assert_eq!(list.to_vec_calls.load(AtomicOrdering::SeqCst), 1);
    assert_eq!(*list.seen_limit.lock().unwrap(), Some(None));
}
