use crate::{
    error::{Error, Result},
    id::Symbol,
    number_domain::{
        NumberBinaryOp, NumberReductionOp, NumberUnaryOp, NumberValueRef, ValueNumberBinaryOp,
        ValueNumberReductionOp, ValueNumberUnaryOp,
    },
    value::Value,
};

use super::{
    Cx,
    selection::{CandidateSelection, ScoredCandidate, choose_best_candidate},
};

#[derive(Clone)]
struct PreparedBinaryOp<T> {
    op: T,
    left: Value,
    right: Value,
}

#[derive(Clone)]
struct PreparedUnaryOp<T> {
    op: T,
    operand: Value,
}

#[derive(Clone)]
struct PreparedReductionOp<T> {
    op: T,
    operands: Vec<Value>,
}

enum BinaryCandidate {
    Literal(PreparedBinaryOp<NumberBinaryOp>),
    Value(PreparedBinaryOp<ValueNumberBinaryOp>),
}

enum UnaryCandidate {
    Literal(PreparedUnaryOp<NumberUnaryOp>),
    Value(PreparedUnaryOp<ValueNumberUnaryOp>),
}

enum ReductionCandidate {
    Literal(PreparedReductionOp<NumberReductionOp>),
    Value(PreparedReductionOp<ValueNumberReductionOp>),
}

impl Cx {
    /// Parses a number literal by trying registered number domains in
    /// `sorted_number_domains` order.
    ///
    /// That ordering is only a parse-disambiguation rule. Codecs that already
    /// have an explicit source `NumberLiteral` must preserve its domain instead
    /// of relying on this order to recover the same domain from canonical text.
    pub fn parse_number_literal(&mut self, text: &str) -> Result<Option<crate::NumberLiteral>> {
        let domains = self.registry_mut().sorted_number_domains();
        for (_symbol, value) in domains {
            let Some(domain) = value.object().as_number_domain() else {
                continue;
            };
            let Some(parsed) = domain.parse_literal(self, text)? else {
                continue;
            };
            let Some(number) = domain.encode_literal(self, parsed)? else {
                return Err(Error::Eval(format!(
                    "number domain {} parsed {text} but cannot encode it back to a literal",
                    domain.symbol()
                )));
            };
            return Ok(Some(number));
        }

        Ok(None)
    }

    /// Encodes a value to a number literal via the first domain that accepts it.
    pub fn encode_number_value(&mut self, value: Value) -> Result<Option<crate::NumberLiteral>> {
        let domains = self.registry_mut().sorted_number_domains();
        for (_symbol, domain_value) in domains {
            let Some(domain) = domain_value.object().as_number_domain() else {
                continue;
            };
            if let Some(number) = domain.encode_literal(self, value.clone())? {
                return Ok(Some(number));
            }
        }
        Ok(None)
    }

    /// Resolves a value to a [`NumberValueRef`] carrying its domain and literal.
    pub fn number_value_ref(&mut self, value: Value) -> Result<Option<NumberValueRef>> {
        if let Some(number) = value.object().as_number_value() {
            let domain = number.number_domain(self)?;
            let literal = number.number_literal(self)?;
            return Ok(Some(NumberValueRef {
                domain,
                value,
                literal,
            }));
        }

        let Some(literal) = self.encode_number_value(value.clone())? else {
            return Ok(None);
        };
        Ok(Some(NumberValueRef {
            domain: literal.domain.clone(),
            value,
            literal: Some(literal),
        }))
    }

