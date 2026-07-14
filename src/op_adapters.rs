use std::sync::Arc;

use crate::{
    Args, Cx, Error, Expr, ObjectEncoding, OpKey, OpSpec, Ref, Result, ShapeDoc, Step, Symbol,
    Value,
    capability::{eval_fabric_capability, read_construct_capability},
    encode::ObjectEncoding as Encoding,
    eval::{Demand, EvalRequest},
    list::force_list_to_vec,
    op::{
        core_any_ref, core_call_op_key, core_class_symbol_op_key, core_dir_is_dir_op_key,
        core_expr_snapshot_op_key, core_force_op_key, core_list_items_op_key,
        core_number_domain_symbol_op_key, core_number_value_op_key, core_object_encoding_op_key,
        core_read_construct_op_key, core_realize_start_op_key, core_ref, core_seq_close_op_key,
        core_seq_next_op_key, core_shape_check_term_op_key, core_shape_check_value_op_key,
        core_shape_describe_op_key, core_symbol, core_table_entries_op_key,
    },
    realize::realize_final,
    ref_resolver::{RefResolver, TemporaryRefResolver},
    seq::{seq_close_value, seq_next_value, sequence_item_value},
    shape::shape_match_value,
};

pub(crate) struct AdapterOp<'a> {
    target: &'a Value,
    kind: AdapterKind,
    pub(crate) spec: OpSpec,
}

impl AdapterOp<'_> {
    pub(crate) fn invoke(&self, cx: &mut Cx, input: Value) -> Result<Step> {
        match self.kind {
            AdapterKind::Call => invoke_callable(cx, self.target, input),
            AdapterKind::ShapeCheckValue => invoke_shape_check_value(cx, self.target, input),
            AdapterKind::ShapeCheckTerm => invoke_shape_check_term(cx, self.target, input),
            AdapterKind::ShapeDescribe => invoke_shape_describe(cx, self.target),
            AdapterKind::ClassSymbol => invoke_class_symbol(cx, self.target),
            AdapterKind::ObjectEncoding => invoke_object_encoding(cx, self.target),
            AdapterKind::ReadConstruct => invoke_read_construct(cx, self.target, input),
            AdapterKind::NumberDomainSymbol => invoke_number_domain_symbol(cx, self.target),
            AdapterKind::NumberValue => invoke_number_value(cx, self.target),
            AdapterKind::RealizeStart => invoke_realize_start(cx, self.target, input),
            AdapterKind::Force => invoke_force(cx, self.target),
            AdapterKind::SeqNext => invoke_seq_next(cx, self.target),
            AdapterKind::SeqClose => invoke_seq_close(cx, self.target),
            AdapterKind::ListItems => invoke_list_items(cx, self.target),
            AdapterKind::TableEntries => invoke_table_entries(cx, self.target),
            AdapterKind::DirIsDir => invoke_dir_is_dir(cx, self.target, input),
            AdapterKind::ExprSnapshot => invoke_expr_snapshot(cx, self.target),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdapterKind {
    Call,
    ShapeCheckValue,
    ShapeCheckTerm,
    ShapeDescribe,
    ClassSymbol,
    ObjectEncoding,
    ReadConstruct,
    NumberDomainSymbol,
    NumberValue,
    RealizeStart,
    Force,
    SeqNext,
    SeqClose,
    ListItems,
    TableEntries,
    DirIsDir,
    ExprSnapshot,
}

const ADAPTER_KINDS: [AdapterKind; 17] = [
    AdapterKind::Call,
    AdapterKind::ShapeCheckValue,
    AdapterKind::ShapeCheckTerm,
    AdapterKind::ShapeDescribe,
    AdapterKind::ClassSymbol,
    AdapterKind::ObjectEncoding,
    AdapterKind::ReadConstruct,
    AdapterKind::NumberDomainSymbol,
    AdapterKind::NumberValue,
    AdapterKind::RealizeStart,
    AdapterKind::Force,
    AdapterKind::SeqNext,
    AdapterKind::SeqClose,
    AdapterKind::ListItems,
    AdapterKind::TableEntries,
    AdapterKind::DirIsDir,
    AdapterKind::ExprSnapshot,
];

impl AdapterKind {
    fn key(self) -> OpKey {
        match self {
            Self::Call => core_call_op_key(),
            Self::ShapeCheckValue => core_shape_check_value_op_key(),
            Self::ShapeCheckTerm => core_shape_check_term_op_key(),
            Self::ShapeDescribe => core_shape_describe_op_key(),
            Self::ClassSymbol => core_class_symbol_op_key(),
            Self::ObjectEncoding => core_object_encoding_op_key(),
            Self::ReadConstruct => core_read_construct_op_key(),
            Self::NumberDomainSymbol => core_number_domain_symbol_op_key(),
            Self::NumberValue => core_number_value_op_key(),
            Self::RealizeStart => core_realize_start_op_key(),
            Self::Force => core_force_op_key(),
            Self::SeqNext => core_seq_next_op_key(),
            Self::SeqClose => core_seq_close_op_key(),
            Self::ListItems => core_list_items_op_key(),
            Self::TableEntries => core_table_entries_op_key(),
            Self::DirIsDir => core_dir_is_dir_op_key(),
            Self::ExprSnapshot => core_expr_snapshot_op_key(),
        }
    }

    fn result_shape(self) -> Ref {
        match self {
            Self::ShapeCheckValue | Self::ShapeCheckTerm => core_ref("ShapeMatch"),
            Self::ShapeDescribe | Self::ObjectEncoding | Self::NumberValue | Self::TableEntries => {
                core_ref("Table")
            }
            Self::ClassSymbol | Self::NumberDomainSymbol => core_ref("Symbol"),
            Self::RealizeStart => core_ref("EvalReply"),
            Self::ReadConstruct | Self::Call | Self::Force | Self::SeqNext => core_any_ref(),
            Self::SeqClose => core_ref("Nil"),
            Self::ListItems => core_ref("List"),
            Self::DirIsDir => core_ref("Bool"),
            Self::ExprSnapshot => core_ref("Expr"),
        }
    }

    fn effects(self) -> Vec<Symbol> {
        match self {
            Self::RealizeStart | Self::Force => vec![core_symbol("eval")],
            Self::SeqNext | Self::SeqClose => vec![core_symbol("stream")],
            _ => Vec::new(),
        }
    }

    fn requirements(self) -> Vec<crate::CapabilityName> {
        match self {
            Self::RealizeStart => vec![eval_fabric_capability()],
            Self::ReadConstruct => vec![read_construct_capability()],
            _ => Vec::new(),
        }
    }
}

pub(crate) fn adapter_operation_keys_for_value(
    target: &Value,
    include_universal: bool,
) -> Vec<OpKey> {
    ADAPTER_KINDS
        .iter()
        .copied()
        .filter(|kind| adapter_kind_available(target, *kind, include_universal))
        .map(AdapterKind::key)
        .collect()
}

pub(crate) fn resolve_adapter<'a>(
    cx: &mut Cx,
    target: &'a Value,
    key: &OpKey,
) -> Result<Option<AdapterOp<'a>>> {
    let Some(kind) = adapter_kind(target, key) else {
        return Ok(None);
    };
    let subject = subject_ref(cx, target)?;
    let spec = OpSpec::new(kind.key(), subject, core_any_ref(), kind.result_shape())
        .with_effects(kind.effects())
        .with_requirements(kind.requirements());
    Ok(Some(AdapterOp { target, kind, spec }))
}

