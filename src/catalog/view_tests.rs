use super::registry_catalog_view;
use crate::{Cx, Error, Expr, Symbol, registry_catalog_read_capability};

fn sym(name: &str) -> Symbol {
    Symbol::new(name)
}

#[test]
fn catalog_view_lists_registry_tables_opens_exports_and_shows_rows() {
    let mut cx = Cx::stub();
    let symbol = sym("catalog-view-value");
    let value = cx.factory().bool(true).unwrap();
    cx.registry_mut()
        .register_value(symbol.clone(), value)
        .unwrap();
    cx.grant_from_host(registry_catalog_read_capability());

    let root = registry_catalog_view(&mut cx).unwrap();
    let root_table = root.object().as_table_impl().unwrap();
    assert_eq!(root_table.keys(&mut cx).unwrap(), vec![sym("registry")]);

    let registry_dir = root
        .object()
        .as_dir()
        .unwrap()
        .opendir(&mut cx, sym("registry"))
        .unwrap()
        .unwrap();
    let registry_table = registry_dir.object().as_table_impl().unwrap();
    assert!(
        registry_table
            .keys(&mut cx)
            .unwrap()
            .contains(&sym("exports"))
    );

    let exports = registry_dir
        .object()
        .as_dir()
        .unwrap()
        .opendir(&mut cx, sym("exports"))
        .unwrap()
        .unwrap();
    let exports_table = exports.object().as_table_impl().unwrap();
    let rows = exports_table.entries(&mut cx).unwrap();
    let row = rows
        .into_iter()
        .find_map(
            |(_, value)| match value.object().as_expr(&mut cx).unwrap() {
                Expr::Map(entries)
                    if map_field(&entries, "symbol") == Some(&Expr::Symbol(symbol.clone())) =>
                {
                    Some(entries)
                }
                _ => None,
            },
        )
        .unwrap();

    assert_eq!(
        map_field(&row, "state"),
        Some(&Expr::Symbol(sym("resolved")))
    );
    assert_eq!(
        exports_table
            .get(&mut cx, sym("missing"))
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );
}

#[test]
fn catalog_view_mutations_fail_closed_and_typed_lookups_stay_unchanged() {
    let mut cx = Cx::stub();
    let symbol = sym("catalog-view-typed-value");
    let value = cx.factory().bool(true).unwrap();
    cx.registry_mut()
        .register_value(symbol.clone(), value.clone())
        .unwrap();

    assert!(matches!(
        registry_catalog_view(&mut cx),
        Err(Error::CapabilityDenied { capability })
            if capability == registry_catalog_read_capability()
    ));

    cx.grant_from_host(registry_catalog_read_capability());
    let root = registry_catalog_view(&mut cx).unwrap();
    assert_eq!(cx.registry().value_by_symbol(&symbol), Some(&value));

    let exports = root
        .object()
        .as_dir()
        .unwrap()
        .opendir(&mut cx, Symbol::qualified("registry", "exports"))
        .unwrap()
        .unwrap();
    let exports_table = exports.object().as_table_impl().unwrap();
    let nil = cx.factory().nil().unwrap();
    assert!(matches!(
        exports_table.set(&mut cx, sym("new"), nil),
        Err(Error::CatalogReadOnly { table })
            if table == Symbol::qualified("registry", "exports")
    ));
    assert!(matches!(
        root.object().as_dir().unwrap().mkdir(&mut cx, sym("new")),
        Err(Error::CatalogReadOnly { .. })
    ));

    assert_eq!(cx.registry().value_by_symbol(&symbol), Some(&value));
}

fn map_field<'a>(entries: &'a [(Expr, Expr)], field: &str) -> Option<&'a Expr> {
    entries
        .iter()
        .find_map(|(key, value)| (key == &Expr::Symbol(sym(field))).then_some(value))
}
