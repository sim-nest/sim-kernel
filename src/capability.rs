//! Capability names and sets: the contract for gating privileged operations.
//!
//! The kernel defines capability identity, trust levels, read policy, and
//! cross-cutting kernel capability names. Libraries mint behavior-specific
//! capability names with [`CapabilityName::new`] in their own crates.

use std::{collections::BTreeSet, fmt, sync::Arc};

use crate::{
    error::{Error, Result},
    eval::Phase,
    id::Symbol,
};

/// The identity of a capability: the token an operation requires to run.
///
/// Capability names are the kernel's contract for gating privileged behavior;
/// libraries decide what each name authorizes. See the README section
/// "Capabilities and trust".
///
/// # Examples
///
/// ```
/// # use sim_kernel::CapabilityName;
/// let cap = CapabilityName::new("table.fs.read");
/// assert_eq!(cap.as_str(), "table.fs.read");
/// assert_eq!(cap.as_symbol().to_string(), "capability/table.fs.read");
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CapabilityName(Arc<str>);

impl CapabilityName {
    /// Interns a capability name from a string.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self(name.into())
    }

    /// Returns the capability name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the name as a `capability`-qualified [`Symbol`].
    pub fn as_symbol(&self) -> Symbol {
        Symbol::qualified("capability", self.as_str().to_owned())
    }
}

impl fmt::Display for CapabilityName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A host-only capability-grant seat: the authority to grant a capability into a
/// [`Cx`].
///
/// A `GrantSeat` is minted ONLY when a context is constructed
/// ([`Cx::new_seated`]) or, under the `test-support` feature, by
/// `GrantSeat::for_test`. It has no public constructor otherwise, so it cannot
/// be forged. Crucially, it is never passed into [`Callable::call`] -- a loaded
/// callable receives a `&mut Cx` but no seat, so it cannot grant itself a
/// capability. Host code (a bootloader, a server session, a loader) holds the
/// seat and grants through it.
///
/// [`Cx`]: crate::Cx
/// [`Cx::new_seated`]: crate::Cx::new_seated
/// [`Callable::call`]: crate::Callable::call
#[derive(Debug)]
pub struct GrantSeat {
    /// The [`Cx`] identity this seat may grant into. `0` is a test-only wildcard
    /// that matches any context (see [`GrantSeat::for_test`]).
    ///
    /// [`Cx`]: crate::Cx
    cx_id: u64,
}

impl GrantSeat {
    pub(crate) fn for_cx(cx_id: u64) -> Self {
        Self { cx_id }
    }

    /// Mints a wildcard seat for tests and fixtures (grants into any context).
    /// Only available under the `test-support` feature; never compiled into a
    /// production build.
    #[cfg(feature = "test-support")]
    pub fn for_test() -> Self {
        Self { cx_id: 0 }
    }

    /// Grants a capability into `cx` under this host authority.
    ///
    /// A seat may only grant into the context it was minted with
    /// ([`Cx::new_seated`]). Minting a fresh `(Cx, GrantSeat)` pair yields a seat
    /// bound to the fresh context, so it cannot be used to escalate a different
    /// (e.g. the caller's own) context -- this is what keeps a loaded callable
    /// from granting itself a capability.
    ///
    /// [`Cx::new_seated`]: crate::Cx::new_seated
    pub fn grant(&self, cx: &mut crate::Cx, capability: CapabilityName) -> Result<()> {
        if self.cx_id != 0 && self.cx_id != cx.grant_id() {
            return Err(Error::ForeignGrantSeat);
        }
        cx.grant_from_host(capability);
        Ok(())
    }

    /// Grants a capability named by a static string into `cx`.
    pub fn grant_named(&self, cx: &mut crate::Cx, capability: &'static str) -> Result<()> {
        self.grant(cx, CapabilityName::new(capability))
    }
}

/// A set of granted capabilities, used as the capability state of a [`Cx`].
///
/// [`Cx`]: crate::Cx
///
/// # Examples
///
/// ```
/// # use sim_kernel::{CapabilityName, capability::CapabilitySet};
/// let cap = CapabilityName::new("read-construct");
/// let granted = CapabilitySet::new().grant(cap.clone());
/// assert!(granted.contains(&cap));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    granted: BTreeSet<CapabilityName>,
}

impl CapabilitySet {
    /// Builds an empty capability set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the set with one more capability granted (builder style).
    pub fn grant(mut self, capability: CapabilityName) -> Self {
        self.granted.insert(capability);
        self
    }

    /// Grants a capability in place.
    pub fn insert(&mut self, capability: CapabilityName) {
        self.granted.insert(capability);
    }

    /// Reports whether the capability is granted.
    pub fn contains(&self, capability: &CapabilityName) -> bool {
        self.granted.contains(capability)
    }

    /// Returns the capabilities present in both sets.
    ///
    /// Monotone: the result is a subset of each input, so using it as an active
    /// capability set can only narrow authority, never widen it.
    pub fn intersect(&self, other: &CapabilitySet) -> CapabilitySet {
        self.iter()
            .filter(|capability| other.contains(capability))
            .cloned()
            .fold(CapabilitySet::new(), CapabilitySet::grant)
    }

