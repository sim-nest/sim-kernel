use crate::Symbol;

use super::{Fixity, PrattOperator, PrattResult, PrattTable, Token, parse_symbol};

fn table() -> PrattTable {
    let mut table = PrattTable::new();
    table.register(PrattOperator {
        symbol: Symbol::new("+"),
        fixity: Fixity::InfixLeft,
        left_bp: 50,
        right_bp: 51,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("*"),
        fixity: Fixity::InfixLeft,
        left_bp: 60,
        right_bp: 61,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("-"),
        fixity: Fixity::Prefix,
        left_bp: 0,
        right_bp: 90,
        result: PrattResult::ExprPrefix,
    });
    table
}

#[test]
fn pratt_table_keeps_protocol_operator_lookups_by_fixity() {
    let table = table();

    assert_eq!(
        table
            .lookup_led(&Token::Operator("+".to_owned()))
            .unwrap()
            .symbol,
        Symbol::new("+")
    );
    assert_eq!(
        table
            .lookup_led(&Token::Operator("*".to_owned()))
            .unwrap()
            .symbol,
        Symbol::new("*")
    );
    assert_eq!(
        table
            .lookup_nud(&Token::Operator("-".to_owned()))
            .unwrap()
            .fixity,
        Fixity::Prefix
    );
    assert!(table.lookup_led(&Token::Number("1".to_owned())).is_none());
}

#[test]
fn pratt_protocol_symbols_parse_plain_and_qualified_text() {
    assert_eq!(parse_symbol("value"), Symbol::new("value"));
    assert_eq!(parse_symbol("math/+"), Symbol::qualified("math", "+"));
    assert_eq!(parse_symbol("math.+"), Symbol::qualified("math", "+"));
}

#[test]
fn pratt_table_requires_registered_operator_by_fixity() {
    let table = table();

    assert!(table.require_infix(&Symbol::new("+")).is_ok());
    assert!(table.require_prefix(&Symbol::new("-")).is_ok());
    assert!(table.require_postfix(&Symbol::new("!")).is_err());
}
