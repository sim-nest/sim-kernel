use std::collections::BTreeMap;

use crate::{Error, Expr, NumberLiteral, Result, Symbol};

use super::{
    CatalogRow, CatalogSnapshot, CatalogSnapshotRow, CatalogTableSpec, CatalogWritePolicy,
};

const SNAPSHOT_VERSION: u64 = 1;

pub(super) fn snapshot_to_expr(snapshot: &CatalogSnapshot) -> Expr {
    Expr::Map(vec![
        (field_expr("kind"), Expr::Symbol(snapshot_kind())),
        (field_expr("version"), u64_expr(SNAPSHOT_VERSION)),
        (field_expr("epoch"), u64_expr(snapshot.epoch)),
        (field_expr("tables"), tables_expr(&snapshot.tables)),
        (field_expr("sequences"), sequences_expr(&snapshot.sequences)),
        (field_expr("rows"), rows_expr(&snapshot.rows)),
    ])
}

pub(super) fn snapshot_from_expr(expr: Expr) -> Result<CatalogSnapshot> {
    let mut fields = expect_map(expr, "catalog snapshot")?;
    expect_exact_symbol(take_field(&mut fields, "kind")?, snapshot_kind(), "kind")?;
    let version = expect_u64(take_field(&mut fields, "version")?, "version")?;
    if version != SNAPSHOT_VERSION {
        return Err(snapshot_error(format!(
            "unsupported catalog snapshot version {version}"
        )));
    }
    let epoch = expect_u64(take_field(&mut fields, "epoch")?, "epoch")?;
    let tables = parse_tables(take_field(&mut fields, "tables")?)?;
    let sequences = parse_sequences(take_field(&mut fields, "sequences")?)?;
    let rows = parse_rows(take_field(&mut fields, "rows")?)?;
    expect_empty(fields, "catalog snapshot")?;
    Ok(CatalogSnapshot {
        tables,
        rows,
        sequences,
        epoch,
    })
}

pub(super) fn unresolved_live_expr(row: &CatalogRow, field: &Symbol) -> Expr {
    Expr::Extension {
        tag: Symbol::qualified("catalog", "unresolved-live"),
        payload: Box::new(Expr::Map(vec![
            (field_expr("table"), Expr::Symbol(row.table.clone())),
            (field_expr("key"), Expr::Symbol(row.key.clone())),
            (field_expr("field"), Expr::Symbol(field.clone())),
            (field_expr("epoch"), u64_expr(row.epoch)),
            (
                field_expr("kind"),
                row.data
                    .get(&Symbol::new("kind"))
                    .cloned()
                    .unwrap_or(Expr::Nil),
            ),
            (
                field_expr("symbol"),
                row.data
                    .get(&Symbol::new("symbol"))
                    .cloned()
                    .unwrap_or(Expr::Nil),
            ),
            (
                field_expr("display"),
                Expr::String(format!("unresolved live field {field}")),
            ),
        ])),
    }
}

fn tables_expr(tables: &BTreeMap<Symbol, CatalogTableSpec>) -> Expr {
    Expr::Vector(tables.values().map(table_expr).collect())
}

fn table_expr(spec: &CatalogTableSpec) -> Expr {
    Expr::Map(vec![
        (field_expr("name"), Expr::Symbol(spec.name.clone())),
        (
            field_expr("policy"),
            Expr::Symbol(policy_symbol(spec.policy)),
        ),
        (
            field_expr("owner"),
            spec.owner
                .as_ref()
                .map_or(Expr::Nil, |symbol| Expr::Symbol(symbol.clone())),
        ),
        (
            field_expr("required-fields"),
            Expr::Vector(
                spec.required_fields
                    .iter()
                    .cloned()
                    .map(Expr::Symbol)
                    .collect(),
            ),
        ),
        (
            field_expr("unique-fields"),
            Expr::Vector(
                spec.unique_fields
                    .iter()
                    .map(|fields| Expr::Vector(fields.iter().cloned().map(Expr::Symbol).collect()))
                    .collect(),
            ),
        ),
    ])
}

fn sequences_expr(sequences: &BTreeMap<Symbol, u64>) -> Expr {
    Expr::Vector(
        sequences
            .iter()
            .map(|(name, next)| {
                Expr::Map(vec![
                    (field_expr("name"), Expr::Symbol(name.clone())),
                    (field_expr("next"), u64_expr(*next)),
                ])
            })
            .collect(),
    )
}

