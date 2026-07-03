use std::sync::Arc;

use crate::{
    ClassRef, Cx, Dir, Env, EvalFabric, EvalReply, EvalRequest, Expr, NumberDomain, NumberLiteral,
    NumberValue, Object, ObjectEncode, ObjectEncoding, ReadConstructor, Result, Sequence,
    SequenceItem, ShapeRef, Step, Stream, Symbol, Table, Value, core_class_symbol_op_key,
    core_dir_is_dir_op_key, core_expr_snapshot_op_key, core_force_op_key, core_list_items_op_key,
    core_number_domain_symbol_op_key, core_number_value_op_key, core_object_encoding_op_key,
    core_read_construct_op_key, core_realize_start_op_key, core_seq_next_op_key,
    core_table_entries_op_key, invoke_op, read_construct_capability, resolve_op,
};

fn qsym(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

struct EncodedObject;

impl Object for EncodedObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<encoded>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EncodedObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_EXPR_CLASS_ID, qsym("test", "Encoded"))
    }
    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for EncodedObject {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: qsym("test", "Encoded"),
            args: vec![Expr::String("arg".to_owned())],
        })
    }
}

struct ReadCtor;

impl Object for ReadCtor {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<read-ctor>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for ReadCtor {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_FUNCTION_CLASS_ID, qsym("core", "Function"))
    }
    fn as_read_constructor(&self) -> Option<&dyn ReadConstructor> {
        Some(self)
    }
}

impl ReadConstructor for ReadCtor {
    fn symbol(&self) -> Symbol {
        qsym("test", "ReadCtor")
    }

    fn args_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        cx.factory().nil()
    }

    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        cx.factory().list(args)
    }
}

struct TestDomain;

impl Object for TestDomain {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<test-domain>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for TestDomain {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            crate::CORE_NUMBER_DOMAIN_CLASS_ID,
            qsym("core", "NumberDomain"),
        )
    }
    fn as_number_domain(&self) -> Option<&dyn NumberDomain> {
        Some(self)
    }
}

impl NumberDomain for TestDomain {
    fn symbol(&self) -> Symbol {
        qsym("numbers", "test")
    }

    fn parse_literal(&self, _cx: &mut Cx, _text: &str) -> Result<Option<Value>> {
        Ok(None)
    }

    fn encode_literal(&self, _cx: &mut Cx, _value: Value) -> Result<Option<NumberLiteral>> {
        Ok(None)
    }
}

struct OpaqueNumber;

impl Object for OpaqueNumber {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("opaque-number".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for OpaqueNumber {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_NUMBER_CLASS_ID, qsym("core", "Number"))
    }
    fn as_number_value(&self) -> Option<&dyn NumberValue> {
        Some(self)
    }
}

impl NumberValue for OpaqueNumber {
    fn number_domain(&self, _cx: &mut Cx) -> Result<Symbol> {
        Ok(qsym("numbers", "opaque"))
    }
}

struct UnitFabric;

impl Object for UnitFabric {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<fabric>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for UnitFabric {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            crate::CORE_LOCAL_EVAL_FABRIC_CLASS_ID,
            qsym("core", "LocalEvalFabric"),
        )
    }
    fn as_eval_fabric(&self) -> Option<&dyn EvalFabric> {
        Some(self)
    }
}

impl EvalFabric for UnitFabric {
    fn realize(&self, cx: &mut Cx, request: EvalRequest) -> Result<EvalReply> {
        Ok(EvalReply {
            value: cx.factory().expr(request.expr)?,
            diagnostics: Vec::new(),
            trace: None,
        })
    }
}

struct EmptySequence;

impl Object for EmptySequence {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<sequence>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EmptySequence {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_SEQUENCE_CLASS_ID, qsym("core", "Sequence"))
    }
    fn as_sequence(&self) -> Option<&dyn Sequence> {
        Some(self)
    }
}

impl Sequence for EmptySequence {
    fn next_item(&self, _cx: &mut Cx) -> Result<Option<SequenceItem>> {
        Ok(None)
    }
}

