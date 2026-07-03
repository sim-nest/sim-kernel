use std::collections::BTreeMap;

use crate::{
    error::{Error, Result},
    id::Symbol,
    number_domain::{
        NumberBinaryOp, NumberValueRef, PromotionRule, ValueNumberBinaryOp, ValuePromotionRule,
    },
    value::Value,
};

use super::Cx;

#[derive(Clone)]
enum PromotionStep {
    Literal(PromotionRule),
    Value(ValuePromotionRule),
}

#[derive(Clone)]
struct SearchState {
    number: NumberValueRef,
    cost: u16,
    path: Vec<Symbol>,
}

impl Cx {
    pub(super) fn promote_value_operands(
        &mut self,
        op: &ValueNumberBinaryOp,
        left: NumberValueRef,
        right: NumberValueRef,
    ) -> Result<Option<(u32, Value, Value)>> {
        let Some((left_cost, left)) = self.promote_number_value(left, &op.left_domain)? else {
            return Ok(None);
        };
        let Some((right_cost, right)) = self.promote_number_value(right, &op.right_domain)? else {
            return Ok(None);
        };
        Ok(Some((
            op.cost as u32 + left_cost as u32 + right_cost as u32,
            left.value,
            right.value,
        )))
    }

    pub(super) fn promote_literal_operands(
        &mut self,
        op: &NumberBinaryOp,
        left: NumberValueRef,
        right: NumberValueRef,
    ) -> Result<Option<(u32, Value, Value)>> {
        let Some((left_cost, left)) = self.promote_literal_operand(left, &op.left_domain)? else {
            return Ok(None);
        };
        let Some((right_cost, right)) = self.promote_literal_operand(right, &op.right_domain)?
        else {
            return Ok(None);
        };
        Ok(Some((
            op.cost as u32 + left_cost as u32 + right_cost as u32,
            left,
            right,
        )))
    }

    pub(super) fn promote_number_values(
        &mut self,
        operands: Vec<NumberValueRef>,
        target_domain: &Symbol,
    ) -> Result<Option<(u16, Vec<NumberValueRef>)>> {
        let mut total_cost = 0_u16;
        let mut promoted = Vec::with_capacity(operands.len());
        for operand in operands {
            let Some((cost, operand)) = self.promote_number_value(operand, target_domain)? else {
                return Ok(None);
            };
            total_cost = total_cost
                .checked_add(cost)
                .ok_or_else(|| Error::Eval("numeric promotion cost overflowed".to_owned()))?;
            promoted.push(operand);
        }
        Ok(Some((total_cost, promoted)))
    }

    pub(super) fn promote_literal_operands_for_reduction(
        &mut self,
        operands: Vec<NumberValueRef>,
        target_domain: &Symbol,
    ) -> Result<Option<(u16, Vec<Value>)>> {
        let mut total_cost = 0_u16;
        let mut promoted = Vec::with_capacity(operands.len());
        for operand in operands {
            let Some((cost, operand)) = self.promote_literal_operand(operand, target_domain)?
            else {
                return Ok(None);
            };
            total_cost = total_cost
                .checked_add(cost)
                .ok_or_else(|| Error::Eval("numeric promotion cost overflowed".to_owned()))?;
            promoted.push(operand);
        }
        Ok(Some((total_cost, promoted)))
    }

    pub(super) fn promote_literal_operand(
        &mut self,
        number: NumberValueRef,
        target_domain: &Symbol,
    ) -> Result<Option<(u16, Value)>> {
        let Some((cost, promoted)) = self.promote_number_value(number, target_domain)? else {
            return Ok(None);
        };
        if promoted.literal.is_some() {
            Ok(Some((cost, promoted.value)))
        } else {
            Ok(None)
        }
    }

    pub(super) fn promote_number_value(
        &mut self,
        number: NumberValueRef,
        target_domain: &Symbol,
    ) -> Result<Option<(u16, NumberValueRef)>> {
        if &number.domain == target_domain {
            return Ok(Some((0, number)));
        }
        let Some((cost, promoted)) = self.find_value_promotion(number, target_domain)? else {
            return Ok(None);
        };
        Ok(Some((cost, promoted)))
    }