fn adapter_kind(target: &Value, key: &OpKey) -> Option<AdapterKind> {
    ADAPTER_KINDS
        .iter()
        .copied()
        .find(|kind| kind.key() == *key && adapter_kind_available(target, *kind, true))
}

fn adapter_kind_available(target: &Value, kind: AdapterKind, include_universal: bool) -> bool {
    match kind {
        AdapterKind::Call => target.object().as_callable().is_some(),
        AdapterKind::ShapeCheckValue | AdapterKind::ShapeCheckTerm | AdapterKind::ShapeDescribe => {
            target.object().as_shape().is_some()
        }
        AdapterKind::ClassSymbol => target.object().as_class().is_some(),
        AdapterKind::ObjectEncoding => target.object().as_object_encoder().is_some(),
        AdapterKind::ReadConstruct => target.object().as_read_constructor().is_some(),
        AdapterKind::NumberDomainSymbol => target.object().as_number_domain().is_some(),
        AdapterKind::NumberValue => target.object().as_number_value().is_some(),
        AdapterKind::RealizeStart => target.object().as_eval_fabric().is_some(),
        AdapterKind::Force => target.object().as_thunk().is_some(),
        AdapterKind::SeqNext | AdapterKind::SeqClose => target_is_sequence(target),
        AdapterKind::ListItems => target.object().as_list().is_some(),
        AdapterKind::TableEntries => target.object().as_table_impl().is_some(),
        AdapterKind::DirIsDir => target.object().as_dir().is_some(),
        AdapterKind::ExprSnapshot => include_universal,
    }
}

