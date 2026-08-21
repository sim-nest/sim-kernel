//! Rank metadata: the contract for ordering and navigating ranked spaces.
//!
//! The kernel defines the rank operation keys and the space/coordinate
//! predicate vocabulary; libraries supply the concrete ranking behavior.

use crate::{
    card::{card_help_predicate, card_kind_predicate, card_ops_predicate},
    claim::{Claim, ClaimPattern},
    datum::Datum,
    env::Cx,
    error::Result,
    id::Symbol,
    ref_id::{ContentId, Coordinate, Ref},
    term::OpKey,
};

/// Operation key for mapping a value to its rank in a space.
pub fn rank_rank_op_key() -> OpKey {
    rank_op_key("rank")
}

/// Operation key for mapping a rank back to its value.
pub fn rank_unrank_op_key() -> OpKey {
    rank_op_key("unrank")
}

/// Operation key for listing a coordinate's neighbors.
pub fn rank_neighbors_op_key() -> OpKey {
    rank_op_key("neighbors")
}

/// Operation key for advancing to the next coordinate in order.
pub fn rank_order_next_op_key() -> OpKey {
    rank_op_key("order-next")
}

/// Card kind symbol identifying a rank space.
pub fn rank_space_kind() -> Symbol {
    rank_symbol("space")
}

/// Card kind symbol identifying a rank coordinate.
pub fn rank_coordinate_kind() -> Symbol {
    rank_symbol("coordinate")
}

/// Claim predicate naming a coordinate's space.
pub fn rank_space_predicate() -> Symbol {
    rank_symbol("space")
}

/// Claim predicate naming a coordinate's ordinal.
pub fn rank_ordinal_predicate() -> Symbol {
    rank_symbol("ordinal")
}

/// Build a coordinate reference for `ordinal` within `space`.
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use sim_kernel::{DefaultFactory, NoopEvalPolicy};
/// # use sim_kernel::env::Cx;
/// # use sim_kernel::datum::Datum;
/// # use sim_kernel::datum_store::DatumStore;
/// # use sim_kernel::id::Symbol;
/// # use sim_kernel::rank::rank_coordinate;
/// # use sim_kernel::ref_id::Ref;
/// let mut cx = Cx::new(
///     Arc::new(NoopEvalPolicy),
///     Arc::new(DefaultFactory),
///     sim_kernel::HandleSeed::new(7),
/// );
/// let ordinal = cx
///     .datum_store_mut()
///     .intern(Datum::String("first".to_owned()))
///     .unwrap();
/// let coord = rank_coordinate(Symbol::qualified("rank", "expr-small"), ordinal);
/// assert!(matches!(coord, Ref::Coord(_)));
/// ```
pub fn rank_coordinate(space: Symbol, ordinal: ContentId) -> Ref {
    Ref::Coord(Coordinate { space, ordinal })
}

/// Publish the kind, operation, and optional help claims describing a rank
/// space, inserting each only if not already present.
pub fn publish_rank_space_claims(cx: &mut Cx, space: Symbol, help: Option<&str>) -> Result<()> {
    let subject = Ref::Symbol(space);
    insert_once(
        cx,
        subject.clone(),
        card_kind_predicate(),
        Ref::Symbol(rank_space_kind()),
    )?;
    for op in [
        rank_rank_op_key(),
        rank_unrank_op_key(),
        rank_neighbors_op_key(),
        rank_order_next_op_key(),
    ] {
        insert_once(
            cx,
            subject.clone(),
            card_ops_predicate(),
            Ref::Symbol(op_symbol(&op)),
        )?;
    }
    if let Some(help) = help {
        let help_ref = Claim::intern_object(cx.datum_store_mut(), Datum::String(help.to_owned()))?;
        insert_once(cx, subject, card_help_predicate(), help_ref)?;
    }
    Ok(())
}

/// Publish the kind, space, and ordinal claims describing a rank coordinate,
/// inserting each only if not already present.
pub fn publish_coordinate_claims(cx: &mut Cx, coordinate: Coordinate) -> Result<()> {
    let subject = Ref::Coord(coordinate.clone());
    insert_once(
        cx,
        subject.clone(),
        card_kind_predicate(),
        Ref::Symbol(rank_coordinate_kind()),
    )?;
    insert_once(
        cx,
        subject.clone(),
        rank_space_predicate(),
        Ref::Symbol(coordinate.space),
    )?;
    insert_once(
        cx,
        subject,
        rank_ordinal_predicate(),
        Ref::Content(coordinate.ordinal),
    )
}

