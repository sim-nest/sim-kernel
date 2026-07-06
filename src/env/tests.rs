use std::sync::{Arc, atomic::Ordering};

use crate::{
    DefaultFactory, Error, Expr, NumberBinaryOp, NumberLiteral, NumberReductionOp, NumberUnaryOp,
    PromotionRule, PromotionSearchLimits, Symbol, ValueNumberBinaryOp, ValuePromotionRule,
};

use super::Cx;
use super::test_support::*;

#[test]
fn sorted_number_domains_are_cached_and_invalidated() {
    let mut cx = Cx::stub();
    let alpha = Symbol::qualified("numbers", "alpha-test");
    let beta = Symbol::qualified("numbers", "beta-test");
    let gamma = Symbol::qualified("numbers", "gamma-test");
    let beta_value = domain_value(&mut cx, beta.clone(), 0);
    let alpha_value = domain_value(&mut cx, alpha.clone(), 10);

    cx.registry_mut()
        .register_number_domain_value(beta.clone(), beta_value)
        .unwrap();
    cx.registry_mut()
        .register_number_domain_value(alpha.clone(), alpha_value)
        .unwrap();

    let symbols = cx
        .registry_mut()
        .sorted_number_domains()
        .into_iter()
        .map(|(symbol, _)| symbol)
        .collect::<Vec<_>>();
    assert_eq!(symbols, vec![alpha.clone(), beta.clone()]);

    let gamma_value = domain_value(&mut cx, gamma.clone(), 5);
    cx.registry_mut()
        .register_number_domain_value(gamma.clone(), gamma_value)
        .unwrap();

    let symbols = cx
        .registry_mut()
        .sorted_number_domains()
        .into_iter()
        .map(|(symbol, _)| symbol)
        .collect::<Vec<_>>();
    assert_eq!(symbols, vec![alpha, gamma, beta]);
}

#[test]
fn binary_dispatch_reports_missing_promotion_path() {
    let mut cx = Cx::stub();
    let target = Symbol::qualified("numbers", "target-test");
    let target_value = domain_value(&mut cx, target.clone(), 0);

    cx.registry_mut()
        .register_number_domain_value(target.clone(), target_value)
        .unwrap();
    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: target.clone(),
        right_domain: target,
        cost: 0,
        apply: unreachable_binary_rule,
    });

    let error = cx
        .apply_number_binary_op(
            &Symbol::qualified("math", "add"),
            NumberLiteral {
                domain: Symbol::qualified("numbers", "left-test"),
                canonical: "1".to_owned(),
            },
            NumberLiteral {
                domain: Symbol::qualified("numbers", "right-test"),
                canonical: "2".to_owned(),
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        Error::NoPromotionPath {
            operator,
            left_domain,
            right_domain,
        } if operator == Symbol::qualified("math", "add")
            && left_domain == Symbol::qualified("numbers", "left-test")
            && right_domain == Symbol::qualified("numbers", "right-test")
    ));
}

#[test]
fn promotion_search_respects_state_limit() {
    let mut cx = Cx::new(
        Arc::new(crate::eval::NoopEvalPolicy),
        Arc::new(DefaultFactory),
    );
    let middle = Symbol::qualified("numbers", "middle-test");
    let target = Symbol::qualified("numbers", "target-test");
    let middle_value = domain_value(&mut cx, middle.clone(), 0);
    let target_value = domain_value(&mut cx, target.clone(), 0);

    cx.set_promotion_search_limits(PromotionSearchLimits {
        max_depth: 8,
        max_states: 1,
    });
    cx.registry_mut()
        .register_number_domain_value(middle.clone(), middle_value)
        .unwrap();
    cx.registry_mut()
        .register_number_domain_value(target.clone(), target_value)
        .unwrap();
    cx.registry_mut().register_promotion_rule(PromotionRule {
        from_domain: Symbol::qualified("numbers", "start-test"),
        to_domain: middle,
        cost: 0,
        convert: promote_to_middle,
    });
    cx.registry_mut().register_promotion_rule(PromotionRule {
        from_domain: Symbol::qualified("numbers", "middle-test"),
        to_domain: target.clone(),
        cost: 0,
        convert: promote_middle_to_target,
    });
    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: target.clone(),
        right_domain: target,
        cost: 0,
        apply: unreachable_binary_rule,
    });

    let error = cx
        .apply_number_binary_op(
            &Symbol::qualified("math", "add"),
            NumberLiteral {
                domain: Symbol::qualified("numbers", "start-test"),
                canonical: "1".to_owned(),
            },
            NumberLiteral {
                domain: Symbol::qualified("numbers", "start-test"),
                canonical: "2".to_owned(),
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        Error::PromotionSearchLimitExceeded {
            from_domain,
            target_domain,
            max_depth,
            max_states,
        } if from_domain == Symbol::qualified("numbers", "start-test")
            && target_domain == Symbol::qualified("numbers", "target-test")
            && max_depth == 8
            && max_states == 1
    ));
}

