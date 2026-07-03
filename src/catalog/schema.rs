use crate::Symbol;

/// Write policy governing how rows of a catalog table may change.
///
/// See the README section "Contract: registry catalog substrate" for how the
/// registry's tables map onto these policies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogWritePolicy {
    /// Insert, replace, and delete are all permitted.
    Mutable,
    /// A key may be inserted once and then never changed or deleted.
    Sealed,
    /// Keys may be inserted but never changed or deleted.
    AppendOnly,
    /// Direct writes fail; rows are derived from other tables.
    Derived,
}

/// Specification of one catalog table: its name, write policy, owner, and field
/// constraints.
///
/// # Examples
///
/// ```
/// # use sim_kernel::catalog::{CatalogTableSpec, CatalogWritePolicy};
/// # use sim_kernel::Symbol;
/// let spec = CatalogTableSpec::new(Symbol::new("registry/libs"), CatalogWritePolicy::Mutable)
///     .with_required_fields(vec![Symbol::new("manifest")]);
/// assert_eq!(spec.policy, CatalogWritePolicy::Mutable);
/// assert_eq!(spec.required_fields, vec![Symbol::new("manifest")]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogTableSpec {
    /// Open symbol naming the table.
    pub name: Symbol,
    /// Write policy enforced on the table's rows.
    pub policy: CatalogWritePolicy,
    /// Optional owning library symbol.
    pub owner: Option<Symbol>,
    /// Fields every row of the table must carry.
    pub required_fields: Vec<Symbol>,
    /// Field tuples that must be unique across the table's rows.
    pub unique_fields: Vec<Vec<Symbol>>,
}

impl CatalogTableSpec {
    /// Creates a spec with the given name and policy and no further constraints.
    pub fn new(name: Symbol, policy: CatalogWritePolicy) -> Self {
        Self {
            name,
            policy,
            owner: None,
            required_fields: Vec::new(),
            unique_fields: Vec::new(),
        }
    }

    /// Returns the spec with its required fields replaced.
    pub fn with_required_fields(mut self, fields: Vec<Symbol>) -> Self {
        self.required_fields = fields;
        self
    }
}