fn invoke_callable(cx: &mut Cx, target: &Value, input: Value) -> Result<Step> {
    let Some(callable) = target.object().as_callable() else {
        return Err(Error::TypeMismatch {
            expected: "callable",
            found: "non-callable",
        });
    };
    let Some(list) = input.object().as_list() else {
        return Err(Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    let args = force_list_to_vec(cx, list, "core/call.v1 input")?;
    callable.call(cx, Args::new(args)).map(Step::Value)
}

fn invoke_shape_check_value(cx: &mut Cx, target: &Value, input: Value) -> Result<Step> {
    let Some(shape) = target.object().as_shape() else {
        return Err(Error::TypeMismatch {
            expected: "shape",
            found: "non-shape",
        });
    };
    let matched = shape.check_value(cx, input)?;
    shape_match_value(cx, matched).map(Step::Value)
}

fn invoke_shape_check_term(cx: &mut Cx, target: &Value, input: Value) -> Result<Step> {
    let Some(shape) = target.object().as_shape() else {
        return Err(Error::TypeMismatch {
            expected: "shape",
            found: "non-shape",
        });
    };
    let expr = input.object().as_expr(cx)?;
    let matched = shape.check_expr(cx, &expr)?;
    shape_match_value(cx, matched).map(Step::Value)
}

fn invoke_shape_describe(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(shape) = target.object().as_shape() else {
        return Err(Error::TypeMismatch {
            expected: "shape",
            found: "non-shape",
        });
    };
    let doc = shape.describe(cx)?;
    shape_doc_value(cx, doc).map(Step::Value)
}

fn invoke_class_symbol(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(class) = target.object().as_class() else {
        return Err(Error::TypeMismatch {
            expected: "class",
            found: "non-class",
        });
    };
    cx.factory().symbol(class.symbol()).map(Step::Value)
}

fn invoke_object_encoding(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(encoder) = target.object().as_object_encoder() else {
        return Err(Error::TypeMismatch {
            expected: "object-encoder",
            found: "non-object-encoder",
        });
    };
    let encoding = encoder.object_encoding(cx)?;
    object_encoding_value(cx, encoding).map(Step::Value)
}

fn invoke_read_construct(cx: &mut Cx, target: &Value, input: Value) -> Result<Step> {
    let Some(read_constructor) = target.object().as_read_constructor() else {
        return Err(Error::TypeMismatch {
            expected: "read-constructor",
            found: "non-read-constructor",
        });
    };
    let Some(list) = input.object().as_list() else {
        return Err(Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    let args = force_list_to_vec(cx, list, "core/read-construct.v1 input")?;
    read_constructor.construct_read(cx, args).map(Step::Value)
}

fn invoke_number_domain_symbol(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(domain) = target.object().as_number_domain() else {
        return Err(Error::TypeMismatch {
            expected: "number-domain",
            found: "non-number-domain",
        });
    };
    cx.factory().symbol(domain.symbol()).map(Step::Value)
}

fn invoke_number_value(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(number) = target.object().as_number_value() else {
        return Err(Error::TypeMismatch {
            expected: "number-value",
            found: "non-number-value",
        });
    };
    let domain_symbol = number.number_domain(cx)?;
    let domain = cx.factory().symbol(domain_symbol)?;
    let literal = match number.number_literal(cx)? {
        Some(literal) => cx.factory().expr(Expr::Number(literal))?,
        None => cx.factory().nil()?,
    };
    cx.factory()
        .table(vec![
            (Symbol::new("domain"), domain),
            (Symbol::new("literal"), literal),
        ])
        .map(Step::Value)
}

fn invoke_realize_start(cx: &mut Cx, target: &Value, input: Value) -> Result<Step> {
    let Some(fabric) = target.object().as_eval_fabric() else {
        return Err(Error::TypeMismatch {
            expected: "eval-fabric",
            found: "non-eval-fabric",
        });
    };
    let Some(request) = input.object().downcast_ref::<EvalRequest>().cloned() else {
        return Err(Error::TypeMismatch {
            expected: "eval-request",
            found: "non-eval-request",
        });
    };
    let reply = realize_final(cx, fabric, request)?;
    cx.factory().opaque(Arc::new(reply)).map(Step::Value)
}

fn invoke_force(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(thunk) = target.object().as_thunk() else {
        return Err(Error::TypeMismatch {
            expected: "thunk",
            found: "non-thunk",
        });
    };
    thunk.force(cx, Demand::Value).map(Step::Value)
}

fn invoke_seq_next(cx: &mut Cx, target: &Value) -> Result<Step> {
    Ok(Step::Value(match seq_next_value(cx, target)? {
        Some(item) => sequence_item_value(cx, item)?,
        None => cx.factory().nil()?,
    }))
}

fn invoke_seq_close(cx: &mut Cx, target: &Value) -> Result<Step> {
    seq_close_value(cx, target)?;
    cx.factory().nil().map(Step::Value)
}

fn invoke_list_items(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(list) = target.object().as_list() else {
        return Err(Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    let items = force_list_to_vec(cx, list, "core/list-items.v1 target")?;
    cx.factory().list(items).map(Step::Value)
}

fn invoke_table_entries(cx: &mut Cx, target: &Value) -> Result<Step> {
    let Some(table) = target.object().as_table_impl() else {
        return Err(Error::TypeMismatch {
            expected: "table",
            found: "non-table",
        });
    };
    let mut values = Vec::new();
    for (key, value) in table.entries(cx)? {
        let pair = cx.factory().list(vec![cx.factory().symbol(key)?, value])?;
        values.push(pair);
    }
    cx.factory().list(values).map(Step::Value)
}

fn invoke_dir_is_dir(cx: &mut Cx, target: &Value, input: Value) -> Result<Step> {
    let Some(dir) = target.object().as_dir() else {
        return Err(Error::TypeMismatch {
            expected: "dir",
            found: "non-dir",
        });
    };
    let Expr::Symbol(name) = input.object().as_expr(cx)? else {
        return Err(Error::TypeMismatch {
            expected: "symbol",
            found: "non-symbol",
        });
    };
    let present = dir.is_dir(cx, name)?;
    cx.factory().bool(present).map(Step::Value)
}

fn invoke_expr_snapshot(cx: &mut Cx, target: &Value) -> Result<Step> {
    let expr = target.object().as_expr(cx)?;
    cx.factory().expr(expr).map(Step::Value)
}

fn object_encoding_value(cx: &mut Cx, encoding: ObjectEncoding) -> Result<Value> {
    match encoding {
        Encoding::Constructor { class, args } => {
            let kind = cx
                .factory()
                .symbol(Symbol::qualified("core", "constructor"))?;
            let class = cx.factory().symbol(class)?;
            let args = expr_list_value(cx, args)?;
            cx.factory().table(vec![
                (Symbol::new("kind"), kind),
                (Symbol::new("class"), class),
                (Symbol::new("args"), args),
            ])
        }
        Encoding::TaggedData { tag, fields } => {
            let kind = cx
                .factory()
                .symbol(Symbol::qualified("core", "tagged-data"))?;
            let tag = cx.factory().symbol(tag)?;
            let fields = expr_fields_value(cx, fields)?;
            cx.factory().table(vec![
                (Symbol::new("kind"), kind),
                (Symbol::new("tag"), tag),
                (Symbol::new("fields"), fields),
            ])
        }
        Encoding::Opaque { class, stable_id } => {
            let kind = cx.factory().symbol(Symbol::qualified("core", "opaque"))?;
            let class = cx.factory().symbol(class)?;
            let stable_id = cx.factory().string(stable_id)?;
            cx.factory().table(vec![
                (Symbol::new("kind"), kind),
                (Symbol::new("class"), class),
                (Symbol::new("stable-id"), stable_id),
            ])
        }
    }
}

fn expr_list_value(cx: &mut Cx, exprs: Vec<Expr>) -> Result<Value> {
    let values = exprs
        .into_iter()
        .map(|expr| cx.factory().expr(expr))
        .collect::<Result<Vec<_>>>()?;
    cx.factory().list(values)
}

fn expr_fields_value(cx: &mut Cx, fields: Vec<(Symbol, Expr)>) -> Result<Value> {
    let mut values = Vec::new();
    for (field, expr) in fields {
        values.push(
            cx.factory()
                .list(vec![cx.factory().symbol(field)?, cx.factory().expr(expr)?])?,
        );
    }
    cx.factory().list(values)
}

fn shape_doc_value(cx: &mut Cx, doc: ShapeDoc) -> Result<Value> {
    let details = doc
        .details
        .into_iter()
        .map(|detail| cx.factory().string(detail))
        .collect::<Result<Vec<_>>>()?;
    let details = cx.factory().list(details)?;
    cx.factory().table(vec![
        (Symbol::new("name"), cx.factory().string(doc.name)?),
        (Symbol::new("details"), details),
    ])
}

fn subject_ref(cx: &mut Cx, target: &Value) -> Result<Ref> {
    let mut resolver = TemporaryRefResolver::new();
    resolver.ref_for_value(cx, target)
}

fn target_is_sequence(target: &Value) -> bool {
    target.object().as_sequence().is_some() || target.object().as_stream().is_some()
}
