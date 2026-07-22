use std::fmt::Write;

use super::*;

fn number(value: &str) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_owned(),
    })
}

#[test]
fn data_only_expressions_convert_to_datum_and_back() {
    let expr = Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("name")),
            Expr::String("sim".to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("items")),
            Expr::Vector(vec![Expr::Nil, Expr::Bool(true)]),
        ),
    ]);

    let datum = Datum::try_from(expr.clone()).unwrap();
    let roundtrip = Expr::from(datum.clone());

    assert_eq!(roundtrip, expr);
    assert_eq!(Datum::try_from(roundtrip).unwrap(), datum);
}

#[test]
fn call_expression_fails_datum_conversion_with_clear_error() {
    let err = Datum::try_from(Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::new("+"))),
        args: vec![Expr::Bool(true)],
    })
    .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("datum expression"));
    assert!(message.contains("call expression"));
}

#[test]
fn map_hash_is_stable_independent_of_insertion_order() {
    let left = Datum::Map(vec![
        (Datum::Symbol(Symbol::new("a")), number("1.0")),
        (Datum::Symbol(Symbol::new("b")), Datum::Nil),
    ]);
    let right = Datum::Map(vec![
        (Datum::Symbol(Symbol::new("b")), Datum::Nil),
        (Datum::Symbol(Symbol::new("a")), number("1.0")),
    ]);

    assert_eq!(left.content_id().unwrap(), right.content_id().unwrap());
}

#[test]
fn set_hash_is_stable_independent_of_insertion_order() {
    let left = Datum::Set(vec![Datum::String("x".to_owned()), Datum::Bool(true)]);
    let right = Datum::Set(vec![Datum::Bool(true), Datum::String("x".to_owned())]);

    assert_eq!(left.content_id().unwrap(), right.content_id().unwrap());
}

#[test]
fn duplicate_set_entries_fail_canonicalization() {
    let err = Datum::Set(vec![Datum::Nil, Datum::Nil])
        .content_id()
        .unwrap_err();

    assert!(err.to_string().contains("duplicate datum set entry"));
}

#[test]
fn node_roundtrips_through_extension_expression() {
    let datum = Datum::Node {
        tag: Symbol::qualified("core", "example"),
        fields: vec![(Symbol::new("payload"), Datum::Bool(true))],
    };

    let expr = Expr::from(datum.clone());

    assert_eq!(Datum::try_from(expr).unwrap(), datum);
}

#[test]
fn content_id_uses_named_datum_algorithm() {
    assert_eq!(
        Datum::Nil.content_id().unwrap().algorithm,
        Symbol::qualified("core", "sha256-datum-v1")
    );
}

#[test]
fn sha256_matches_known_digest() {
    assert_eq!(
        to_hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_matches_empty_input_digest() {
    assert_eq!(
        to_hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_matches_one_block_boundary_digest() {
    assert_eq!(
        to_hex(&sha256(&[b'a'; 55])),
        "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
    );
}

#[test]
fn sha256_matches_multi_block_digest() {
    assert_eq!(
        to_hex(&sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for byte in bytes {
        write!(&mut out, "{byte:02x}").unwrap();
    }
    out
}
