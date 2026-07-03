use crate::{Cx, Error, Expr, NumberLiteral, Symbol};

use super::{
    CatalogRow, CatalogSnapshot, CatalogStore, CatalogTableSpec, CatalogTx, CatalogWritePolicy,
};

fn sym(name: &str) -> Symbol {
    Symbol::new(name)
}

fn table(name: &str) -> Symbol {
    Symbol::qualified("delta-test", name)
}

fn field(name: &str) -> Symbol {
    sym(name)
}

fn number(value: u64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn spec(name: &str, policy: CatalogWritePolicy) -> CatalogTableSpec {
    CatalogTableSpec::new(table(name), policy).with_required_fields(vec![field("value")])
}

fn row(table: &Symbol, key: &str, value: u64) -> CatalogRow {
    CatalogRow::new(table.clone(), sym(key)).with_data(field("value"), number(value))
}

fn commit_rows(store: &mut CatalogStore, rows: Vec<CatalogRow>) {
    let mut tx = CatalogTx::new();
    for row in rows {
        tx.put_row(row);
    }
    tx.commit(store).unwrap();
}

#[test]
fn delta_from_empty_to_loaded_registry_applies_cleanly() {
    let cx = Cx::stub();
    let loaded = CatalogStore::from_snapshot(cx.registry().catalog_snapshot()).unwrap();
    let delta = loaded.delta_since(0).unwrap();
    let mut replica = CatalogStore::new();

    replica.apply_delta(delta).unwrap();

    assert_eq!(
        CatalogSnapshot::from_store(&replica),
        CatalogSnapshot::from_store(&loaded)
    );
}

#[test]
fn stale_delta_is_rejected_without_mutating_target() {
    let mut source = CatalogStore::new();
    let table = table("stale");
    source
        .install_table(spec("stale", CatalogWritePolicy::Mutable))
        .unwrap();
    commit_rows(&mut source, vec![row(&table, "row", 1)]);
    let delta = source.delta_since(0).unwrap();
    let mut target = CatalogStore::new();
    target.epoch = 1;

    let before = CatalogSnapshot::from_store(&target);
    let err = target.apply_delta(delta).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogSchema { table, message }
            if table == Symbol::qualified("catalog", "delta")
                && message == "catalog delta source epoch mismatch"
    ));
    assert_eq!(CatalogSnapshot::from_store(&target), before);
}

#[test]
fn sealed_conflict_is_rejected_without_mutating_target() {
    let mut source = CatalogStore::new();
    let table = table("sealed");
    source
        .install_table(spec("sealed", CatalogWritePolicy::Sealed))
        .unwrap();
    commit_rows(&mut source, vec![row(&table, "row", 2)]);
    let delta = source.delta_since(0).unwrap();

    let mut target = CatalogStore::new();
    target
        .install_table(spec("sealed", CatalogWritePolicy::Sealed))
        .unwrap();
    target
        .rows
        .entry(table.clone())
        .or_default()
        .insert(sym("row"), row(&table, "row", 1));
    let before = CatalogSnapshot::from_store(&target);

    let err = target.apply_delta(delta).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogConflict {
            table: conflict_table,
            key
        } if conflict_table == table && key == sym("row")
    ));
    assert_eq!(CatalogSnapshot::from_store(&target), before);
}

#[test]
fn applying_incremental_delta_then_snapshot_equals_direct_snapshot() {
    let mut source = CatalogStore::new();
    let table = table("incremental");
    let sequence = sym("delta-seq/incremental");
    source
        .install_table(spec("incremental", CatalogWritePolicy::Mutable))
        .unwrap();
    commit_rows(&mut source, vec![row(&table, "first", 1)]);

    let mut replica = CatalogStore::new();
    replica.apply_delta(source.delta_since(0).unwrap()).unwrap();

    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "first", 2));
    tx.put_row(row(&table, "second", 3));
    tx.bump_sequence(sequence.clone(), 5);
    tx.commit(&mut source).unwrap();

    let delta = source.delta_since(replica.epoch()).unwrap();
    replica.apply_delta(delta).unwrap();

    assert_eq!(
        CatalogSnapshot::from_store(&replica),
        CatalogSnapshot::from_store(&source)
    );
    assert_eq!(replica.sequence(&sequence), Some(5));
}

#[test]
fn incompatible_table_specs_are_rejected() {
    let mut source = CatalogStore::new();
    let table = table("spec");
    source
        .install_table(spec("spec", CatalogWritePolicy::Mutable))
        .unwrap();
    commit_rows(&mut source, vec![row(&table, "row", 1)]);
    let delta = source.delta_since(0).unwrap();

    let mut target = CatalogStore::new();
    target
        .install_table(spec("spec", CatalogWritePolicy::Sealed))
        .unwrap();
    let err = target.apply_delta(delta).unwrap_err();

    assert!(matches!(
        err,
        Error::CatalogSchema {
            table: spec_table,
            message
        } if spec_table == table && message == "incompatible catalog table spec"
    ));
}
