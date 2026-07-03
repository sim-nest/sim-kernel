use std::collections::BTreeSet;

use crate::{
    Claim, ContentId, Cx, Datum, ExportKind, ExportRecord, ExportState, LoadedLib, Ref,
    RefResolver, RuntimeId, Symbol, TemporaryRefResolver, Value, card, core_call_op_key,
    core_class_symbol_op_key, core_dir_is_dir_op_key, core_expr_snapshot_op_key, core_force_op_key,
    core_list_items_op_key, core_number_domain_symbol_op_key, core_number_value_op_key,
    core_object_encoding_op_key, core_read_construct_op_key, core_realize_start_op_key,
    core_seq_close_op_key, core_seq_next_op_key, core_shape_check_term_op_key,
    core_shape_check_value_op_key, core_shape_describe_op_key, core_table_entries_op_key,
};

use crate::error::Result;
use crate::term::OpKey;

pub(crate) fn publish_loaded_lib_claims(cx: &mut Cx, loaded: &LoadedLib) -> Result<Vec<ContentId>> {
    let mut claim_ids = Vec::new();
    for record in &loaded.exports {
        let subject = Ref::Symbol(record.symbol.clone());
        record_claim_id(
            &mut claim_ids,
            insert_ref_claim(
                cx,
                &subject,
                card::card_kind_predicate(),
                export_kind_ref(&record.kind),
            )?,
        );
        record_claim_id(
            &mut claim_ids,
            insert_ref_claim(
                cx,
                &subject,
                exported_by_predicate(),
                Ref::Symbol(loaded.manifest.id.clone()),
            )?,
        );
        for capability in &loaded.manifest.capabilities {
            record_claim_id(
                &mut claim_ids,
                insert_string_claim(
                    cx,
                    &subject,
                    card::card_requires_predicate(),
                    capability.as_str().to_owned(),
                )?,
            );
        }
        if let Some(value) = export_value(cx, record) {
            claim_ids.extend(publish_callable_shape_claims(cx, &subject, &value)?);
            for op in operation_claims_for_value(&value) {
                record_claim_id(
                    &mut claim_ids,
                    insert_string_claim(cx, &subject, card::card_ops_predicate(), op)?,
                );
            }
        }
    }
    Ok(claim_ids)
}

fn record_claim_id(claim_ids: &mut Vec<ContentId>, claim_id: Option<ContentId>) {
    if let Some(claim_id) = claim_id {
        claim_ids.push(claim_id);
    }
}

pub fn exported_by_predicate() -> Symbol {
    core_symbol("exported-by")
}

fn export_kind_ref(kind: &ExportKind) -> Ref {
    let symbol = match kind.name() {
        Some(name) => core_symbol(name),
        None => kind.symbol().clone(),
    };
    Ref::Symbol(symbol)
}

fn export_value(cx: &Cx, record: &ExportRecord) -> Option<Value> {
    let ExportState::Resolved { id } = record.state else {
        return None;
    };
    match id {
        RuntimeId::Class(id) => cx.registry().class_value(id).cloned(),
        RuntimeId::Function(id) => cx.registry().function_value(id).cloned(),
        RuntimeId::Macro(id) => cx.registry().macro_value(id).cloned(),
        RuntimeId::Shape(id) => cx.registry().shape_value(id).cloned(),
        RuntimeId::Codec(id) => cx.registry().codec_value(id).cloned(),
        RuntimeId::NumberDomain(id) => cx.registry().number_domain_value(id).cloned(),
        RuntimeId::Site(id) => cx.registry().site_value(RuntimeId::Site(id)).cloned(),
        RuntimeId::Value => cx.registry().value_by_symbol(&record.symbol).cloned(),
    }
}

fn operation_claims_for_value(value: &Value) -> Vec<String> {
    let object = value.object();
    let mut ops = BTreeSet::new();

    for key in known_operation_keys() {
        if object.op(&key).is_some() {
            ops.insert(operation_key_text(&key));
        }
    }
    if object.as_callable().is_some() {
        ops.insert(operation_key_text(&core_call_op_key()));
    }
    if object.as_shape().is_some() {
        ops.insert(operation_key_text(&core_shape_check_value_op_key()));
        ops.insert(operation_key_text(&core_shape_check_term_op_key()));
        ops.insert(operation_key_text(&core_shape_describe_op_key()));
    }
    if object.as_class().is_some() {
        ops.insert(operation_key_text(&core_class_symbol_op_key()));
    }
    if object.as_object_encoder().is_some() {
        ops.insert(operation_key_text(&core_object_encoding_op_key()));
    }
    if object.as_read_constructor().is_some() {
        ops.insert(operation_key_text(&core_read_construct_op_key()));
    }
    if object.as_number_domain().is_some() {
        ops.insert(operation_key_text(&core_number_domain_symbol_op_key()));
    }
    if object.as_number_value().is_some() {
        ops.insert(operation_key_text(&core_number_value_op_key()));
    }
    if object.as_eval_fabric().is_some() {
        ops.insert(operation_key_text(&core_realize_start_op_key()));
    }
    if object.as_thunk().is_some() {
        ops.insert(operation_key_text(&core_force_op_key()));
    }
    if object.as_sequence().is_some() || object.as_stream().is_some() {
        ops.insert(operation_key_text(&core_seq_next_op_key()));
        ops.insert(operation_key_text(&core_seq_close_op_key()));
    }
    if object.as_list().is_some() {
        ops.insert(operation_key_text(&core_list_items_op_key()));
    }
    if object.as_table_impl().is_some() {
        ops.insert(operation_key_text(&core_table_entries_op_key()));
    }
    if object.as_dir().is_some() {
        ops.insert(operation_key_text(&core_dir_is_dir_op_key()));
    }

    ops.into_iter().collect()
}