struct EmptyStream;

impl Object for EmptyStream {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<stream>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for EmptyStream {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_SEQUENCE_CLASS_ID, qsym("core", "Stream"))
    }
    fn as_stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }
}

impl Stream for EmptyStream {
    fn next(&self, _cx: &mut Cx) -> Result<Option<Value>> {
        Ok(None)
    }
}

struct TestDir;

impl Object for TestDir {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<dir>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for TestDir {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(crate::CORE_TABLE_CLASS_ID, qsym("core", "Table"))
    }
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for TestDir {
    fn backend_symbol(&self) -> Symbol {
        qsym("test", "dir")
    }

    fn get(&self, cx: &mut Cx, _key: Symbol) -> Result<Value> {
        cx.factory().nil()
    }

    fn set(&self, _cx: &mut Cx, _key: Symbol, _value: Value) -> Result<()> {
        Ok(())
    }

    fn has(&self, _cx: &mut Cx, _key: Symbol) -> Result<bool> {
        Ok(false)
    }

    fn del(&self, cx: &mut Cx, _key: Symbol) -> Result<Value> {
        cx.factory().nil()
    }

    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(Vec::new())
    }

    fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        Ok(Vec::new())
    }

    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(0)
    }

    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        Ok(())
    }
}

impl Dir for TestDir {
    fn mkdir(&self, cx: &mut Cx, _name: Symbol) -> Result<Value> {
        cx.factory().nil()
    }

    fn opendir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Option<Value>> {
        Ok(None)
    }

    fn rmdir(&self, cx: &mut Cx, _name: Symbol) -> Result<Value> {
        cx.factory().nil()
    }

    fn is_dir(&self, _cx: &mut Cx, name: Symbol) -> Result<bool> {
        Ok(name == Symbol::new("child"))
    }
}

#[test]
fn k6_17_adapters_resolve_for_newly_deprecated_accessor_families() {
    let mut cx = Cx::stub();
    let cases = vec![
        (
            cx.factory()
                .class_stub(crate::CORE_CLASS_CLASS_ID, qsym("test", "Class"))
                .unwrap(),
            core_class_symbol_op_key(),
        ),
        (opaque(&cx, EncodedObject), core_object_encoding_op_key()),
        (opaque(&cx, ReadCtor), core_read_construct_op_key()),
        (opaque(&cx, TestDomain), core_number_domain_symbol_op_key()),
        (opaque(&cx, OpaqueNumber), core_number_value_op_key()),
        (opaque(&cx, UnitFabric), core_realize_start_op_key()),
        (
            opaque(
                &cx,
                crate::ThunkObject::new(Expr::String("later".to_owned()), Env::default()),
            ),
            core_force_op_key(),
        ),
        (opaque(&cx, EmptySequence), core_seq_next_op_key()),
        (opaque(&cx, EmptyStream), core_seq_next_op_key()),
        (
            cx.factory()
                .list(vec![cx.factory().bool(true).unwrap()])
                .unwrap(),
            core_list_items_op_key(),
        ),
        (
            cx.factory()
                .table(vec![(Symbol::new("key"), cx.factory().bool(true).unwrap())])
                .unwrap(),
            core_table_entries_op_key(),
        ),
        (opaque(&cx, TestDir), core_dir_is_dir_op_key()),
        (
            cx.factory().string("expr".to_owned()).unwrap(),
            core_expr_snapshot_op_key(),
        ),
    ];

    for (target, key) in cases {
        resolve_op(&mut cx, &target, &key).expect("adapter should resolve");
    }
}

