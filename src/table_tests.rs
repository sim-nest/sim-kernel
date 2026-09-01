use std::{sync::Arc, thread};

use crate::{
    AssocTable, Cx, Error, Expr, Symbol, Table, TableExpected, TableObserved, TableRegistry,
    TableReplacement,
};

#[test]
fn assoc_table_basic() {
    let mut cx = Cx::stub();
    let table = AssocTable::new();
    let one = cx.factory().bool(true).unwrap();

    table.set(&mut cx, Symbol::new("a"), one.clone()).unwrap();
    assert!(table.has(&mut cx, Symbol::new("a")).unwrap());
    assert_eq!(table.len(&mut cx).unwrap(), 1);
    assert_eq!(table.get(&mut cx, Symbol::new("a")).unwrap(), one);
    table.del(&mut cx, Symbol::new("a")).unwrap();
    assert_eq!(table.len(&mut cx).unwrap(), 0);
}

// guards the table catalog lookup contract
#[test]
fn assoc_table_missing_get_returns_nil() {
    let mut cx = Cx::stub();
    let table = AssocTable::new();

    let missing = table.get(&mut cx, Symbol::new("missing")).unwrap();

    assert_eq!(missing.object().as_expr(&mut cx).unwrap(), Expr::Nil);
}

#[test]
fn assoc_table_returns_error_for_poisoned_lock() {
    let mut cx = Cx::stub();
    let table = Arc::new(AssocTable::new());
    let poisoned = Arc::clone(&table);

    assert!(
        thread::spawn(move || {
            let _guard = poisoned.entries.write().unwrap();
            panic!("poison assoc table lock");
        })
        .join()
        .is_err()
    );
    assert!(table.entries.is_poisoned());

    let err = table.len(&mut cx).unwrap_err();
    assert!(matches!(
        err,
        Error::Eval(message) if message == "assoc table lock poisoned"
    ));
}

#[test]
fn table_registry_defaults_to_assoc_backend() {
    let registry = TableRegistry::new();
    assert_eq!(registry.active(), "assoc");
}

#[test]
fn assoc_compare_exchange_has_one_winner_and_distinguishes_nil() {
    let table = Arc::new(AssocTable::new());
    let key = Symbol::new("lease");
    let mut workers = Vec::new();
    for owner in ["one", "two"] {
        let table = Arc::clone(&table);
        let key = key.clone();
        workers.push(thread::spawn(move || {
            let mut cx = Cx::stub();
            let value = cx.factory().string(owner.to_owned()).unwrap();
            table
                .compare_exchange(
                    &mut cx,
                    key,
                    TableExpected::Absent,
                    TableReplacement::Value(value),
                )
                .unwrap()
                .exchanged
        }));
    }
    assert_eq!(
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .filter(|won| *won)
            .count(),
        1
    );

    let mut cx = Cx::stub();
    let observed = table
        .compare_exchange(
            &mut cx,
            key.clone(),
            TableExpected::Absent,
            TableReplacement::Delete,
        )
        .unwrap();
    assert!(!observed.exchanged);
    assert!(matches!(observed.observed, TableObserved::Value(_)));

    let nil = cx.factory().nil().unwrap();
    table.set(&mut cx, key.clone(), nil).unwrap();
    assert!(
        table
            .compare_exchange(
                &mut cx,
                key.clone(),
                TableExpected::Value(Expr::Nil),
                TableReplacement::Delete,
            )
            .unwrap()
            .exchanged
    );
    assert!(!table.has(&mut cx, key).unwrap());
}
