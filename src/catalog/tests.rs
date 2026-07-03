use crate::{Cx, DefaultFactory, Error, Expr, Factory, Symbol};

use super::{
    CatalogEventOp, CatalogRow, CatalogStore, CatalogTableSpec, CatalogTx, CatalogWritePolicy,
    catalog_key, split_catalog_key,
};

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

fn commit_one(store: &mut CatalogStore, row: CatalogRow) -> u64 {
    let mut tx = CatalogTx::new();
    tx.put_row(row);
    tx.commit(store).unwrap()
}

#[test]
fn simple_catalog_key_round_trips() {
    let key = catalog_key("registry", &[]);

    assert_eq!(split_catalog_key(&key).unwrap(), vec!["registry"]);
}

#[test]
fn multi_part_catalog_key_round_trips() {
    let key = catalog_key("registry-export", &["function", "core/add"]);

    assert_eq!(
        split_catalog_key(&key).unwrap(),
        vec!["registry-export", "function", "core/add"]
    );
}

#[test]
fn slash_escapes_as_uppercase_percent_2f() {
    let key = catalog_key("registry", &["a/b"]);

    assert_eq!(key.as_qualified_str(), "registry/a%2Fb");
    assert_eq!(split_catalog_key(&key).unwrap(), vec!["registry", "a/b"]);
}

#[test]
fn percent_escapes_as_uppercase_percent_25() {
    let key = catalog_key("registry", &["a%b"]);

    assert_eq!(key.as_qualified_str(), "registry/a%25b");
    assert_eq!(split_catalog_key(&key).unwrap(), vec!["registry", "a%b"]);
}

#[test]
fn control_bytes_escape_and_round_trip() {
    let key = catalog_key("registry", &["line\nbreak", "nul\u{0}end"]);

    assert_eq!(key.as_qualified_str(), "registry/line%0Abreak/nul%00end");
    assert_eq!(
        split_catalog_key(&key).unwrap(),
        vec!["registry", "line\nbreak", "nul\u{0}end"]
    );
}

#[test]
fn spaces_round_trip_without_changing_components() {
    let key = catalog_key("registry", &["two words"]);

    assert_eq!(key.as_qualified_str(), "registry/two words");
    assert_eq!(
        split_catalog_key(&key).unwrap(),
        vec!["registry", "two words"]
    );
}

#[test]
fn malformed_escape_returns_catalog_schema_error() {
    let err = split_catalog_key(&Symbol::new("registry/%XX")).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogSchema { table, message }
            if table == Symbol::new("catalog/key")
                && message == "malformed catalog key escape"
    ));
}

#[test]
fn mutable_policy_allows_insert_replace_and_delete() {
    let mut store = CatalogStore::new();
    let table = sym("mutable");
    let key = sym("row");
    store
        .install_table(spec("mutable", CatalogWritePolicy::Mutable))
        .unwrap();

    assert_eq!(commit_one(&mut store, row(&table, "row", true)), 1);
    assert_eq!(store.row(&table, &key).unwrap().epoch, 1);
    assert_eq!(
        store.row(&table, &key).unwrap().data.get(&required_field()),
        Some(&Expr::Bool(true))
    );

    assert_eq!(commit_one(&mut store, row(&table, "row", false)), 2);
    assert_eq!(store.row(&table, &key).unwrap().epoch, 2);
    assert_eq!(
        store.row(&table, &key).unwrap().data.get(&required_field()),
        Some(&Expr::Bool(false))
    );

    let mut tx = CatalogTx::new();
    tx.delete_row(table.clone(), key.clone());
    assert_eq!(tx.commit(&mut store).unwrap(), 3);

    assert!(store.row(&table, &key).is_none());
    assert_eq!(store.epoch(), 3);
    assert_eq!(
        store
            .journal()
            .iter()
            .map(|event| &event.op)
            .collect::<Vec<_>>(),
        vec![
            &CatalogEventOp::PutRow {
                table: table.clone(),
                key: key.clone()
            },
            &CatalogEventOp::PutRow {
                table: table.clone(),
                key: key.clone()
            },
            &CatalogEventOp::DeleteRow { table, key },
        ]
    );
}