#[test]
fn k6_17_adapters_invoke_legacy_protocols_without_new_object_methods() {
    let mut cx = Cx::stub();
    cx.grant(read_construct_capability());

    let class = cx
        .factory()
        .class_stub(crate::CORE_CLASS_CLASS_ID, qsym("test", "Class"))
        .unwrap();
    assert_eq!(
        expr_of(step_value(invoke_no_input(
            &mut cx,
            class,
            &core_class_symbol_op_key(),
        ))),
        Expr::Symbol(qsym("test", "Class"))
    );

    let encoded_target = opaque(&cx, EncodedObject);
    let encoded = step_value(invoke_no_input(
        &mut cx,
        encoded_target,
        &core_object_encoding_op_key(),
    ));
    assert!(matches!(
        map_value(&mut cx, &encoded, "kind"),
        Some(Expr::Symbol(symbol)) if symbol == qsym("core", "constructor")
    ));

    let arg = cx.factory().string("arg".to_owned()).unwrap();
    let input = cx.factory().list(vec![arg.clone()]).unwrap();
    let read_ctor = opaque(&cx, ReadCtor);
    assert_eq!(
        expr_of(step_value(
            invoke_op(&mut cx, read_ctor, &core_read_construct_op_key(), input,).unwrap(),
        )),
        Expr::List(vec![Expr::String("arg".to_owned())])
    );

    let number = cx
        .factory()
        .number_literal(qsym("numbers", "exact"), "7".to_owned())
        .unwrap();
    let number_view = step_value(invoke_no_input(
        &mut cx,
        number,
        &core_number_value_op_key(),
    ));
    assert!(matches!(
        map_value(&mut cx, &number_view, "domain"),
        Some(Expr::Symbol(symbol)) if symbol == qsym("numbers", "exact")
    ));

    let items = cx
        .factory()
        .list(vec![cx.factory().bool(true).unwrap()])
        .unwrap();
    assert_eq!(
        expr_of(step_value(invoke_no_input(
            &mut cx,
            items,
            &core_list_items_op_key(),
        ))),
        Expr::List(vec![Expr::Bool(true)])
    );

    let table = cx
        .factory()
        .table(vec![(
            Symbol::new("k"),
            cx.factory().string("v".to_owned()).unwrap(),
        )])
        .unwrap();
    assert_eq!(
        expr_of(step_value(invoke_no_input(
            &mut cx,
            table,
            &core_table_entries_op_key(),
        ))),
        Expr::List(vec![Expr::List(vec![
            Expr::Symbol(Symbol::new("k")),
            Expr::String("v".to_owned()),
        ])])
    );

    let dir_input = cx.factory().symbol(Symbol::new("child")).unwrap();
    let dir = opaque(&cx, TestDir);
    assert_eq!(
        expr_of(step_value(
            invoke_op(&mut cx, dir, &core_dir_is_dir_op_key(), dir_input,).unwrap(),
        )),
        Expr::Bool(true)
    );

    let snap = cx.factory().string("snap".to_owned()).unwrap();
    let expr = step_value(invoke_no_input(&mut cx, snap, &core_expr_snapshot_op_key()));
    assert_eq!(expr_of(expr), Expr::String("snap".to_owned()));
}

fn opaque<T: crate::RuntimeObject + 'static>(cx: &Cx, value: T) -> Value {
    cx.factory().opaque(Arc::new(value)).unwrap()
}

fn invoke_no_input(cx: &mut Cx, target: Value, key: &crate::OpKey) -> Step {
    let input = cx.factory().nil().unwrap();
    invoke_op(cx, target, key, input).unwrap()
}

fn step_value(step: Step) -> Value {
    match step {
        Step::Value(value) => value,
        Step::Events(_) | Step::Suspended(_) => panic!("expected value step"),
    }
}

fn expr_of(value: Value) -> Expr {
    value.object().as_expr(&mut Cx::stub()).unwrap()
}

fn map_value(cx: &mut Cx, value: &Value, key: &str) -> Option<Expr> {
    let Expr::Map(entries) = value.object().as_expr(cx).unwrap() else {
        panic!("expected map expression");
    };
    entries
        .into_iter()
        .find_map(|(candidate, value)| match candidate {
            Expr::Symbol(symbol) if symbol == Symbol::new(key) => Some(value),
            _ => None,
        })
}
