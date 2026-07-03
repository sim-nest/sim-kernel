use std::sync::Arc;

use crate::{
    ExportKind, ExportState, Expr, LibId, LibManifest, LibTarget, NumberBinaryOp, NumberLiteral,
    NumberReductionOp, NumberUnaryOp, PromotionRule, RuntimeId, Symbol, Test, Value,
    ValueNumberBinaryOp, ValueNumberReductionOp, ValueNumberUnaryOp, ValuePromotionRule,
    catalog::CatalogRow,
};

use super::schema::{
    export_key, export_record_key, exports_table, field, lib_key, libs_table, number_op_key,
    number_ops_table, plain_value_key, promotion_rule_key, promotion_rules_table, runtime_key,
    runtime_table, sequence_key, sequences_table, test_key, tests_table, value_promotion_rule_key,
    value_promotion_rules_table,
};

pub(crate) fn sequence_row(kind: Symbol, next: u64) -> CatalogRow {
    CatalogRow::new(sequences_table(), sequence_key(&kind))
        .with_data(field("kind"), Expr::Symbol(kind))
        .with_data(field("next"), integer_expr(next))
}

pub(crate) fn lib_row(id: LibId, manifest: &LibManifest, trusted: bool) -> CatalogRow {
    CatalogRow::new(libs_table(), lib_key(&manifest.id))
        .with_data(field("id"), integer_expr(u64::from(id.0)))
        .with_data(field("symbol"), Expr::Symbol(manifest.id.clone()))
        .with_data(field("version"), Expr::String(manifest.version.0.clone()))
        .with_data(
            field("abi-major"),
            integer_expr(u64::from(manifest.abi.major)),
        )
        .with_data(
            field("abi-minor"),
            integer_expr(u64::from(manifest.abi.minor)),
        )
        .with_data(
            field("target"),
            Expr::Symbol(lib_target_symbol(&manifest.target)),
        )
        .with_data(field("trusted"), Expr::Bool(trusted))
}

pub(crate) fn export_row(
    kind: ExportKind,
    symbol: Symbol,
    lib: Symbol,
    state: ExportState,
) -> CatalogRow {
    let kind_symbol = kind.symbol().clone();
    let key = match &state {
        ExportState::Resolved { .. } => export_key(&kind_symbol, &symbol),
        _ => export_record_key(&lib, &kind_symbol, &symbol),
    };
    let mut row = CatalogRow::new(exports_table(), key)
        .with_data(field("kind"), Expr::Symbol(kind_symbol))
        .with_data(field("symbol"), Expr::Symbol(symbol))
        .with_data(field("lib"), Expr::Symbol(lib));

    match state {
        ExportState::Resolved { id } => {
            let (runtime_kind, runtime_id) = runtime_id_fields(id);
            row = row
                .with_data(field("state"), Expr::Symbol(Symbol::new("resolved")))
                .with_data(field("runtime-kind"), Expr::Symbol(runtime_kind));
            if let Some(runtime_id) = runtime_id {
                row = row.with_data(field("runtime-id"), integer_expr(runtime_id));
            }
            row
        }
        ExportState::Declared => {
            row.with_data(field("state"), Expr::Symbol(Symbol::new("declared")))
        }
        ExportState::Unsupported { reason } => row
            .with_data(field("state"), Expr::Symbol(Symbol::new("unsupported")))
            .with_data(field("reason"), Expr::String(reason)),
        ExportState::Invalid { error } => row
            .with_data(field("state"), Expr::Symbol(Symbol::new("invalid")))
            .with_data(field("error"), Expr::String(error)),
    }
}

pub(crate) fn runtime_value_row(kind: Symbol, id: u64, symbol: Symbol, value: Value) -> CatalogRow {
    let mut row = CatalogRow::new(runtime_table(), runtime_key(&kind, id))
        .with_data(field("kind"), Expr::Symbol(kind))
        .with_data(field("id"), integer_expr(id))
        .with_data(field("symbol"), Expr::Symbol(symbol));
    row.insert_live_value(field("value"), value);
    row
}

pub(crate) fn plain_value_row(symbol: Symbol, value: Value) -> CatalogRow {
    let mut row = CatalogRow::new(runtime_table(), plain_value_key(&symbol))
        .with_data(field("kind"), Expr::Symbol(Symbol::new("value")))
        .with_data(field("symbol"), Expr::Symbol(symbol));
    row.insert_live_value(field("value"), value);
    row
}

pub(crate) fn test_row(
    symbol: Symbol,
    lib: Symbol,
    test: Arc<dyn Test>,
    subjects: Vec<Symbol>,
) -> CatalogRow {
    let mut row = CatalogRow::new(tests_table(), test_key(&symbol))
        .with_data(field("symbol"), Expr::Symbol(symbol))
        .with_data(field("lib"), Expr::Symbol(lib))
        .with_data(
            field("subjects"),
            Expr::List(subjects.into_iter().map(Expr::Symbol).collect()),
        );
    row.insert_live_test(field("test"), test);
    row
}

pub(crate) fn number_unary_op_row(ordinal: u64, op: NumberUnaryOp) -> CatalogRow {
    let family = Symbol::new("unary");
    let mut row = number_op_base_row(family, op.operator.clone(), ordinal)
        .with_data(
            field("operand-domain"),
            Expr::Symbol(op.operand_domain.clone()),
        )
        .with_data(field("cost"), integer_expr(u64::from(op.cost)));
    row.insert_live_number_unary_op(field("op"), op);
    row
}

