use crate::{Error, Expr, Symbol};

use super::{CatalogRow, CatalogStore, CatalogTableSpec, CatalogTx, CatalogWritePolicy};

fn sym(name: &str) -> Symbol {
    Symbol::new(name)
}

fn required_field() -> Symbol {
    sym("required")
}

fn spec(name: &str, policy: CatalogWritePolicy) -> CatalogTableSpec {
    CatalogTableSpec::new(sym(name), policy).with_required_fields(vec![required_field()])
}

fn row(table: &Symbol, key: &str, value: bool) -> CatalogRow {
    CatalogRow::new(table.clone(), sym(key)).with_data(required_field(), Expr::Bool(value))
}

fn store_with_mutable_table(name: &str) -> CatalogStore {
    let mut store = CatalogStore::new();
    store
        .install_table(spec(name, CatalogWritePolicy::Mutable))
        .unwrap();
    store
}

#[test]
fn overlay_success_commits_rows_sequences_and_journal() {
    let table = sym("overlay-success");
    let key = sym("row");
    let sequence = sym("registry-seq/test");
    let mut store = store_with_mutable_table("overlay-success");

    store
        .with_overlay(|store| {
            let mut tx = CatalogTx::new();
            tx.put_row(row(&table, "row", true));
            tx.bump_sequence(sequence.clone(), 3);
            tx.commit(store)?;

            assert_eq!(
                store.row(&table, &key).unwrap().data.get(&required_field()),
                Some(&Expr::Bool(true))
            );
            assert_eq!(store.sequence(&sequence), Some(3));
            assert_eq!(store.journal().len(), 2);
            Ok(())
        })
        .unwrap();

    assert_eq!(store.epoch(), 1);
    assert_eq!(
        store.row(&table, &key).unwrap().data.get(&required_field()),
        Some(&Expr::Bool(true))
    );
    assert_eq!(store.sequence(&sequence), Some(3));
    assert_eq!(store.journal().len(), 2);
}

#[test]
fn overlay_error_rolls_back_rows_sequences_and_journal() {
    let table = sym("overlay-rollback");
    let key = sym("row");
    let sequence = sym("registry-seq/test");
    let mut store = store_with_mutable_table("overlay-rollback");

    let err = store
        .with_overlay(|store| {
            let mut tx = CatalogTx::new();
            tx.put_row(row(&table, "row", true));
            tx.bump_sequence(sequence.clone(), 9);
            tx.commit(store)?;

            assert!(store.row(&table, &key).is_some());
            assert_eq!(store.sequence(&sequence), Some(9));
            Err::<(), Error>(Error::Eval("rollback overlay".to_owned()))
        })
        .unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message == "rollback overlay"));
    assert!(store.row(&table, &key).is_none());
    assert_eq!(store.sequence(&sequence), None);
    assert_eq!(store.epoch(), 0);
    assert!(store.journal().is_empty());
}

#[test]
fn overlay_delete_is_local_until_commit() {
    let table = sym("overlay-delete");
    let key = sym("row");
    let mut store = store_with_mutable_table("overlay-delete");
    let mut seed = CatalogTx::new();
    seed.put_row(row(&table, "row", true));
    seed.commit(&mut store).unwrap();

    let err = store
        .with_overlay(|store| {
            let mut tx = CatalogTx::new();
            tx.delete_row(table.clone(), key.clone());
            tx.commit(store)?;
            assert!(store.row(&table, &key).is_none());
            Err::<(), Error>(Error::Eval("rollback delete".to_owned()))
        })
        .unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message == "rollback delete"));
    assert!(store.row(&table, &key).is_some());

    store
        .with_overlay(|store| {
            let mut tx = CatalogTx::new();
            tx.delete_row(table.clone(), key.clone());
            tx.commit(store)?;
            Ok(())
        })
        .unwrap();
    assert!(store.row(&table, &key).is_none());
}