fn rows_expr(rows: &BTreeMap<Symbol, BTreeMap<Symbol, CatalogSnapshotRow>>) -> Expr {
    Expr::Vector(
        rows.values()
            .flat_map(BTreeMap::values)
            .map(row_expr)
            .collect(),
    )
}

fn row_expr(row: &CatalogSnapshotRow) -> Expr {
    Expr::Map(vec![
        (field_expr("table"), Expr::Symbol(row.table.clone())),
        (field_expr("key"), Expr::Symbol(row.key.clone())),
        (field_expr("epoch"), u64_expr(row.epoch)),
        (field_expr("data"), data_expr(&row.data)),
    ])
}

fn data_expr(data: &BTreeMap<Symbol, Expr>) -> Expr {
    Expr::Map(
        data.iter()
            .map(|(field, value)| (Expr::Symbol(field.clone()), value.clone()))
            .collect(),
    )
}

fn parse_tables(expr: Expr) -> Result<BTreeMap<Symbol, CatalogTableSpec>> {
    expect_vector(expr, "tables")?
        .into_iter()
        .map(parse_table)
        .try_fold(BTreeMap::new(), |mut tables, spec| {
            let spec = spec?;
            if tables.insert(spec.name.clone(), spec).is_some() {
                return Err(snapshot_error("duplicate table spec"));
            }
            Ok(tables)
        })
}

fn parse_table(expr: Expr) -> Result<CatalogTableSpec> {
    let mut fields = expect_map(expr, "table spec")?;
    let name = expect_symbol(take_field(&mut fields, "name")?, "name")?;
    let policy = parse_policy(expect_symbol(take_field(&mut fields, "policy")?, "policy")?)?;
    let owner = match take_field(&mut fields, "owner")? {
        Expr::Nil => None,
        Expr::Symbol(symbol) => Some(symbol),
        _ => return Err(snapshot_error("owner must be nil or symbol")),
    };
    let required_fields = parse_symbol_vector(
        take_field(&mut fields, "required-fields")?,
        "required-fields",
    )?;
    let unique_fields = parse_unique_fields(take_field(&mut fields, "unique-fields")?)?;
    expect_empty(fields, "table spec")?;
    Ok(CatalogTableSpec {
        name,
        policy,
        owner,
        required_fields,
        unique_fields,
    })
}

fn parse_sequences(expr: Expr) -> Result<BTreeMap<Symbol, u64>> {
    expect_vector(expr, "sequences")?
        .into_iter()
        .map(parse_sequence)
        .try_fold(BTreeMap::new(), |mut sequences, item| {
            let (name, next) = item?;
            if sequences.insert(name, next).is_some() {
                return Err(snapshot_error("duplicate sequence"));
            }
            Ok(sequences)
        })
}

fn parse_sequence(expr: Expr) -> Result<(Symbol, u64)> {
    let mut fields = expect_map(expr, "sequence")?;
    let name = expect_symbol(take_field(&mut fields, "name")?, "name")?;
    let next = expect_u64(take_field(&mut fields, "next")?, "next")?;
    expect_empty(fields, "sequence")?;
    Ok((name, next))
}

fn parse_rows(expr: Expr) -> Result<BTreeMap<Symbol, BTreeMap<Symbol, CatalogSnapshotRow>>> {
    let mut rows = BTreeMap::<Symbol, BTreeMap<Symbol, CatalogSnapshotRow>>::new();
    for expr in expect_vector(expr, "rows")? {
        let row = parse_row(expr)?;
        if rows
            .entry(row.table.clone())
            .or_default()
            .insert(row.key.clone(), row)
            .is_some()
        {
            return Err(snapshot_error("duplicate row"));
        }
    }
    Ok(rows)
}

fn parse_row(expr: Expr) -> Result<CatalogSnapshotRow> {
    let mut fields = expect_map(expr, "row")?;
    let table = expect_symbol(take_field(&mut fields, "table")?, "table")?;
    let key = expect_symbol(take_field(&mut fields, "key")?, "key")?;
    let epoch = expect_u64(take_field(&mut fields, "epoch")?, "epoch")?;
    let data = parse_data(take_field(&mut fields, "data")?)?;
    expect_empty(fields, "row")?;
    Ok(CatalogSnapshotRow {
        table,
        key,
        epoch,
        data,
    })
}

