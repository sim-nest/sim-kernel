use std::collections::BTreeMap;

use crate::{DefaultFactory, Expr, Factory, NumberLiteral, Symbol};

use super::{
    CatalogReplay, CatalogRow, CatalogSnapshot, CatalogStore, CatalogTableSpec, CatalogTx,
    CatalogWritePolicy,
};

fn sym(name: &str) -> Symbol {
    Symbol::new(name)
}

fn field(name: &str) -> Symbol {
    sym(name)
}

fn table(name: &str) -> Symbol {
    Symbol::qualified("test", name)
}

fn spec(name: &str, fields: &[&str]) -> CatalogTableSpec {
    CatalogTableSpec::new(table(name), CatalogWritePolicy::Mutable)
        .with_required_fields(fields.iter().copied().map(field).collect())
}

fn number(value: u64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn row(table: &Symbol, key: &str, value: u64) -> CatalogRow {
    CatalogRow::new(table.clone(), sym(key)).with_data(field("value"), number(value))
}

fn put_rows(store: &mut CatalogStore, rows: Vec<CatalogRow>) {
    let mut tx = CatalogTx::new();
    for row in rows {
        tx.put_row(row);
    }
    tx.commit(store).unwrap();
}

#[test]
fn snapshot_expression_ordering_is_deterministic() {
    let mut store = CatalogStore::new();
    let alpha = table("alpha");
    let beta = table("beta");
    store.install_table(spec("beta", &["value"])).unwrap();
    store.install_table(spec("alpha", &["value"])).unwrap();

    let mut tx = CatalogTx::new();
    tx.put_row(row(&beta, "z", 3));
    tx.put_row(row(&alpha, "b", 2));
    tx.put_row(row(&alpha, "a", 1));
    tx.bump_sequence(sym("seq/b"), 8);
    tx.bump_sequence(sym("seq/a"), 4);
    tx.commit(&mut store).unwrap();

    let snapshot = CatalogSnapshot::from_store(&store);
    let expr = snapshot.to_expr();

    assert_eq!(snapshot.to_expr(), expr);
    assert_eq!(
        CatalogSnapshot::from_expr(expr.clone()).unwrap().to_expr(),
        expr
    );
    assert_eq!(
        snapshot_table_names(&expr),
        vec![alpha.clone(), beta.clone()]
    );
    assert_eq!(
        snapshot_row_keys(&expr),
        vec![
            (alpha.clone(), sym("a")),
            (alpha, sym("b")),
            (beta, sym("z")),
        ]
    );
    assert_eq!(
        snapshot_sequence_names(&expr),
        vec![sym("seq/a"), sym("seq/b")]
    );
}

#[test]
fn snapshot_from_expr_restores_data_rows_sequences_and_epoch() {
    let mut store = CatalogStore::new();
    let table = table("data");
    let sequence = sym("seq/data");
    store.install_table(spec("data", &["value"])).unwrap();
    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "row", 42));
    tx.bump_sequence(sequence.clone(), 7);
    tx.commit(&mut store).unwrap();

    let snapshot =
        CatalogSnapshot::from_expr(CatalogSnapshot::from_store(&store).to_expr()).unwrap();
    let restored = CatalogStore::from_snapshot(snapshot).unwrap();

    assert_eq!(restored.epoch(), store.epoch());
    assert_eq!(restored.sequence(&sequence), Some(7));
    assert_eq!(
        restored
            .row(&table, &sym("row"))
            .unwrap()
            .data
            .get(&field("value")),
        Some(&number(42))
    );
    assert!(restored.journal().is_empty());
}

#[test]
fn replay_reconstructs_data_only_rows() {
    let mut store = CatalogStore::new();
    let table = table("replay");
    let sequence = sym("seq/replay");
    store.install_table(spec("replay", &["value"])).unwrap();
    let mut tx = CatalogTx::new();
    tx.put_row(row(&table, "first", 1));
    tx.put_row(row(&table, "second", 2));
    tx.bump_sequence(sequence.clone(), 3);
    tx.commit(&mut store).unwrap();

    let snapshot = CatalogSnapshot::from_store(&store);
    let restored = CatalogStore::replay(CatalogReplay::from_snapshot(&snapshot)).unwrap();

    assert_eq!(restored.epoch(), store.epoch());
    assert_eq!(restored.sequence(&sequence), Some(3));
    assert_eq!(
        restored
            .rows(&table)
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![sym("first"), sym("second")]
    );
}