#[test]
fn sealed_policy_rejects_overwrite_and_delete() {
    let mut store = CatalogStore::new();
    let table = sym("sealed");
    let key = sym("row");
    store
        .install_table(spec("sealed", CatalogWritePolicy::Sealed))
        .unwrap();
    commit_one(&mut store, row(&table, "row", true));

    let mut overwrite = CatalogTx::new();
    overwrite.put_row(row(&table, "row", false));
    let err = overwrite.commit(&mut store).unwrap_err();
    assert!(matches!(
        err,
        Error::CatalogConflict {
            table: conflict_table,
            key: conflict_key,
        } if conflict_table == table && conflict_key == key
    ));

    let mut delete = CatalogTx::new();
    delete.delete_row(table.clone(), key.clone());
    let err = delete.commit(&mut store).unwrap_err();
    assert!(matches!(
        err,
        Error::CatalogReadOnly { table: read_only } if read_only == table
    ));
    assert_eq!(
        store.row(&table, &key).unwrap().data.get(&required_field()),
        Some(&Expr::Bool(true))
    );
    assert_eq!(store.epoch(), 1);
    assert_eq!(store.journal().len(), 1);
}

#[test]
fn append_only_policy_rejects_overwrite_and_delete() {
    let mut store = CatalogStore::new();
    let table = sym("append-only");
    let key = sym("row");
    store
        .install_table(spec("append-only", CatalogWritePolicy::AppendOnly))
        .unwrap();
    commit_one(&mut store, row(&table, "row", true));

    let mut overwrite = CatalogTx::new();
    overwrite.put_row(row(&table, "row", false));
    let err = overwrite.commit(&mut store).unwrap_err();
    assert!(matches!(
        err,
        Error::CatalogConflict {
            table: conflict_table,
            key: conflict_key,
        } if conflict_table == table && conflict_key == key
    ));

    let mut delete = CatalogTx::new();
    delete.delete_row(table.clone(), key.clone());
    let err = delete.commit(&mut store).unwrap_err();
    assert!(matches!(
        err,
        Error::CatalogReadOnly { table: read_only } if read_only == table
    ));
    assert!(store.row(&table, &key).is_some());
    assert_eq!(store.epoch(), 1);
    assert_eq!(store.journal().len(), 1);
}

#[test]
fn derived_policy_rejects_direct_writes() {
    let mut store = CatalogStore::new();
    let table = sym("derived");
    let key = sym("row");
    store
        .install_table(spec("derived", CatalogWritePolicy::Derived))
        .unwrap();

    let mut put = CatalogTx::new();
    put.put_row(row(&table, "row", true));
    let err = put.commit(&mut store).unwrap_err();
    assert!(matches!(
        err,
        Error::CatalogReadOnly { table: read_only } if read_only == table
    ));

    let mut delete = CatalogTx::new();
    delete.delete_row(table.clone(), key);
    let err = delete.commit(&mut store).unwrap_err();
    assert!(matches!(
        err,
        Error::CatalogReadOnly { table: read_only } if read_only == table
    ));
    assert_eq!(store.epoch(), 0);
    assert!(store.journal().is_empty());
}

#[test]
fn unknown_table_is_a_schema_error() {
    let mut store = CatalogStore::new();
    let table = sym("missing-table");
    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "row", true));

    let err = tx.commit(&mut store).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogSchema {
            table: schema_table,
            message,
        } if schema_table == table && message == "unknown catalog table"
    ));
    assert_eq!(store.epoch(), 0);
    assert!(store.journal().is_empty());
}

#[test]
fn live_cells_can_satisfy_required_fields() {
    let mut store = CatalogStore::new();
    let table = sym("live-required");
    let key = sym("row");
    let field = required_field();
    let value = DefaultFactory.bool(true).unwrap();
    store
        .install_table(spec("live-required", CatalogWritePolicy::Mutable))
        .unwrap();
    let mut row = CatalogRow::new(table.clone(), key.clone());
    row.insert_live_value(field.clone(), value.clone());

    commit_one(&mut store, row);

    assert_eq!(
        store.row(&table, &key).unwrap().live_value(&field),
        Some(&value)
    );
}