pub(crate) fn number_reduction_op_row(ordinal: u64, op: NumberReductionOp) -> CatalogRow {
    let family = Symbol::new("reduction");
    let mut row = number_op_base_row(family, op.operator.clone(), ordinal)
        .with_data(
            field("operand-domain"),
            Expr::Symbol(op.operand_domain.clone()),
        )
        .with_data(field("cost"), integer_expr(u64::from(op.cost)));
    row.insert_live_number_reduction_op(field("op"), op);
    row
}

pub(crate) fn number_binary_op_row(ordinal: u64, op: NumberBinaryOp) -> CatalogRow {
    let family = Symbol::new("binary");
    let mut row = number_op_base_row(family, op.operator.clone(), ordinal)
        .with_data(field("left-domain"), Expr::Symbol(op.left_domain.clone()))
        .with_data(field("right-domain"), Expr::Symbol(op.right_domain.clone()))
        .with_data(field("cost"), integer_expr(u64::from(op.cost)));
    row.insert_live_number_binary_op(field("op"), op);
    row
}

pub(crate) fn value_number_unary_op_row(ordinal: u64, op: ValueNumberUnaryOp) -> CatalogRow {
    let family = Symbol::new("value-unary");
    let mut row = number_op_base_row(family, op.operator.clone(), ordinal)
        .with_data(
            field("operand-domain"),
            Expr::Symbol(op.operand_domain.clone()),
        )
        .with_data(field("cost"), integer_expr(u64::from(op.cost)));
    row.insert_live_value_number_unary_op(field("op"), op);
    row
}

pub(crate) fn value_number_reduction_op_row(
    ordinal: u64,
    op: ValueNumberReductionOp,
) -> CatalogRow {
    let family = Symbol::new("value-reduction");
    let mut row = number_op_base_row(family, op.operator.clone(), ordinal)
        .with_data(
            field("operand-domain"),
            Expr::Symbol(op.operand_domain.clone()),
        )
        .with_data(field("cost"), integer_expr(u64::from(op.cost)));
    row.insert_live_value_number_reduction_op(field("op"), op);
    row
}

pub(crate) fn value_number_binary_op_row(ordinal: u64, op: ValueNumberBinaryOp) -> CatalogRow {
    let family = Symbol::new("value-binary");
    let mut row = number_op_base_row(family, op.operator.clone(), ordinal)
        .with_data(field("left-domain"), Expr::Symbol(op.left_domain.clone()))
        .with_data(field("right-domain"), Expr::Symbol(op.right_domain.clone()))
        .with_data(field("cost"), integer_expr(u64::from(op.cost)));
    row.insert_live_value_number_binary_op(field("op"), op);
    row
}

pub(crate) fn promotion_rule_row(ordinal: u64, rule: PromotionRule) -> CatalogRow {
    let mut row = CatalogRow::new(
        promotion_rules_table(),
        promotion_rule_key(&rule.from_domain, &rule.to_domain, ordinal),
    )
    .with_data(field("from"), Expr::Symbol(rule.from_domain.clone()))
    .with_data(field("to"), Expr::Symbol(rule.to_domain.clone()))
    .with_data(field("ordinal"), integer_expr(ordinal))
    .with_data(field("cost"), integer_expr(u64::from(rule.cost)));
    row.insert_live_promotion_rule(field("rule"), rule);
    row
}

pub(crate) fn value_promotion_rule_row(ordinal: u64, rule: ValuePromotionRule) -> CatalogRow {
    let mut row = CatalogRow::new(
        value_promotion_rules_table(),
        value_promotion_rule_key(&rule.from_domain, &rule.to_domain, ordinal),
    )
    .with_data(field("from"), Expr::Symbol(rule.from_domain.clone()))
    .with_data(field("to"), Expr::Symbol(rule.to_domain.clone()))
    .with_data(field("ordinal"), integer_expr(ordinal))
    .with_data(field("cost"), integer_expr(u64::from(rule.cost)));
    row.insert_live_value_promotion_rule(field("rule"), rule);
    row
}

fn number_op_base_row(family: Symbol, operator: Symbol, ordinal: u64) -> CatalogRow {
    CatalogRow::new(
        number_ops_table(),
        number_op_key(&family, &operator, ordinal),
    )
    .with_data(field("family"), Expr::Symbol(family))
    .with_data(field("operator"), Expr::Symbol(operator))
    .with_data(field("ordinal"), integer_expr(ordinal))
}

fn integer_expr(value: u64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn lib_target_symbol(target: &LibTarget) -> Symbol {
    target.to_symbol()
}

fn runtime_id_fields(id: RuntimeId) -> (Symbol, Option<u64>) {
    match id {
        RuntimeId::Class(id) => (Symbol::new("class"), Some(u64::from(id.0))),
        RuntimeId::Function(id) => (Symbol::new("function"), Some(u64::from(id.0))),
        RuntimeId::Macro(id) => (Symbol::new("macro"), Some(u64::from(id.0))),
        RuntimeId::Shape(id) => (Symbol::new("shape"), Some(u64::from(id.0))),
        RuntimeId::Codec(id) => (Symbol::new("codec"), Some(u64::from(id.0))),
        RuntimeId::NumberDomain(id) => (Symbol::new("number-domain"), Some(u64::from(id.0))),
        RuntimeId::Site(id) => (Symbol::new("site"), Some(u64::from(id.0))),
        RuntimeId::Value => (Symbol::new("value"), None),
    }
}