#[test]
fn live_only_rows_restore_as_unresolved_data() {
    let mut store = CatalogStore::new();
    let table = table("live");
    let value_field = field("value");
    store
        .install_table(spec("live", &["kind", "symbol", "value"]))
        .unwrap();
    let mut live_row = CatalogRow::new(table.clone(), sym("row"))
        .with_data(field("kind"), Expr::Symbol(sym("runtime")))
        .with_data(field("symbol"), Expr::Symbol(sym("live-symbol")));
    live_row.insert_live_value(value_field.clone(), DefaultFactory.bool(true).unwrap());
    put_rows(&mut store, vec![live_row]);

    let snapshot = CatalogSnapshot::from_store(&store);
    let marker = snapshot
        .rows(&table)
        .unwrap()
        .get(&sym("row"))
        .unwrap()
        .data
        .get(&value_field)
        .unwrap();

    assert_unresolved_marker(marker, &table, &sym("row"), &value_field);
    let restored = CatalogStore::from_snapshot(snapshot).unwrap();
    let restored_row = restored.row(&table, &sym("row")).unwrap();
    assert!(restored_row.live_value(&value_field).is_none());
    assert_unresolved_marker(
        restored_row.data.get(&value_field).unwrap(),
        &table,
        &sym("row"),
        &value_field,
    );
}

fn snapshot_table_names(expr: &Expr) -> Vec<Symbol> {
    vector_field(expr, "tables")
        .iter()
        .map(|expr| symbol_map_field(expr, "name"))
        .collect()
}

fn snapshot_sequence_names(expr: &Expr) -> Vec<Symbol> {
    vector_field(expr, "sequences")
        .iter()
        .map(|expr| symbol_map_field(expr, "name"))
        .collect()
}

fn snapshot_row_keys(expr: &Expr) -> Vec<(Symbol, Symbol)> {
    vector_field(expr, "rows")
        .iter()
        .map(|expr| {
            (
                symbol_map_field(expr, "table"),
                symbol_map_field(expr, "key"),
            )
        })
        .collect()
}

fn vector_field<'a>(expr: &'a Expr, field: &str) -> &'a Vec<Expr> {
    let Expr::Vector(items) = map_field(expr, field) else {
        panic!("expected vector field {field}");
    };
    items
}

fn symbol_map_field(expr: &Expr, field: &str) -> Symbol {
    let Expr::Symbol(symbol) = map_field(expr, field) else {
        panic!("expected symbol field {field}");
    };
    symbol.clone()
}

fn map_field<'a>(expr: &'a Expr, field: &str) -> &'a Expr {
    let Expr::Map(entries) = expr else {
        panic!("expected map");
    };
    entries
        .iter()
        .find_map(|(key, value)| match key {
            Expr::Symbol(symbol) if symbol == &sym(field) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing field {field}"))
}

fn assert_unresolved_marker(marker: &Expr, table: &Symbol, key: &Symbol, field: &Symbol) {
    let Expr::Extension { tag, payload } = marker else {
        panic!("expected unresolved extension marker");
    };
    assert_eq!(tag, &Symbol::qualified("catalog", "unresolved-live"));
    let payload = map_payload(payload);
    assert_eq!(
        payload.get(&sym("table")),
        Some(&Expr::Symbol(table.clone()))
    );
    assert_eq!(payload.get(&sym("key")), Some(&Expr::Symbol(key.clone())));
    assert_eq!(
        payload.get(&sym("field")),
        Some(&Expr::Symbol(field.clone()))
    );
    assert_eq!(
        payload.get(&sym("kind")),
        Some(&Expr::Symbol(sym("runtime")))
    );
    assert_eq!(
        payload.get(&sym("symbol")),
        Some(&Expr::Symbol(sym("live-symbol")))
    );
}

fn map_payload(expr: &Expr) -> BTreeMap<Symbol, Expr> {
    let Expr::Map(entries) = expr else {
        panic!("expected marker payload map");
    };
    entries
        .iter()
        .map(|(key, value)| {
            let Expr::Symbol(symbol) = key else {
                panic!("expected symbol payload key");
            };
            (symbol.clone(), value.clone())
        })
        .collect()
}