fn publish_callable_shape_claims(
    cx: &mut Cx,
    subject: &Ref,
    value: &Value,
) -> Result<Vec<ContentId>> {
    let Some(callable) = value.object().as_callable() else {
        return Ok(Vec::new());
    };
    let mut claim_ids = Vec::new();
    let args = callable.browse_args_shape(cx)?;
    let result = callable.browse_result_shape(cx)?;
    let args_known = insert_shape_claim(
        cx,
        subject,
        card::card_args_predicate(),
        args.as_ref(),
        &mut claim_ids,
    )?;
    let result_known = insert_shape_claim(
        cx,
        subject,
        card::card_result_predicate(),
        result.as_ref(),
        &mut claim_ids,
    )?;
    record_claim_id(
        &mut claim_ids,
        insert_bool_claim(
            cx,
            subject,
            card::card_shape_known_predicate(),
            args_known && result_known,
        )?,
    );
    Ok(claim_ids)
}

fn insert_shape_claim(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    value: Option<&Value>,
    claim_ids: &mut Vec<ContentId>,
) -> Result<bool> {
    let Some(value) = value else {
        record_claim_id(
            claim_ids,
            insert_ref_claim(cx, subject, predicate, Ref::Symbol(core_symbol("Any")))?,
        );
        return Ok(false);
    };
    let Some(reference) = stable_shape_ref(cx, value)? else {
        record_claim_id(
            claim_ids,
            insert_ref_claim(cx, subject, predicate, Ref::Symbol(core_symbol("Any")))?,
        );
        return Ok(false);
    };
    record_claim_id(
        claim_ids,
        insert_ref_claim(cx, subject, predicate, reference)?,
    );
    Ok(true)
}

fn stable_shape_ref(cx: &mut Cx, value: &Value) -> Result<Option<Ref>> {
    if let Some(symbol) = value.object().as_shape().and_then(|shape| shape.symbol()) {
        return Ok(Some(Ref::Symbol(symbol)));
    }
    let mut resolver = TemporaryRefResolver::new();
    match resolver.ref_for_value(cx, value)? {
        Ref::Handle(_) => Ok(None),
        reference => Ok(Some(reference)),
    }
}

fn known_operation_keys() -> [OpKey; 17] {
    [
        core_call_op_key(),
        core_shape_check_value_op_key(),
        core_shape_check_term_op_key(),
        core_shape_describe_op_key(),
        core_class_symbol_op_key(),
        core_object_encoding_op_key(),
        core_read_construct_op_key(),
        core_number_domain_symbol_op_key(),
        core_number_value_op_key(),
        core_realize_start_op_key(),
        core_force_op_key(),
        core_seq_next_op_key(),
        core_seq_close_op_key(),
        core_list_items_op_key(),
        core_table_entries_op_key(),
        core_dir_is_dir_op_key(),
        core_expr_snapshot_op_key(),
    ]
}

fn operation_key_text(key: &OpKey) -> String {
    format!("{}/{}.v{}", key.namespace, key.name, key.version)
}

fn insert_ref_claim(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    object: Ref,
) -> Result<Option<ContentId>> {
    cx.insert_recorded_fact(Claim::public(subject.clone(), predicate, object))
        .map(|(_, id)| id)
}

fn insert_string_claim(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    object: String,
) -> Result<Option<ContentId>> {
    let claim = Claim::content_object(
        cx.datum_store_mut(),
        subject.clone(),
        predicate,
        Datum::String(object),
    )?;
    cx.insert_recorded_fact(claim).map(|(_, id)| id)
}

fn insert_bool_claim(
    cx: &mut Cx,
    subject: &Ref,
    predicate: Symbol,
    object: bool,
) -> Result<Option<ContentId>> {
    let claim = Claim::content_object(
        cx.datum_store_mut(),
        subject.clone(),
        predicate,
        Datum::Bool(object),
    )?;
    cx.insert_recorded_fact(claim).map(|(_, id)| id)
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name.to_owned())
}