    /// Applies a binary numeric operator to two values, selecting the best
    /// registered rule after promoting operands.
    ///
    /// Errors with [`Error::AmbiguousNumberDispatch`] when no single rule wins.
    pub fn apply_value_number_binary_op(
        &mut self,
        operator: &Symbol,
        left: Value,
        right: Value,
    ) -> Result<Value> {
        let left_ref = self.require_number_value(operator, left.clone(), "left")?;
        let right_ref = self.require_number_value(operator, right.clone(), "right")?;
        let literal_candidates = self
            .registry()
            .number_binary_ops()
            .iter()
            .filter(|op| &op.operator == operator)
            .cloned()
            .collect::<Vec<_>>();
        let value_candidates = self
            .registry()
            .value_number_binary_ops()
            .iter()
            .filter(|op| &op.operator == operator)
            .cloned()
            .collect::<Vec<_>>();
        if literal_candidates.is_empty() && value_candidates.is_empty() {
            return Err(Error::Eval(format!(
                "operator {operator} has no registered number rules"
            )));
        }

        let mut scored = Vec::new();
        for op in literal_candidates {
            let Some((cost, promoted_left, promoted_right)) =
                self.promote_literal_operands(&op, left_ref.clone(), right_ref.clone())?
            else {
                continue;
            };
            scored.push(ScoredCandidate {
                cost,
                candidate: BinaryCandidate::Literal(PreparedBinaryOp {
                    op,
                    left: promoted_left,
                    right: promoted_right,
                }),
            });
        }

        for op in value_candidates {
            let Some((cost, promoted_left, promoted_right)) =
                self.promote_value_operands(&op, left_ref.clone(), right_ref.clone())?
            else {
                continue;
            };
            scored.push(ScoredCandidate {
                cost,
                candidate: BinaryCandidate::Value(PreparedBinaryOp {
                    op,
                    left: promoted_left,
                    right: promoted_right,
                }),
            });
        }

        match choose_best_candidate(scored) {
            CandidateSelection::None => Err(Error::NoPromotionPath {
                operator: operator.clone(),
                left_domain: left_ref.domain,
                right_domain: right_ref.domain,
            }),
            CandidateSelection::Unique(BinaryCandidate::Literal(best)) => {
                let left = self.expect_literal(best.left)?;
                let right = self.expect_literal(best.right)?;
                (best.op.apply)(self, left, right)
            }
            CandidateSelection::Unique(BinaryCandidate::Value(best)) => {
                (best.op.apply)(self, best.left, best.right)
            }
            CandidateSelection::Ambiguous(best) => Err(Error::AmbiguousNumberDispatch {
                operator: operator.clone(),
                candidates: best
                    .into_iter()
                    .map(|candidate| match candidate {
                        BinaryCandidate::Literal(candidate) => {
                            (candidate.op.left_domain, candidate.op.right_domain)
                        }
                        BinaryCandidate::Value(candidate) => {
                            (candidate.op.left_domain, candidate.op.right_domain)
                        }
                    })
                    .collect(),
            }),
        }
    }

    /// Applies a binary numeric operator to two literals.
    ///
    /// Convenience over [`apply_value_number_binary_op`](Cx::apply_value_number_binary_op).
    pub fn apply_number_binary_op(
        &mut self,
        operator: &Symbol,
        left: crate::NumberLiteral,
        right: crate::NumberLiteral,
    ) -> Result<Value> {
        let left_value = self.factory().number_literal(left.domain, left.canonical)?;
        let right_value = self
            .factory()
            .number_literal(right.domain, right.canonical)?;
        self.apply_value_number_binary_op(operator, left_value, right_value)
    }

    /// Applies a unary numeric operator to a value, selecting the best
    /// registered rule after promoting the operand.
    pub fn apply_value_number_unary_op(
        &mut self,
        operator: &Symbol,
        operand: Value,
    ) -> Result<Value> {
        let operand_ref = self.require_number_value(operator, operand.clone(), "operand")?;
        let literal_candidates = self
            .registry()
            .number_unary_ops()
            .iter()
            .filter(|op| &op.operator == operator)
            .cloned()
            .collect::<Vec<_>>();
        let value_candidates = self
            .registry()
            .value_number_unary_ops()
            .iter()
            .filter(|op| &op.operator == operator)
            .cloned()
            .collect::<Vec<_>>();
        if literal_candidates.is_empty() && value_candidates.is_empty() {
            return Err(Error::Eval(format!(
                "operator {operator} has no registered number rules"
            )));
        }

        let mut scored = Vec::new();
        for op in literal_candidates {
            let Some((cost, promoted_operand)) =
                self.promote_literal_operand(operand_ref.clone(), &op.operand_domain)?
            else {
                continue;
            };
            scored.push(ScoredCandidate {
                cost: cost as u32 + op.cost as u32,
                candidate: UnaryCandidate::Literal(PreparedUnaryOp {
                    op,
                    operand: promoted_operand,
                }),
            });
        }

        for op in value_candidates {
            let Some((cost, promoted_operand)) =
                self.promote_number_value(operand_ref.clone(), &op.operand_domain)?
            else {
                continue;
            };
            scored.push(ScoredCandidate {
                cost: cost as u32 + op.cost as u32,
                candidate: UnaryCandidate::Value(PreparedUnaryOp {
                    op,
                    operand: promoted_operand.value,
                }),
            });
        }

        match choose_best_candidate(scored) {
            CandidateSelection::None => Err(Error::Eval(format!(
                "operator {operator} has no registered number rule for {}",
                operand_ref.domain
            ))),
            CandidateSelection::Unique(UnaryCandidate::Literal(best)) => {
                let operand = self.expect_literal(best.operand)?;
                (best.op.apply)(self, operand)
            }
            CandidateSelection::Unique(UnaryCandidate::Value(best)) => {
                (best.op.apply)(self, best.operand)
            }
            CandidateSelection::Ambiguous(best) => Err(Error::AmbiguousNumberDispatch {
                operator: operator.clone(),
                candidates: best
                    .into_iter()
                    .map(|candidate| match candidate {
                        UnaryCandidate::Literal(candidate) => (
                            candidate.op.operand_domain.clone(),
                            candidate.op.operand_domain,
                        ),
                        UnaryCandidate::Value(candidate) => (
                            candidate.op.operand_domain.clone(),
                            candidate.op.operand_domain,
                        ),
                    })
                    .collect(),
            }),
        }
    }