    /// Iterates the granted capabilities in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityName> {
        self.granted.iter()
    }
}

/// Computes the diminished capability set for an explicitly narrowed run.
///
/// The result contains exactly the capabilities the caller already holds and
/// the request explicitly allows. It is therefore a subset of both inputs and
/// never grants a capability absent from `current`.
pub fn diminish(current: &CapabilitySet, allowed: &CapabilitySet) -> CapabilitySet {
    current.intersect(allowed)
}

/// The trust level of a source, gating capabilities beyond mere possession.
///
/// Even a granted capability may be denied when trust is insufficient; see
/// [`ReadPolicy::require`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrustLevel {
    /// Untrusted input; eval-class capabilities are denied even if granted.
    #[default]
    Untrusted,
    /// A source trusted to request evaluation.
    TrustedSource,
    /// The host itself, the most trusted level.
    HostInternal,
}

/// The trust level and capability set governing a read.
///
/// Pairs a [`TrustLevel`] with a [`CapabilitySet`] so the kernel can both
/// check possession and apply trust-sensitive policy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadPolicy {
    /// The trust level of the source being read.
    pub trust: TrustLevel,
    /// The capabilities granted to the read.
    pub capabilities: CapabilitySet,
}

impl ReadPolicy {
    /// Demands a capability, applying trust policy on top of possession.
    ///
    /// Returns [`Error::CapabilityDenied`] when the capability is not granted,
    /// and [`Error::TrustDenied`] when an [`Untrusted`](TrustLevel::Untrusted)
    /// source requests the read-eval capability.
    pub fn require(&self, capability: &CapabilityName) -> Result<()> {
        if !self.capabilities.contains(capability) {
            return Err(Error::CapabilityDenied {
                capability: capability.clone(),
            });
        }

        if self.trust == TrustLevel::Untrusted && capability == &read_eval_capability() {
            return Err(Error::TrustDenied {
                capability: capability.clone(),
                trust: self.trust,
            });
        }

        Ok(())
    }
}

/// The capability gating read-time construction (`read-construct`).
pub fn read_construct_capability() -> CapabilityName {
    CapabilityName::new("read-construct")
}

/// The capability gating read-time evaluation (`read-eval`).
///
/// Trust-sensitive: denied for [`Untrusted`](TrustLevel::Untrusted) sources.
pub fn read_eval_capability() -> CapabilityName {
    CapabilityName::new("read-eval")
}

/// The capability gating native dynamic library loading (`loader.native`).
pub fn native_dynamic_load_capability() -> CapabilityName {
    CapabilityName::new("loader.native")
}

/// The capability gating macro expansion (`macro.expand`).
pub fn macro_expand_capability() -> CapabilityName {
    CapabilityName::new("macro.expand")
}

/// The phase-specific capability required to run macro expansion.
pub fn macro_expansion_capability_for_phase(phase: Phase) -> CapabilityName {
    match phase {
        Phase::Read => macro_expand_read_capability(),
        Phase::Expand => macro_expand_capability(),
        Phase::Compile => macro_expand_compile_capability(),
        Phase::Eval => macro_expand_eval_capability(),
    }
}

/// The capability gating compile-phase macro expansion (`macro.expand.compile`).
pub fn macro_expand_compile_capability() -> CapabilityName {
    CapabilityName::new("macro.expand.compile")
}

/// The capability gating eval-phase macro expansion (`macro.expand.eval`).
pub fn macro_expand_eval_capability() -> CapabilityName {
    CapabilityName::new("macro.expand.eval")
}

/// The capability gating read-phase macro expansion (`macro.expand.read`).
pub fn macro_expand_read_capability() -> CapabilityName {
    CapabilityName::new("macro.expand.read")
}

/// The capability gating use of the eval fabric (`eval.fabric`).
pub fn eval_fabric_capability() -> CapabilityName {
    CapabilityName::new("eval.fabric")
}

/// The capability gating remote evaluation (`eval.remote`).
pub fn eval_remote_capability() -> CapabilityName {
    CapabilityName::new("eval.remote")
}

/// The capability gating control prompts (`control.prompt`).
pub fn control_prompt_capability() -> CapabilityName {
    CapabilityName::new("control.prompt")
}

/// The capability gating control-stack capture (`control.capture`).
pub fn control_capture_capability() -> CapabilityName {
    CapabilityName::new("control.capture")
}

/// The capability gating continuation resumption (`control.resume`).
pub fn control_resume_capability() -> CapabilityName {
    CapabilityName::new("control.resume")
}

/// The capability gating multi-shot continuations (`control.multishot`).
pub fn control_multishot_capability() -> CapabilityName {
    CapabilityName::new("control.multishot")
}

/// The capability gating access to private facts (`kernel.fact.private`).
pub fn fact_private_capability() -> CapabilityName {
    CapabilityName::new("kernel.fact.private")
}

