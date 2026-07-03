use std::collections::BTreeSet;

use crate::{
    Error, ExportKind, ExportRecord, ExportState, LibManifest, Result, RuntimeId, Symbol, Value,
    library::{Export, Registry},
};

use super::catalog::CatalogRuntimeValue;
use crate::library::transaction::PendingExports;

pub(super) struct LoadedLibPlan {
    pub(super) export_records: Vec<ExportRecord>,
    pub(super) runtime_values: Vec<CatalogRuntimeValue>,
}

pub(super) fn resolve_loaded_lib_plan(
    manifest: &LibManifest,
    pending: &PendingExports,
) -> Result<LoadedLibPlan> {
    let mut used_exports = BTreeSet::new();
    let mut used_records = BTreeSet::new();
    let mut plan = LoadedLibPlan {
        export_records: Vec::with_capacity(
            manifest.exports.len() + pending.exports.len() + pending.export_records.len(),
        ),
        runtime_values: Vec::new(),
    };

    for export in &manifest.exports {
        let kind = export.kind_symbol();
        let symbol = export.symbol().clone();
        if let Some((index, record)) = find_export_record(pending, &kind, &symbol) {
            used_records.insert(index);
            plan.export_records.push(record.clone());
            continue;
        }

        match find_pending_export(pending, &kind, &symbol) {
            Some((index, pending_export)) => {
                used_exports.insert(index);
                push_export_plan(&mut plan, pending_export, pending)?;
            }
            None => push_manifest_declaration(&mut plan, export)?,
        }
    }

    for (index, export) in pending.exports.iter().enumerate() {
        if !used_exports.contains(&index) {
            push_export_plan(&mut plan, export, pending)?;
        }
    }
    for (index, record) in pending.export_records.iter().enumerate() {
        if !used_records.contains(&index) {
            plan.export_records.push(record.clone());
        }
    }
    for record in &plan.export_records {
        Registry::validate_export_record_against_manifest(manifest, record)?;
    }
    Ok(plan)
}

fn find_export_record<'a>(
    pending: &'a PendingExports,
    kind: &ExportKind,
    symbol: &Symbol,
) -> Option<(usize, &'a ExportRecord)> {
    pending
        .export_records
        .iter()
        .enumerate()
        .find(|(_, record)| &record.kind == kind && &record.symbol == symbol)
}

fn find_pending_export<'a>(
    pending: &'a PendingExports,
    kind: &ExportKind,
    symbol: &Symbol,
) -> Option<(usize, &'a Export)> {
    pending
        .exports
        .iter()
        .enumerate()
        .find(|(_, export)| &export.kind_symbol() == kind && export.symbol() == symbol)
}

fn push_manifest_declaration(plan: &mut LoadedLibPlan, export: &Export) -> Result<()> {
    let kind = export.kind_symbol();
    let symbol = export.symbol().clone();
    match export {
        Export::Class {
            class_id: Some(_), ..
        } => missing_value(ExportKind::CLASS, &symbol),
        Export::Function {
            function_id: Some(_),
            ..
        } => missing_value(ExportKind::FUNCTION, &symbol),
        Export::Macro {
            macro_id: Some(_), ..
        } => missing_value(ExportKind::MACRO, &symbol),
        Export::Shape {
            shape_id: Some(_), ..
        } => missing_value(ExportKind::SHAPE, &symbol),
        Export::Codec {
            codec_id: Some(_), ..
        } => missing_value(ExportKind::CODEC, &symbol),
        Export::NumberDomain {
            number_domain_id: Some(_),
            ..
        } => missing_value(ExportKind::NUMBER_DOMAIN, &symbol),
        Export::Site {
            runtime_id: Some(_),
            ..
        } => missing_value(ExportKind::SITE, &symbol),
        _ => {
            plan.export_records.push(declared_record(kind, symbol));
            Ok(())
        }
    }
}