#[test]
fn number_binary_dispatch_picks_lowest_cost_without_applying_losers() {
    let _guard = NUMBER_DISPATCH_TEST_LOCK.lock().unwrap();
    reset_apply_counters();
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "slow-test");
    register_test_domain(&mut cx, "medium-a-test");
    register_test_domain(&mut cx, "medium-b-test");
    register_test_domain(&mut cx, "fast-test");

    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: Symbol::qualified("numbers", "slow-test"),
        right_domain: Symbol::qualified("numbers", "slow-test"),
        cost: 5,
        apply: high_cost_binary_rule,
    });
    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: Symbol::qualified("numbers", "medium-a-test"),
        right_domain: Symbol::qualified("numbers", "medium-a-test"),
        cost: 5,
        apply: high_cost_binary_rule,
    });
    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: Symbol::qualified("numbers", "fast-test"),
        right_domain: Symbol::qualified("numbers", "fast-test"),
        cost: 3,
        apply: winning_binary_rule,
    });

    let value = cx
        .apply_number_binary_op(
            &Symbol::qualified("math", "add"),
            number("fast-test", "1"),
            number("fast-test", "2"),
        )
        .unwrap();

    assert_eq!(
        value.object().as_expr(&mut cx).unwrap(),
        Expr::Number(number("fast-test", "fast"))
    );
    assert_eq!(HIGH_COST_BINARY_APPLIES.load(Ordering::SeqCst), 0);
    assert_eq!(WINNING_BINARY_APPLIES.load(Ordering::SeqCst), 1);
}

#[test]
fn number_binary_dispatch_reports_equal_minimal_cost_as_ambiguous() {
    let _guard = NUMBER_DISPATCH_TEST_LOCK.lock().unwrap();
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "fast-a-test");

    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: Symbol::qualified("numbers", "fast-a-test"),
        right_domain: Symbol::qualified("numbers", "fast-a-test"),
        cost: 3,
        apply: winning_binary_rule,
    });
    cx.registry_mut().register_number_binary_op(NumberBinaryOp {
        operator: Symbol::qualified("math", "add"),
        left_domain: Symbol::qualified("numbers", "fast-a-test"),
        right_domain: Symbol::qualified("numbers", "fast-a-test"),
        cost: 3,
        apply: winning_binary_rule,
    });

    let error = cx
        .apply_number_binary_op(
            &Symbol::qualified("math", "add"),
            number("fast-a-test", "1"),
            number("fast-a-test", "2"),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        Error::AmbiguousNumberDispatch { operator, candidates }
        if operator == Symbol::qualified("math", "add")
            && candidates.len() == 2
            && candidates.contains(&(
                Symbol::qualified("numbers", "fast-a-test"),
                Symbol::qualified("numbers", "fast-a-test")
            ))
    ));
}

#[test]
fn number_value_ref_prefers_protocol_and_falls_back_to_literal_encoding() {
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "value-start-test");
    register_test_domain(&mut cx, "f64");

    let opaque = opaque_number(&cx, "value-start-test", 7);
    let opaque_ref = cx.number_value_ref(opaque).unwrap().unwrap();
    assert_eq!(
        opaque_ref.domain,
        Symbol::qualified("numbers", "value-start-test")
    );
    assert!(opaque_ref.literal.is_none());

    let literal = cx
        .factory()
        .number_literal(Symbol::qualified("numbers", "f64"), "1.5".to_owned())
        .unwrap();
    let literal_ref = cx.number_value_ref(literal).unwrap().unwrap();
    assert_eq!(literal_ref.domain, Symbol::qualified("numbers", "f64"));
    assert_eq!(literal_ref.literal.unwrap(), number("f64", "1.5"));
}

