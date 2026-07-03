use crate::{Datum, Error, Expr, NumberLiteral, OpKey, QuoteMode, Ref, Symbol, Term};

fn sym(name: &str) -> Symbol {
    Symbol::new(name)
}

fn qsym(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn number(value: &str) -> NumberLiteral {
    NumberLiteral {
        domain: qsym("numbers", "f64"),
        canonical: value.to_owned(),
    }
}

fn lower(expr: Expr) -> Term {
    Term::try_from(expr).expect("expression should lower")
}

#[test]
fn scalar_data_lowers_to_literal_terms() {
    assert_eq!(lower(Expr::Nil), Term::Literal(Datum::Nil));
    assert_eq!(lower(Expr::Bool(true)), Term::Literal(Datum::Bool(true)));
    assert_eq!(
        lower(Expr::Number(number("1.0"))),
        Term::Literal(Datum::Number(number("1.0")))
    );
    assert_eq!(
        lower(Expr::String("hello".to_owned())),
        Term::Literal(Datum::String("hello".to_owned()))
    );
    assert_eq!(
        lower(Expr::Bytes(vec![1, 2, 3])),
        Term::Literal(Datum::Bytes(vec![1, 2, 3]))
    );
}

#[test]
fn symbol_lowers_to_ref_and_local_lowers_to_local() {
    assert_eq!(
        lower(Expr::Symbol(qsym("core", "add"))),
        Term::Ref(Ref::Symbol(qsym("core", "add")))
    );
    assert_eq!(lower(Expr::Local(sym("x"))), Term::Local(sym("x")));
}

#[test]
fn data_collections_lower_to_literal_terms() {
    assert_eq!(
        lower(Expr::List(vec![Expr::String("x".to_owned())])),
        Term::Literal(Datum::List(vec![Datum::String("x".to_owned())]))
    );
    assert_eq!(
        lower(Expr::Vector(vec![Expr::Bool(false)])),
        Term::Literal(Datum::Vector(vec![Datum::Bool(false)]))
    );
    assert_eq!(
        lower(Expr::Map(vec![(Expr::Symbol(sym("k")), Expr::Nil)])),
        Term::Literal(Datum::Map(vec![(Datum::Symbol(sym("k")), Datum::Nil)]))
    );
    assert_eq!(
        lower(Expr::Set(vec![Expr::Bytes(vec![9])])),
        Term::Literal(Datum::Set(vec![Datum::Bytes(vec![9])]))
    );
}

#[test]
fn call_lowers_operator_and_arguments() {
    let term = lower(Expr::Call {
        operator: Box::new(Expr::Symbol(qsym("core", "add"))),
        args: vec![Expr::Number(number("1")), Expr::Local(sym("x"))],
    });

    assert_eq!(
        term,
        Term::Call {
            target: Box::new(Term::Ref(Ref::Symbol(qsym("core", "add")))),
            args: vec![
                Term::Literal(Datum::Number(number("1"))),
                Term::Local(sym("x"))
            ],
        }
    );
}

#[test]
fn infix_prefix_and_postfix_lower_to_calls() {
    assert_eq!(
        lower(Expr::Infix {
            operator: qsym("core", "add"),
            left: Box::new(Expr::Number(number("1"))),
            right: Box::new(Expr::Number(number("2"))),
        }),
        Term::Call {
            target: Box::new(Term::Ref(Ref::Symbol(qsym("core", "add")))),
            args: vec![
                Term::Literal(Datum::Number(number("1"))),
                Term::Literal(Datum::Number(number("2"))),
            ],
        }
    );
    assert_eq!(
        lower(Expr::Prefix {
            operator: qsym("core", "neg"),
            arg: Box::new(Expr::Number(number("3"))),
        }),
        Term::Call {
            target: Box::new(Term::Ref(Ref::Symbol(qsym("core", "neg")))),
            args: vec![Term::Literal(Datum::Number(number("3")))],
        }
    );
    assert_eq!(
        lower(Expr::Postfix {
            operator: qsym("core", "factorial"),
            arg: Box::new(Expr::Number(number("4"))),
        }),
        Term::Call {
            target: Box::new(Term::Ref(Ref::Symbol(qsym("core", "factorial")))),
            args: vec![Term::Literal(Datum::Number(number("4")))],
        }
    );
}

#[test]
fn block_lowers_to_sequence() {
    assert_eq!(
        lower(Expr::Block(vec![Expr::Bool(true), Expr::Local(sym("x"))])),
        Term::Seq(vec![
            Term::Literal(Datum::Bool(true)),
            Term::Local(sym("x"))
        ])
    );
}

#[test]
fn quoted_data_lowers_to_quote() {
    assert_eq!(
        lower(Expr::Quote {
            mode: QuoteMode::Quote,
            expr: Box::new(Expr::List(vec![Expr::Symbol(sym("x"))])),
        }),
        Term::Quote {
            mode: QuoteMode::Quote,
            datum: Datum::List(vec![Datum::Symbol(sym("x"))]),
        }
    );
}

#[test]
fn quoted_executable_form_fails() {
    let err = Term::try_from(Expr::Quote {
        mode: QuoteMode::Quote,
        expr: Box::new(Expr::Call {
            operator: Box::new(Expr::Symbol(sym("f"))),
            args: Vec::new(),
        }),
    })
    .expect_err("quoted call should not lower as datum");

    assert!(matches!(
        err,
        Error::TypeMismatch {
            expected: "datum expression",
            found: "call expression",
        }
    ));
}

#[test]
fn annotated_terms_require_data_annotations() {
    assert_eq!(
        lower(Expr::Annotated {
            expr: Box::new(Expr::Local(sym("x"))),
            annotations: vec![(sym("doc"), Expr::String("value".to_owned()))],
        }),
        Term::Annotated {
            term: Box::new(Term::Local(sym("x"))),
            annotations: vec![(sym("doc"), Datum::String("value".to_owned()))],
        }
    );
}

#[test]
fn extension_lowers_to_data_payload() {
    assert_eq!(
        lower(Expr::Extension {
            tag: qsym("reader", "node"),
            payload: Box::new(Expr::Map(vec![(Expr::Symbol(sym("field")), Expr::Nil)])),
        }),
        Term::Extension {
            tag: qsym("reader", "node"),
            payload: Datum::Map(vec![(Datum::Symbol(sym("field")), Datum::Nil)]),
        }
    );
}

#[test]
fn collection_with_executable_child_fails_data_lowering() {
    let err = Term::try_from(Expr::List(vec![Expr::Call {
        operator: Box::new(Expr::Symbol(sym("f"))),
        args: Vec::new(),
    }]))
    .expect_err("list with call should not lower as datum");

    assert!(matches!(
        err,
        Error::TypeMismatch {
            expected: "datum expression",
            found: "call expression",
        }
    ));
}

#[test]
fn term_converts_back_to_compatible_expression() {
    let expr = Expr::from(Term::Call {
        target: Box::new(Term::Ref(Ref::Symbol(qsym("core", "add")))),
        args: vec![Term::Literal(Datum::Number(number("1")))],
    });

    assert_eq!(
        expr,
        Expr::Call {
            operator: Box::new(Expr::Symbol(qsym("core", "add"))),
            args: vec![Expr::Number(number("1"))],
        }
    );
}

#[test]
fn let_and_op_terms_convert_to_structured_extensions() {
    let let_expr = Expr::from(Term::Let {
        name: sym("x"),
        value: Box::new(Term::Literal(Datum::Bool(true))),
        body: Box::new(Term::Local(sym("x"))),
    });
    assert!(matches!(let_expr, Expr::Extension { .. }));

    let op_expr = Expr::from(Term::Op {
        op: OpKey::new(qsym("core", "shape"), qsym("core", "check-term"), 1),
        input: Box::new(Term::Local(sym("input"))),
    });
    assert!(matches!(op_expr, Expr::Extension { .. }));
}
