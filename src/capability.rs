//! Capability names and sets: the contract for gating privileged operations.
//!
//! The kernel defines capability identity, trust levels, and read policy plus
//! the well-known core capability names; libraries decide what each capability
//! authorizes.

use std::{collections::BTreeSet, fmt, sync::Arc};

use crate::{
    error::{Error, Result},
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

    /// Iterates the granted capabilities in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityName> {
        self.granted.iter()
    }
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

/// The capability gating browse reads (`browse.read`).
pub fn browse_read_capability() -> CapabilityName {
    CapabilityName::new("browse.read")
}

/// The capability gating browse-driven test runs (`browse.run-tests`).
pub fn browse_run_tests_capability() -> CapabilityName {
    CapabilityName::new("browse.run-tests")
}

/// The capability gating internal browse surfaces (`browse.internal`).
pub fn browse_internal_capability() -> CapabilityName {
    CapabilityName::new("browse.internal")
}

/// The capability gating logic-database writes (`logic.db.write`).
pub fn logic_db_write_capability() -> CapabilityName {
    CapabilityName::new("logic.db.write")
}

/// The capability gating logic file consulting (`logic.consult.file`).
pub fn logic_consult_file_capability() -> CapabilityName {
    CapabilityName::new("logic.consult.file")
}

/// The capability gating logic tool calls (`logic.tool-call`).
pub fn logic_tool_call_capability() -> CapabilityName {
    CapabilityName::new("logic.tool-call")
}

/// The capability gating the configured list implementation (`config.list.impl`).
pub fn config_list_impl_capability() -> CapabilityName {
    CapabilityName::new("config.list.impl")
}

/// The capability gating the configured table implementation (`config.table.impl`).
pub fn config_table_impl_capability() -> CapabilityName {
    CapabilityName::new("config.table.impl")
}

/// The capability gating unbounded list forcing (`list.force.unbounded`).
pub fn list_force_unbounded_capability() -> CapabilityName {
    CapabilityName::new("list.force.unbounded")
}

/// The capability gating filesystem-backed tables (`table.fs`).
pub fn table_fs_capability() -> CapabilityName {
    CapabilityName::new("table.fs")
}

/// The capability gating filesystem table reads (`table.fs.read`).
pub fn table_fs_read_capability() -> CapabilityName {
    CapabilityName::new("table.fs.read")
}

/// The capability gating filesystem table writes (`table.fs.write`).
pub fn table_fs_write_capability() -> CapabilityName {
    CapabilityName::new("table.fs.write")
}

/// The capability gating filesystem table directory creation (`table.fs.mkdir`).
pub fn table_fs_mkdir_capability() -> CapabilityName {
    CapabilityName::new("table.fs.mkdir")
}

/// The capability gating filesystem table directory removal (`table.fs.rmdir`).
pub fn table_fs_rmdir_capability() -> CapabilityName {
    CapabilityName::new("table.fs.rmdir")
}

/// The capability gating database-backed tables (`table.db`).
pub fn table_db_capability() -> CapabilityName {
    CapabilityName::new("table.db")
}

/// The capability gating database table reads (`table.db.read`).
pub fn table_db_read_capability() -> CapabilityName {
    CapabilityName::new("table.db.read")
}

/// The capability gating database table writes (`table.db.write`).
pub fn table_db_write_capability() -> CapabilityName {
    CapabilityName::new("table.db.write")
}

/// The capability gating database table directory creation (`table.db.mkdir`).
pub fn table_db_mkdir_capability() -> CapabilityName {
    CapabilityName::new("table.db.mkdir")
}

/// The capability gating database table directory removal (`table.db.rmdir`).
pub fn table_db_rmdir_capability() -> CapabilityName {
    CapabilityName::new("table.db.rmdir")
}

/// The capability gating remote tables (`table.remote`).
pub fn table_remote_capability() -> CapabilityName {
    CapabilityName::new("table.remote")
}

/// The capability gating registry catalog reads (`registry.catalog.read`).
pub fn registry_catalog_read_capability() -> CapabilityName {
    CapabilityName::new("registry.catalog.read")
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilitySet, ReadPolicy, TrustLevel, read_construct_capability, read_eval_capability,
    };
    use crate::Error;

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
}