fn parse_data(expr: Expr) -> Result<BTreeMap<Symbol, Expr>> {
    let mut data = BTreeMap::new();
    for (key, value) in expect_map_entries(expr, "row data")? {
        let Expr::Symbol(field) = key else {
            return Err(snapshot_error("row data keys must be symbols"));
        };
        if data.insert(field, value).is_some() {
            return Err(snapshot_error("duplicate row data field"));
        }
    }
    Ok(data)
}

fn parse_unique_fields(expr: Expr) -> Result<Vec<Vec<Symbol>>> {
    expect_vector(expr, "unique-fields")?
        .into_iter()
        .map(|expr| parse_symbol_vector(expr, "unique field group"))
        .collect()
}

fn parse_symbol_vector(expr: Expr, context: &'static str) -> Result<Vec<Symbol>> {
    expect_vector(expr, context)?
        .into_iter()
        .map(|expr| expect_symbol(expr, context))
        .collect()
}

fn expect_map(expr: Expr, context: &'static str) -> Result<BTreeMap<Symbol, Expr>> {
    let entries = expect_map_entries(expr, context)?;
    let mut fields = BTreeMap::new();
    for (key, value) in entries {
        let Expr::Symbol(field) = key else {
            return Err(snapshot_error(format!("{context} keys must be symbols")));
        };
        if fields.insert(field, value).is_some() {
            return Err(snapshot_error(format!("{context} has duplicate field")));
        }
    }
    Ok(fields)
}

fn expect_map_entries(expr: Expr, context: &'static str) -> Result<Vec<(Expr, Expr)>> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(snapshot_error(format!("{context} must be a map"))),
    }
}

fn expect_vector(expr: Expr, context: &'static str) -> Result<Vec<Expr>> {
    match expr {
        Expr::Vector(items) => Ok(items),
        _ => Err(snapshot_error(format!("{context} must be a vector"))),
    }
}

fn take_field(fields: &mut BTreeMap<Symbol, Expr>, name: &'static str) -> Result<Expr> {
    fields
        .remove(&Symbol::new(name))
        .ok_or_else(|| snapshot_error(format!("missing {name}")))
}

fn expect_empty(fields: BTreeMap<Symbol, Expr>, context: &'static str) -> Result<()> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(snapshot_error(format!("{context} has unknown fields")))
    }
}

fn expect_symbol(expr: Expr, context: &'static str) -> Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(snapshot_error(format!("{context} must be a symbol"))),
    }
}

fn expect_exact_symbol(expr: Expr, expected: Symbol, context: &'static str) -> Result<()> {
    let symbol = expect_symbol(expr, context)?;
    if symbol == expected {
        Ok(())
    } else {
        Err(snapshot_error(format!(
            "unexpected {context} symbol {symbol}"
        )))
    }
}

fn expect_u64(expr: Expr, context: &'static str) -> Result<u64> {
    let Expr::Number(NumberLiteral { canonical, .. }) = expr else {
        return Err(snapshot_error(format!("{context} must be an integer")));
    };
    canonical
        .parse()
        .map_err(|_| snapshot_error(format!("{context} must be an unsigned integer")))
}

fn parse_policy(symbol: Symbol) -> Result<CatalogWritePolicy> {
    match symbol.as_qualified_str().as_str() {
        "mutable" => Ok(CatalogWritePolicy::Mutable),
        "sealed" => Ok(CatalogWritePolicy::Sealed),
        "append-only" => Ok(CatalogWritePolicy::AppendOnly),
        "derived" => Ok(CatalogWritePolicy::Derived),
        _ => Err(snapshot_error(format!(
            "unknown catalog write policy {symbol}"
        ))),
    }
}

fn policy_symbol(policy: CatalogWritePolicy) -> Symbol {
    Symbol::new(match policy {
        CatalogWritePolicy::Mutable => "mutable",
        CatalogWritePolicy::Sealed => "sealed",
        CatalogWritePolicy::AppendOnly => "append-only",
        CatalogWritePolicy::Derived => "derived",
    })
}

fn field_expr(name: &'static str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn u64_expr(value: u64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn snapshot_kind() -> Symbol {
    Symbol::qualified("catalog", "snapshot")
}

fn snapshot_error(message: impl Into<String>) -> Error {
    Error::CatalogSchema {
        table: Symbol::qualified("catalog", "snapshot"),
        message: message.into(),
    }
}
