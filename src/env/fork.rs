//! Seeded-context forking: [`Cx::fork_from_seed`].

use crate::Cx;

impl Cx {
    /// Forks a fresh, independently-seated context from `self` used as a seed.
    ///
    /// The fork is a brand-new [`Cx`] (with its own [`GrantSeat`] identity, its
    /// own stores and ledgers) that carries a copy of the seed's evaluation
    /// surface: the eval policy and factory it is built from, plus the seed's
    /// environment, behavior registry, source registry, promotion-search limits,
    /// control policy, and macro expander. The seed's granted capabilities are
    /// re-granted into the fork through the fork's own host seat, so the fork
    /// starts with the same authority without sharing the seed's mutable state.
    ///
    /// This generalizes the per-consumer `clone_stream_cx` / `clone_server_cx`
    /// helpers into one kernel primitive; it is a strict superset (it copies the
    /// control policy and macro expander that only some of those helpers did).
    ///
    /// The data substrate (datum/fact/handle stores, effect ledger, load claims,
    /// diagnostics) is intentionally *not* copied: those are per-context runtime
    /// state, and a fork begins with the fresh empty stores of [`Cx::new`].
    ///
    /// [`GrantSeat`]: crate::GrantSeat
    pub fn fork_from_seed(&self) -> Cx {
        let (mut fork, seat) = Cx::new_seated(self.eval_policy_ref(), self.factory_ref());
        *fork.env_mut() = self.env().clone();
        *fork.registry_mut() = self.registry().clone();
        *fork.sources_mut() = self.sources().clone();
        fork.set_promotion_search_limits(self.promotion_search_limits());
        fork.set_control_policy(self.control_policy_ref());
        if let Some(expander) = self.macro_expander_ref() {
            fork.set_macro_expander(expander);
        }
        for capability in self.capabilities().iter() {
            seat.grant(&mut fork, capability.clone());
        }
        fork
    }
}
