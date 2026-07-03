use crate::{
    Result, Symbol,
    catalog::{CatalogStore, CatalogTableSpec, CatalogWritePolicy, catalog_key},
};

pub(crate) fn schema_table() -> Symbol {
    table_symbol("schema")
}

pub(crate) fn sequences_table() -> Symbol {
    table_symbol("sequences")
}

pub(crate) fn libs_table() -> Symbol {
    table_symbol("libs")
}

pub(crate) fn exports_table() -> Symbol {
    table_symbol("exports")
}

pub(crate) fn runtime_table() -> Symbol {
    table_symbol("runtime")
}

pub(crate) fn tests_table() -> Symbol {
    table_symbol("tests")
}

pub(crate) fn tests_by_lib_table() -> Symbol {
    table_symbol("tests-by-lib")
}

pub(crate) fn number_ops_table() -> Symbol {
    table_symbol("number-ops")
}

pub(crate) fn promotion_rules_table() -> Symbol {
    table_symbol("promotion-rules")
}

pub(crate) fn value_promotion_rules_table() -> Symbol {
    table_symbol("value-promotion-rules")
}

pub(crate) fn install_registry_catalog_schema(store: &mut CatalogStore) -> Result<()> {
    for spec in registry_catalog_specs() {
        store.install_table(spec)?;
    }
    Ok(())
}

pub(crate) fn registry_catalog_specs() -> Vec<CatalogTableSpec> {
    vec![
        table_spec(
            schema_table(),
            CatalogWritePolicy::Sealed,
            &["name", "policy"],
        ),
        table_spec(
            sequences_table(),
            CatalogWritePolicy::Mutable,
            &["kind", "next"],
        ),
        table_spec(
            libs_table(),
            CatalogWritePolicy::Sealed,
            &[
                "id",
                "symbol",
                "version",
                "abi-major",
                "abi-minor",
                "target",
                "trusted",
            ],
        ),
        table_spec(
            exports_table(),
            CatalogWritePolicy::Sealed,
            &["kind", "symbol", "lib", "state"],
        ),
        table_spec(
            runtime_table(),
            CatalogWritePolicy::Sealed,
            &["kind", "symbol", "value"],
        ),
        table_spec(
            tests_table(),
            CatalogWritePolicy::Sealed,
            &["symbol", "lib", "subjects", "test"],
        ),
        table_spec(
            tests_by_lib_table(),
            CatalogWritePolicy::Derived,
            &["lib", "tests"],
        ),
        table_spec(
            number_ops_table(),
            CatalogWritePolicy::AppendOnly,
            &["family", "operator", "ordinal"],
        ),
        table_spec(
            promotion_rules_table(),
            CatalogWritePolicy::AppendOnly,
            &["from", "to", "ordinal"],
        ),
        table_spec(
            value_promotion_rules_table(),
            CatalogWritePolicy::AppendOnly,
            &["from", "to", "ordinal"],
        ),
    ]
}

pub(crate) fn field(name: &'static str) -> Symbol {
    Symbol::new(name)
}

pub(crate) fn sequence_key(kind: &Symbol) -> Symbol {
    key("registry-seq", &[kind])
}

pub(crate) fn lib_key(symbol: &Symbol) -> Symbol {
    key("registry-lib", &[symbol])
}

pub(crate) fn export_key(kind: &Symbol, symbol: &Symbol) -> Symbol {
    key("registry-export", &[kind, symbol])
}

pub(crate) fn export_record_key(lib: &Symbol, kind: &Symbol, symbol: &Symbol) -> Symbol {
    key("registry-export-record", &[lib, kind, symbol])
}

pub(crate) fn runtime_key(kind: &Symbol, id: u64) -> Symbol {
    let id = id.to_string();
    catalog_key("registry-runtime", &[&kind.as_qualified_str(), &id])
}

pub(crate) fn plain_value_key(symbol: &Symbol) -> Symbol {
    key("registry-runtime", &[&Symbol::new("value"), symbol])
}

pub(crate) fn test_key(symbol: &Symbol) -> Symbol {
    key("registry-test", &[symbol])
}

pub(crate) fn number_op_key(family: &Symbol, operator: &Symbol, ordinal: u64) -> Symbol {
    let ordinal = ordinal.to_string();
    catalog_key(
        "registry-number-op",
        &[
            &family.as_qualified_str(),
            &operator.as_qualified_str(),
            &ordinal,
        ],
    )
}

pub(crate) fn promotion_rule_key(from: &Symbol, to: &Symbol, ordinal: u64) -> Symbol {
    let ordinal = ordinal.to_string();
    catalog_key(
        "registry-promotion",
        &[&from.as_qualified_str(), &to.as_qualified_str(), &ordinal],
    )
}

pub(crate) fn value_promotion_rule_key(from: &Symbol, to: &Symbol, ordinal: u64) -> Symbol {
    let ordinal = ordinal.to_string();
    catalog_key(
        "registry-value-promotion",
        &[&from.as_qualified_str(), &to.as_qualified_str(), &ordinal],
    )
}

fn table_symbol(name: &'static str) -> Symbol {
    Symbol::qualified("registry", name)
}

fn table_spec(
    name: Symbol,
    policy: CatalogWritePolicy,
    required_fields: &[&'static str],
) -> CatalogTableSpec {
    CatalogTableSpec::new(name, policy)
        .with_required_fields(required_fields.iter().copied().map(field).collect())
}

fn key(namespace: &str, parts: &[&Symbol]) -> Symbol {
    let parts = parts
        .iter()
        .map(|part| part.as_qualified_str())
        .collect::<Vec<_>>();
    let borrowed = parts.iter().map(String::as_str).collect::<Vec<_>>();
    catalog_key(namespace, &borrowed)
}

#[cfg(test)]
pub(crate) fn split_key(symbol: &Symbol) -> Vec<String> {
    crate::catalog::split_catalog_key(symbol).unwrap()
}
