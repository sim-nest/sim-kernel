use crate::catalog::CatalogTx;

use super::{
    number_binary_op_row, number_reduction_op_row, number_unary_op_row, promotion_rule_row,
    value_number_binary_op_row, value_number_reduction_op_row, value_number_unary_op_row,
    value_promotion_rule_row,
};
use crate::library::{Registry, transaction::PendingExports};

pub(super) fn push_number_op_rows(
    tx: &mut CatalogTx,
    registry: &Registry,
    pending: &PendingExports,
) {
    for (index, op) in pending.number_unary_ops.iter().cloned().enumerate() {
        tx.put_row(number_unary_op_row(
            ordinal(registry.number_unary_ops.len(), index),
            op,
        ));
    }
    for (index, op) in pending.number_reduction_ops.iter().cloned().enumerate() {
        tx.put_row(number_reduction_op_row(
            ordinal(registry.number_reduction_ops.len(), index),
            op,
        ));
    }
    for (index, op) in pending.number_binary_ops.iter().cloned().enumerate() {
        tx.put_row(number_binary_op_row(
            ordinal(registry.number_binary_ops.len(), index),
            op,
        ));
    }
    for (index, op) in pending.value_number_unary_ops.iter().cloned().enumerate() {
        tx.put_row(value_number_unary_op_row(
            ordinal(registry.value_number_unary_ops.len(), index),
            op,
        ));
    }
    for (index, op) in pending
        .value_number_reduction_ops
        .iter()
        .cloned()
        .enumerate()
    {
        tx.put_row(value_number_reduction_op_row(
            ordinal(registry.value_number_reduction_ops.len(), index),
            op,
        ));
    }
    for (index, op) in pending.value_number_binary_ops.iter().cloned().enumerate() {
        tx.put_row(value_number_binary_op_row(
            ordinal(registry.value_number_binary_ops.len(), index),
            op,
        ));
    }
}

pub(super) fn push_promotion_rule_rows(
    tx: &mut CatalogTx,
    registry: &Registry,
    pending: &PendingExports,
) {
    for (index, rule) in pending.promotion_rules.iter().cloned().enumerate() {
        tx.put_row(promotion_rule_row(
            ordinal(registry.promotion_rules.len(), index),
            rule,
        ));
    }
    for (index, rule) in pending.value_promotion_rules.iter().cloned().enumerate() {
        tx.put_row(value_promotion_rule_row(
            ordinal(registry.value_promotion_rules.len(), index),
            rule,
        ));
    }
}

fn ordinal(existing: usize, index: usize) -> u64 {
    u64::try_from(existing + index).expect("registry catalog ordinal exceeded u64")
}