fn push_export_plan(
    plan: &mut LoadedLibPlan,
    export: &Export,
    pending: &PendingExports,
) -> Result<()> {
    let kind = export.kind_symbol();
    let symbol = export.symbol().clone();
    match export {
        Export::Class {
            class_id: Some(id), ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Class(*id),
            pending_class_value(pending, *id, &symbol)?,
        ),
        Export::Function {
            function_id: Some(id),
            ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Function(*id),
            pending_function_value(pending, *id, &symbol)?,
        ),
        Export::Macro {
            macro_id: Some(id), ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Macro(*id),
            pending_macro_value(pending, *id, &symbol)?,
        ),
        Export::Shape {
            shape_id: Some(id), ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Shape(*id),
            pending_shape_value(pending, *id, &symbol)?,
        ),
        Export::Codec {
            codec_id: Some(id), ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Codec(*id),
            pending_codec_value(pending, *id, &symbol)?,
        ),
        Export::NumberDomain {
            number_domain_id: Some(id),
            ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::NumberDomain(*id),
            pending_number_domain_value(pending, *id, &symbol)?,
        ),
        Export::Value { .. } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Value,
            pending_value(pending, &symbol)?,
        ),
        Export::Site {
            runtime_id: Some(RuntimeId::Site(id)),
            ..
        } => push_resolved(
            plan,
            kind,
            symbol.clone(),
            RuntimeId::Site(*id),
            pending_site_value(pending, *id, &symbol)?,
        ),
        Export::Site {
            runtime_id: Some(other),
            ..
        } => Err(Error::Lib(format!(
            "site export {symbol} has non-site runtime id {other:?}"
        ))),
        _ => {
            plan.export_records.push(declared_record(kind, symbol));
            Ok(())
        }
    }
}

fn push_resolved(
    plan: &mut LoadedLibPlan,
    kind: ExportKind,
    symbol: Symbol,
    runtime_id: RuntimeId,
    value: Value,
) -> Result<()> {
    plan.export_records
        .push(resolved_record(kind.clone(), symbol.clone(), runtime_id));
    plan.runtime_values.push(CatalogRuntimeValue {
        kind,
        symbol,
        runtime_id,
        value,
    });
    Ok(())
}

fn resolved_record(kind: ExportKind, symbol: Symbol, id: RuntimeId) -> ExportRecord {
    ExportRecord {
        kind,
        symbol,
        state: ExportState::Resolved { id },
    }
}

fn declared_record(kind: ExportKind, symbol: Symbol) -> ExportRecord {
    ExportRecord {
        kind,
        symbol,
        state: ExportState::Declared,
    }
}

fn pending_class_value(
    pending: &PendingExports,
    id: crate::ClassId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .class_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::CLASS, symbol))
}

fn pending_function_value(
    pending: &PendingExports,
    id: crate::FunctionId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .function_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::FUNCTION, symbol))
}

fn pending_macro_value(
    pending: &PendingExports,
    id: crate::MacroId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .macro_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::MACRO, symbol))
}

fn pending_shape_value(
    pending: &PendingExports,
    id: crate::ShapeId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .shape_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::SHAPE, symbol))
}

fn pending_codec_value(
    pending: &PendingExports,
    id: crate::CodecId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .codec_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::CODEC, symbol))
}

fn pending_number_domain_value(
    pending: &PendingExports,
    id: crate::NumberDomainId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .number_domain_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::NUMBER_DOMAIN, symbol))
}

fn pending_value(pending: &PendingExports, symbol: &Symbol) -> Result<Value> {
    pending
        .values
        .iter()
        .find_map(|(candidate, value)| (candidate == symbol).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::VALUE, symbol))
}

fn pending_site_value(
    pending: &PendingExports,
    id: crate::SiteId,
    symbol: &Symbol,
) -> Result<Value> {
    pending
        .site_value_cache
        .iter()
        .find_map(|(candidate, value)| (*candidate == id).then(|| value.clone()))
        .ok_or_else(|| missing_value_error(ExportKind::SITE, symbol))
}

fn missing_value(kind: &'static str, symbol: &Symbol) -> Result<()> {
    Err(missing_value_error(kind, symbol))
}

fn missing_value_error(kind: &'static str, symbol: &Symbol) -> Error {
    Error::Lib(format!("{kind} export {symbol} has no value"))
}