    fn find_value_promotion(
        &mut self,
        from_value: NumberValueRef,
        target_domain: &Symbol,
    ) -> Result<Option<(u16, NumberValueRef)>> {
        let from_domain = from_value.domain.clone();
        let limits = self.promotion_search_limits();
        let mut best: Option<(u16, NumberValueRef)> = None;
        let mut queue = vec![SearchState {
            number: from_value,
            cost: 0,
            path: vec![from_domain.clone()],
        }];
        let mut best_seen = BTreeMap::from([(from_domain.clone(), 0_u16)]);
        let mut visited_states = 0_usize;

        while let Some(state) = queue.pop() {
            visited_states += 1;
            if visited_states > limits.max_states {
                return Err(Error::PromotionSearchLimitExceeded {
                    from_domain: from_domain.clone(),
                    target_domain: target_domain.clone(),
                    max_depth: limits.max_depth,
                    max_states: limits.max_states,
                });
            }
            if &state.number.domain == target_domain {
                match &best {
                    None => best = Some((state.cost, state.number.clone())),
                    Some((best_cost, _)) if state.cost < *best_cost => {
                        best = Some((state.cost, state.number.clone()))
                    }
                    _ => {}
                }
                continue;
            }
            if state.path.len().saturating_sub(1) >= limits.max_depth {
                continue;
            }

            for step in self.promotion_steps_for(&state.number) {
                let next_domain = match &step {
                    PromotionStep::Literal(rule) => rule.to_domain.clone(),
                    PromotionStep::Value(rule) => rule.to_domain.clone(),
                };
                if state.path.contains(&next_domain) {
                    continue;
                }
                let next_cost = state.cost.saturating_add(match &step {
                    PromotionStep::Literal(rule) => rule.cost,
                    PromotionStep::Value(rule) => rule.cost,
                });
                if best
                    .as_ref()
                    .is_some_and(|(best_cost, _)| next_cost >= *best_cost)
                {
                    continue;
                }
                if best_seen
                    .get(&next_domain)
                    .is_some_and(|seen_cost| next_cost >= *seen_cost)
                {
                    continue;
                }
                let Some(next_value) = self.apply_promotion_step(&step, state.number.clone())?
                else {
                    continue;
                };
                best_seen.insert(next_domain.clone(), next_cost);
                let mut next_path = state.path.clone();
                next_path.push(next_domain);
                queue.push(SearchState {
                    number: next_value,
                    cost: next_cost,
                    path: next_path,
                });
            }
        }

        Ok(best)
    }

    fn promotion_steps_for(&self, number: &NumberValueRef) -> Vec<PromotionStep> {
        let mut steps = self
            .registry()
            .value_promotion_rules()
            .iter()
            .filter(|rule| rule.from_domain == number.domain)
            .cloned()
            .map(PromotionStep::Value)
            .collect::<Vec<_>>();
        if number.literal.is_some() {
            steps.extend(
                self.registry()
                    .promotion_rules()
                    .iter()
                    .filter(|rule| rule.from_domain == number.domain)
                    .cloned()
                    .map(PromotionStep::Literal),
            );
        }
        steps
    }

    fn apply_promotion_step(
        &mut self,
        step: &PromotionStep,
        number: NumberValueRef,
    ) -> Result<Option<NumberValueRef>> {
        match step {
            PromotionStep::Literal(rule) => {
                let literal = number.literal.ok_or_else(|| {
                    Error::Eval(format!(
                        "literal promotion from {} requires a literal-backed number value",
                        rule.from_domain
                    ))
                })?;
                let promoted = (rule.convert)(self, literal)?;
                let domain = promoted.domain.clone();
                let value = self
                    .factory()
                    .number_literal(domain.clone(), promoted.canonical.clone())?;
                Ok(Some(NumberValueRef {
                    domain,
                    value,
                    literal: Some(promoted),
                }))
            }
            PromotionStep::Value(rule) => {
                let value = (rule.convert)(self, number.value)?;
                self.number_value_ref(value)
            }
        }
    }
}