fn insert_once(cx: &mut Cx, subject: Ref, predicate: Symbol, object: Ref) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        cx.insert_fact(Claim::public(subject, predicate, object))?;
    }
    Ok(())
}

fn rank_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("rank"), Symbol::new(name), 1)
}

fn op_symbol(op: &OpKey) -> Symbol {
    Symbol::qualified(
        op.namespace.to_string(),
        format!("{}.v{}", op.name, op.version),
    )
}

fn rank_symbol(name: &str) -> Symbol {
    Symbol::qualified("rank", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DefaultFactory, Expr, NoopEvalPolicy, card::card_for_ref, datum_store::DatumStore,
    };
    use std::sync::Arc;

    #[test]
    fn rank_space_and_coordinate_claims_are_publishable_without_accessor() {
        let mut cx = Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let space = Symbol::qualified("rank", "expr-small");
        publish_rank_space_claims(&mut cx, space.clone(), Some("small expression rank")).unwrap();

        let ordinal = cx
            .datum_store_mut()
            .intern(Datum::String("first".to_owned()))
            .unwrap();
        let coordinate = Coordinate {
            space: space.clone(),
            ordinal: ordinal.clone(),
        };
        publish_coordinate_claims(&mut cx, coordinate.clone()).unwrap();

        assert_has_claim(
            &cx,
            Ref::Symbol(space),
            card_kind_predicate(),
            Ref::Symbol(rank_space_kind()),
        );
        assert_has_claim(
            &cx,
            Ref::Coord(coordinate),
            rank_ordinal_predicate(),
            Ref::Content(ordinal),
        );
    }

    #[test]
    fn rank_space_and_coordinate_claims_project_to_cards() {
        let mut cx = Cx::new(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let space = Symbol::qualified("rank", "expr-small");
        publish_rank_space_claims(&mut cx, space.clone(), Some("small expression rank")).unwrap();

        let ordinal = cx
            .datum_store_mut()
            .intern(Datum::String("first".to_owned()))
            .unwrap();
        let coordinate = Coordinate {
            space: space.clone(),
            ordinal,
        };
        publish_coordinate_claims(&mut cx, coordinate.clone()).unwrap();

        let space_card = card_expr(&mut cx, Ref::Symbol(space));
        assert_eq!(
            table_value(&space_card, "kind"),
            Some(&Expr::Symbol(rank_space_kind()))
        );
        assert_eq!(
            table_value(&space_card, "help"),
            Some(&Expr::String("small expression rank".to_owned()))
        );
        assert_list_contains_symbol(
            table_value(&space_card, "ops").expect("rank ops"),
            Symbol::qualified("rank", "rank.v1"),
        );
        assert_list_contains_symbol(
            table_value(&space_card, "ops").expect("rank ops"),
            Symbol::qualified("rank", "unrank.v1"),
        );

        let coordinate_card = card_expr(&mut cx, Ref::Coord(coordinate));
        assert_eq!(
            table_value(&coordinate_card, "kind"),
            Some(&Expr::Symbol(rank_coordinate_kind()))
        );
    }

    fn assert_has_claim(cx: &Cx, subject: Ref, predicate: Symbol, object: Ref) {
        let claims = cx
            .query_facts(ClaimPattern::exact(subject, predicate, object))
            .unwrap();
        assert_eq!(claims.len(), 1);
    }

    fn card_expr(cx: &mut Cx, subject: Ref) -> Expr {
        card_for_ref(cx, subject)
            .unwrap()
            .object()
            .as_expr(cx)
            .unwrap()
    }

    fn table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a Expr> {
        let Expr::Map(entries) = expr else {
            return None;
        };
        entries.iter().find_map(|(entry_key, entry_value)| {
            let Expr::Symbol(entry_key) = entry_key else {
                return None;
            };
            (entry_key == &Symbol::new(key)).then_some(entry_value)
        })
    }

    fn assert_list_contains_symbol(expr: &Expr, expected: Symbol) {
        assert!(matches!(expr, Expr::List(_)), "expected list");
        let Expr::List(items) = expr else {
            return;
        };
        assert!(
            items
                .iter()
                .any(|item| item == &Expr::Symbol(expected.clone())),
            "expected list to contain {expected}"
        );
    }
}