#[test]
fn value_number_binary_dispatch_finds_multi_hop_promotion_paths() {
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "value-start-test");
    register_test_domain(&mut cx, "value-middle-test");
    register_test_domain(&mut cx, "value-target-test");

    cx.registry_mut()
        .register_value_promotion_rule(ValuePromotionRule {
            from_domain: Symbol::qualified("numbers", "value-start-test"),
            to_domain: Symbol::qualified("numbers", "value-middle-test"),
            cost: 1,
            convert: promote_value_to_middle,
        });
    cx.registry_mut()
        .register_value_promotion_rule(ValuePromotionRule {
            from_domain: Symbol::qualified("numbers", "value-middle-test"),
            to_domain: Symbol::qualified("numbers", "value-target-test"),
            cost: 1,
            convert: promote_value_middle_to_target,
        });
    cx.registry_mut()
        .register_value_number_binary_op(ValueNumberBinaryOp {
            operator: Symbol::qualified("math", "add"),
            left_domain: Symbol::qualified("numbers", "value-target-test"),
            right_domain: Symbol::qualified("numbers", "value-target-test"),
            cost: 0,
            apply: value_binary_rule,
        });

    let value = cx
        .apply_value_number_binary_op(
            &Symbol::qualified("math", "add"),
            opaque_number(&cx, "value-start-test", 2),
            opaque_number(&cx, "value-target-test", 3),
        )
        .unwrap();

    assert_eq!(read_opaque_number(&value).value, 5);
    assert_eq!(
        read_opaque_number(&value).domain,
        Symbol::qualified("numbers", "value-target-test")
    );
}

#[test]
fn value_number_binary_dispatch_reports_equal_minimal_cost_as_ambiguous() {
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "value-start-test");
    register_test_domain(&mut cx, "value-middle-test");
    register_test_domain(&mut cx, "value-target-test");
    register_test_domain(&mut cx, "value-alt-target-test");

    cx.registry_mut()
        .register_value_promotion_rule(ValuePromotionRule {
            from_domain: Symbol::qualified("numbers", "value-start-test"),
            to_domain: Symbol::qualified("numbers", "value-middle-test"),
            cost: 1,
            convert: promote_value_to_middle,
        });
    cx.registry_mut()
        .register_value_promotion_rule(ValuePromotionRule {
            from_domain: Symbol::qualified("numbers", "value-middle-test"),
            to_domain: Symbol::qualified("numbers", "value-target-test"),
            cost: 1,
            convert: promote_value_middle_to_target,
        });
    cx.registry_mut()
        .register_value_promotion_rule(ValuePromotionRule {
            from_domain: Symbol::qualified("numbers", "value-start-test"),
            to_domain: Symbol::qualified("numbers", "value-alt-target-test"),
            cost: 2,
            convert: promote_value_to_alt_target,
        });
    cx.registry_mut()
        .register_value_number_binary_op(ValueNumberBinaryOp {
            operator: Symbol::qualified("math", "add"),
            left_domain: Symbol::qualified("numbers", "value-target-test"),
            right_domain: Symbol::qualified("numbers", "value-target-test"),
            cost: 0,
            apply: value_binary_rule,
        });
    cx.registry_mut()
        .register_value_number_binary_op(ValueNumberBinaryOp {
            operator: Symbol::qualified("math", "add"),
            left_domain: Symbol::qualified("numbers", "value-alt-target-test"),
            right_domain: Symbol::qualified("numbers", "value-alt-target-test"),
            cost: 0,
            apply: value_binary_alt_rule,
        });

    let error = cx
        .apply_value_number_binary_op(
            &Symbol::qualified("math", "add"),
            opaque_number(&cx, "value-start-test", 1),
            opaque_number(&cx, "value-start-test", 2),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        Error::AmbiguousNumberDispatch { operator, candidates }
            if operator == Symbol::qualified("math", "add")
                && candidates.contains(&(
                    Symbol::qualified("numbers", "value-target-test"),
                    Symbol::qualified("numbers", "value-target-test")
                ))
                && candidates.contains(&(
                    Symbol::qualified("numbers", "value-alt-target-test"),
                    Symbol::qualified("numbers", "value-alt-target-test")
                ))
    ));
}