    /// Applies a unary numeric operator to a literal.
    ///
    /// Convenience over [`apply_value_number_unary_op`](Cx::apply_value_number_unary_op).
    pub fn apply_number_unary_op(
        &mut self,
        operator: &Symbol,
        operand: crate::NumberLiteral,
    ) -> Result<Value> {
        let operand = self
            .factory()
            .number_literal(operand.domain, operand.canonical)?;
        self.apply_value_number_unary_op(operator, operand)
    }

    /// Applies a reduction numeric operator across many values, selecting the
    /// best registered rule after promoting all operands to a common domain.
    pub fn apply_value_number_reduction_op(
        &mut self,
        operator: &Symbol,
        operands: Vec<Value>,
    ) -> Result<Value> {
        let operand_refs = operands
            .into_iter()
            .map(|operand| self.require_number_value(operator, operand, "operand"))
            .collect::<Result<Vec<_>>>()?;
        let literal_candidates = self
            .registry()
            .number_reduction_ops()
            .iter()
            .filter(|op| &op.operator == operator)
            .cloned()
            .collect::<Vec<_>>();
        let value_candidates = self
            .registry()
            .value_number_reduction_ops()
            .iter()
            .filter(|op| &op.operator == operator)
            .cloned()
            .collect::<Vec<_>>();
        if literal_candidates.is_empty() && value_candidates.is_empty() {
            return Err(Error::Eval(format!(
                "operator {operator} has no registered number rules"
            )));
        }

        let mut scored = Vec::new();
        for op in literal_candidates {
            let Some((cost, promoted_operands)) = self
                .promote_literal_operands_for_reduction(operand_refs.clone(), &op.operand_domain)?
            else {
                continue;
            };
            scored.push(ScoredCandidate {
                cost: cost as u32 + op.cost as u32,
                candidate: ReductionCandidate::Literal(PreparedReductionOp {
                    op,
                    operands: promoted_operands,
                }),
            });
        }

        for op in value_candidates {
            let Some((cost, promoted_operands)) =
                self.promote_number_values(operand_refs.clone(), &op.operand_domain)?
            else {
                continue;
            };
            scored.push(ScoredCandidate {
                cost: cost as u32 + op.cost as u32,
                candidate: ReductionCandidate::Value(PreparedReductionOp {
                    op,
                    operands: promoted_operands
                        .into_iter()
                        .map(|item| item.value)
                        .collect(),
                }),
            });
        }

        let domains = operand_refs
            .iter()
            .map(|operand| operand.domain.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        match choose_best_candidate(scored) {
            CandidateSelection::None => Err(Error::Eval(format!(
                "operator {operator} has no registered number rule for [{domains}]"
            ))),
            CandidateSelection::Unique(ReductionCandidate::Literal(best)) => {
                let operands = best
                    .operands
                    .into_iter()
                    .map(|operand| self.expect_literal(operand))
                    .collect::<Result<Vec<_>>>()?;
                (best.op.apply)(self, operands)
            }
            CandidateSelection::Unique(ReductionCandidate::Value(best)) => {
                (best.op.apply)(self, best.operands)
            }
            CandidateSelection::Ambiguous(best) => Err(Error::AmbiguousNumberDispatch {
                operator: operator.clone(),
                candidates: best
                    .into_iter()
                    .map(|candidate| match candidate {
                        ReductionCandidate::Literal(candidate) => (
                            candidate.op.operand_domain.clone(),
                            candidate.op.operand_domain,
                        ),
                        ReductionCandidate::Value(candidate) => (
                            candidate.op.operand_domain.clone(),
                            candidate.op.operand_domain,
                        ),
                    })
                    .collect(),
            }),
        }
    }

    /// Applies a reduction numeric operator across many literals.
    ///
    /// Convenience over [`apply_value_number_reduction_op`](Cx::apply_value_number_reduction_op).
    pub fn apply_number_reduction_op(
        &mut self,
        operator: &Symbol,
        operands: Vec<crate::NumberLiteral>,
    ) -> Result<Value> {
        let operands = operands
            .into_iter()
            .map(|operand| {
                self.factory()
                    .number_literal(operand.domain, operand.canonical)
            })
            .collect::<Result<Vec<_>>>()?;
        self.apply_value_number_reduction_op(operator, operands)
    }

    fn require_number_value(
        &mut self,
        operator: &Symbol,
        value: Value,
        side: &str,
    ) -> Result<NumberValueRef> {
        self.number_value_ref(value)?.ok_or_else(|| {
            Error::Eval(format!(
                "operator {operator} {side} operand is not a registered number"
            ))
        })
    }

    fn expect_literal(&mut self, value: Value) -> Result<crate::NumberLiteral> {
        self.number_value_ref(value)?.and_then(|number| number.literal).ok_or_else(|| {
            Error::Eval("value-level numeric dispatch selected a literal-only rule for a non-literal value".to_owned())
        })
    }
}