/// The capability gating unbounded list forcing (`list.force.unbounded`).
pub fn list_force_unbounded_capability() -> CapabilityName {
    CapabilityName::new("list.force.unbounded")
}

/// The capability gating registry catalog reads (`registry.catalog.read`).
pub fn registry_catalog_read_capability() -> CapabilityName {
    CapabilityName::new("registry.catalog.read")
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilityName, CapabilitySet, ReadPolicy, TrustLevel, diminish, read_construct_capability,
        read_eval_capability,
    };
    use crate::Error;

    fn named_set(names: &[&str]) -> CapabilitySet {
        names
            .iter()
            .copied()
            .map(CapabilityName::new)
            .fold(CapabilitySet::new(), CapabilitySet::grant)
    }

    fn policy(trust: TrustLevel, capabilities: &[crate::CapabilityName]) -> ReadPolicy {
        ReadPolicy {
            trust,
            capabilities: capabilities
                .iter()
                .cloned()
                .fold(CapabilitySet::new(), |set, capability| {
                    set.grant(capability)
                }),
        }
    }

    fn mask_set(universe: &[&str], mask: usize) -> CapabilitySet {
        universe
            .iter()
            .enumerate()
            .filter_map(|(index, name)| ((mask & (1 << index)) != 0).then_some(*name))
            .map(CapabilityName::new)
            .fold(CapabilitySet::new(), CapabilitySet::grant)
    }

    fn assert_subset(subset: &CapabilitySet, superset: &CapabilitySet) {
        for capability in subset.iter() {
            assert!(
                superset.contains(capability),
                "{capability} must be present in the superset"
            );
        }
    }

    #[test]
    fn diminish_is_subset_of_current_and_allowed_for_small_sets() {
        let universe = ["read-construct", "read-eval", "eval.fabric", "eval.remote"];
        for current_mask in 0..(1 << universe.len()) {
            for allowed_mask in 0..(1 << universe.len()) {
                let current = mask_set(&universe, current_mask);
                let allowed = mask_set(&universe, allowed_mask);
                let diminished = diminish(&current, &allowed);
                assert_subset(&diminished, &current);
                assert_subset(&diminished, &allowed);

                for name in universe {
                    let capability = CapabilityName::new(name);
                    assert_eq!(
                        diminished.contains(&capability),
                        current.contains(&capability) && allowed.contains(&capability),
                        "{name} membership should be exact set intersection"
                    );
                }
            }
        }
    }

    #[test]
    fn diminish_never_adds_a_capability_from_allowed_only() {
        let current = named_set(&["read-construct"]);
        let allowed = named_set(&["read-construct", "read-eval"]);
        let diminished = diminish(&current, &allowed);
        assert!(diminished.contains(&read_construct_capability()));
        assert!(!diminished.contains(&read_eval_capability()));
    }

    #[test]
    fn untrusted_policy_denies_read_eval_even_when_capability_is_present() {
        let err = policy(TrustLevel::Untrusted, &[read_eval_capability()])
            .require(&read_eval_capability())
            .unwrap_err();
        assert!(matches!(
            err,
            Error::TrustDenied { capability, trust }
                if capability == read_eval_capability() && trust == TrustLevel::Untrusted
        ));
    }

    #[test]
    fn trusted_policy_allows_read_eval_when_capability_is_present() {
        policy(TrustLevel::TrustedSource, &[read_eval_capability()])
            .require(&read_eval_capability())
            .unwrap();
    }

    #[test]
    fn non_eval_capabilities_remain_capability_gated() {
        policy(TrustLevel::Untrusted, &[read_construct_capability()])
            .require(&read_construct_capability())
            .unwrap();
    }

    #[test]
    fn grant_seat_grants_a_capability_into_a_cx() {
        use std::sync::Arc;

        use crate::{Cx, DefaultFactory, NoopEvalPolicy};

        let cap = read_eval_capability();
        let (mut cx, seat) = Cx::new_seated(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        // A freshly seated context grants nothing until the host uses the seat.
        assert!(cx.require(&cap).is_err());
        seat.grant(&mut cx, cap.clone()).unwrap();
        assert!(cx.require(&cap).is_ok());
    }

    #[test]
    fn a_seat_cannot_grant_into_a_foreign_cx() {
        use std::sync::Arc;

        use crate::{Cx, DefaultFactory, NoopEvalPolicy};

        // The escalation a loaded callable would attempt: it holds `victim`
        // (its own &mut Cx) and mints a fresh throwaway pair, then tries to use
        // the throwaway seat to grant into `victim`.
        let (mut victim, _victim_seat) = Cx::new_seated(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(7),
        );
        let (_throwaway, forged) = Cx::new_seated(
            Arc::new(NoopEvalPolicy),
            Arc::new(DefaultFactory),
            crate::HandleSeed::new(8),
        );
        assert!(matches!(
            forged.grant(&mut victim, read_eval_capability()),
            Err(Error::ForeignGrantSeat)
        ));
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn grant_seat_for_test_is_available_under_the_feature() {
        // Compiles and runs only when `test-support` is enabled.
        let _seat = super::GrantSeat::for_test();
    }
}