#[test]
fn number_unary_dispatch_applies_only_the_winning_rule_once() {
    let _guard = NUMBER_DISPATCH_TEST_LOCK.lock().unwrap();
    reset_apply_counters();
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "slow-test");
    register_test_domain(&mut cx, "fast-test");

    cx.registry_mut().register_number_unary_op(NumberUnaryOp {
        operator: Symbol::qualified("math", "neg"),
        operand_domain: Symbol::qualified("numbers", "slow-test"),
        cost: 5,
        apply: |_cx, _operand| {
            HIGH_COST_BINARY_APPLIES.fetch_add(1, Ordering::SeqCst);
            unreachable!("higher-cost unary candidate should never be applied")
        },
    });
    cx.registry_mut().register_number_unary_op(NumberUnaryOp {
        operator: Symbol::qualified("math", "neg"),
        operand_domain: Symbol::qualified("numbers", "fast-test"),
        cost: 3,
        apply: winning_unary_rule,
    });

    let value = cx
        .apply_number_unary_op(&Symbol::qualified("math", "neg"), number("fast-test", "2"))
        .unwrap();

    assert_eq!(
        value.object().as_expr(&mut cx).unwrap(),
        Expr::Number(number("fast-test", "neg"))
    );
    assert_eq!(HIGH_COST_BINARY_APPLIES.load(Ordering::SeqCst), 0);
    assert_eq!(WINNING_UNARY_APPLIES.load(Ordering::SeqCst), 1);
}

#[test]
fn number_reduction_dispatch_applies_only_the_winning_rule_once() {
    let _guard = NUMBER_DISPATCH_TEST_LOCK.lock().unwrap();
    reset_apply_counters();
    let mut cx = Cx::stub();
    register_test_domain(&mut cx, "slow-test");
    register_test_domain(&mut cx, "fast-test");

    cx.registry_mut()
        .register_number_reduction_op(NumberReductionOp {
            operator: Symbol::qualified("math", "sum"),
            operand_domain: Symbol::qualified("numbers", "slow-test"),
            cost: 5,
            apply: |_cx, _operands| {
                HIGH_COST_BINARY_APPLIES.fetch_add(1, Ordering::SeqCst);
                unreachable!("higher-cost reduction candidate should never be applied")
            },
        });
    cx.registry_mut()
        .register_number_reduction_op(NumberReductionOp {
            operator: Symbol::qualified("math", "sum"),
            operand_domain: Symbol::qualified("numbers", "fast-test"),
            cost: 3,
            apply: winning_reduction_rule,
        });

    let value = cx
        .apply_number_reduction_op(
            &Symbol::qualified("math", "sum"),
            vec![number("fast-test", "1"), number("fast-test", "2")],
        )
        .unwrap();

    assert_eq!(
        value.object().as_expr(&mut cx).unwrap(),
        Expr::Number(number("fast-test", "sum"))
    );
    assert_eq!(HIGH_COST_BINARY_APPLIES.load(Ordering::SeqCst), 0);
    assert_eq!(WINNING_REDUCTION_APPLIES.load(Ordering::SeqCst), 1);
}

#[test]
fn fork_from_seed_reproduces_capabilities_and_copied_content() {
    use crate::{CapabilityName, DefaultFactory, NoopEvalPolicy, capability::GrantSeat};

    let (mut seed, seat): (Cx, GrantSeat) =
        Cx::new_seated(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

    // A capability granted through the seed's host seat.
    let cap = CapabilityName::new("fork-test-cap");
    seat.grant(&mut seed, cap.clone());

    // Some copied env content and copied registry content.
    let binding = Symbol::qualified("fork", "binding-test");
    let bound_value = opaque_number(&seed, "fork-env-test", 7);
    seed.env_mut().define(binding.clone(), bound_value);
    let domain = Symbol::qualified("numbers", "fork-domain-test");
    register_test_domain(&mut seed, "fork-domain-test");

    let fork = seed.fork_from_seed();

    // The fork carries the same granted capabilities as its seed.
    assert!(fork.require(&cap).is_ok());
    assert_eq!(fork.capabilities(), seed.capabilities());

    // The fork shares the copied env content.
    let forked_binding = fork.env().get(&binding).expect("env binding copied");
    assert_eq!(
        forked_binding
            .object()
            .downcast_ref::<super::test_support::OpaqueNumber>()
            .unwrap()
            .value,
        7
    );

    // The fork shares the copied registry content.
    assert!(fork.resolve_number_domain(&domain).is_ok());
}