#[test]
fn duplicate_row_ops_in_one_transaction_fail_atomically() {
    let mut store = CatalogStore::new();
    let table = sym("duplicate-ops");
    let key = sym("row");
    store
        .install_table(spec("duplicate-ops", CatalogWritePolicy::Mutable))
        .unwrap();
    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "row", true));
    tx.put_row(row(&table, "row", false));

    let err = tx.commit(&mut store).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogConflict {
            table: conflict_table,
            key: conflict_key,
        } if conflict_table == table && conflict_key == key
    ));
    assert!(store.row(&table, &key).is_none());
    assert_eq!(store.epoch(), 0);
    assert!(store.journal().is_empty());
}

#[test]
fn failed_transaction_leaves_no_partial_rows_sequences_or_journal() {
    let mut store = CatalogStore::new();
    let table = sym("atomic");
    let good_key = sym("good");
    let bad_key = sym("bad");
    let sequence = sym("registry-seq/lib");
    store
        .install_table(spec("atomic", CatalogWritePolicy::Mutable))
        .unwrap();
    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "good", true));
    tx.bump_sequence(sequence.clone(), 1);
    tx.put_row(CatalogRow::new(table.clone(), bad_key.clone()));

    let err = tx.commit(&mut store).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogSchema {
            table: schema_table,
            message,
        } if schema_table == table
            && message == "missing required catalog field required"
    ));
    assert!(store.row(&table, &good_key).is_none());
    assert!(store.row(&table, &bad_key).is_none());
    assert_eq!(store.sequence(&sequence), None);
    assert_eq!(store.epoch(), 0);
    assert!(store.journal().is_empty());
}

#[test]
fn commit_bumps_epoch_once_and_journals_operations_in_order() {
    let mut store = CatalogStore::new();
    let table = sym("journal-order");
    let first_key = sym("first");
    let second_key = sym("second");
    let sequence = sym("registry-seq/lib");
    store
        .install_table(spec("journal-order", CatalogWritePolicy::Mutable))
        .unwrap();
    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "first", true));
    tx.bump_sequence(sequence.clone(), 1);
    tx.put_row(row(&table, "second", false));

    let epoch = tx.commit(&mut store).unwrap();

    assert_eq!(epoch, 1);
    assert_eq!(store.epoch(), 1);
    assert_eq!(store.sequence(&sequence), Some(1));
    assert_eq!(store.row(&table, &first_key).unwrap().epoch, 1);
    assert_eq!(store.row(&table, &second_key).unwrap().epoch, 1);
    assert!(store.journal().iter().all(|event| event.epoch == 1));
    assert_eq!(
        store
            .journal()
            .iter()
            .map(|event| &event.op)
            .collect::<Vec<_>>(),
        vec![
            &CatalogEventOp::PutRow {
                table: table.clone(),
                key: first_key,
            },
            &CatalogEventOp::Sequence {
                name: sequence,
                next: 1,
            },
            &CatalogEventOp::PutRow {
                table,
                key: second_key,
            },
        ]
    );
}

#[test]
fn catalog_snapshot_carries_schema_rows_and_omits_live_payloads() {
    let mut cx = Cx::stub();
    let symbol = sym("snapshot-visible-value");
    let value = cx.factory().bool(true).unwrap();
    cx.registry_mut()
        .register_value(symbol.clone(), value)
        .unwrap();

    let snapshot = cx.registry().catalog_snapshot();
    let runtime_table = Symbol::qualified("registry", "runtime");
    let runtime_rows = snapshot.rows.get(&runtime_table).unwrap();
    let row = runtime_rows
        .values()
        .find(|row| row.data.get(&sym("symbol")) == Some(&Expr::Symbol(symbol.clone())))
        .unwrap();

    assert!(snapshot.tables.contains_key(&runtime_table));
    assert!(matches!(
        row.data.get(&sym("value")),
        Some(Expr::Extension { tag, .. }) if tag == &Symbol::qualified("catalog", "unresolved-live")
    ));
}
