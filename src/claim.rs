//! Claims: the contract for asserted facts about runtime subjects.
//!
//! The kernel defines the [`Claim`] record, its kinds, visibility, and the
//! [`ClaimPattern`] match contract; libraries supply the claims and the stores
//! that hold them.

use crate::{
    capability::CapabilityName,
    datum::Datum,
    datum_store::DatumStore,
    error::Result,
    id::Symbol,
    ref_id::{ContentId, Coordinate, HandleId, Ref},
};

/// An asserted fact about a runtime subject: a `subject`/`predicate`/`object`
/// triple plus kind, evidence, required capabilities, and visibility.
///
/// The kernel defines the record and its canonical datum form; libraries supply
/// the claims and the [`FactStore`](crate::fact_store::FactStore)s that hold
/// them.
///
/// # Examples
///
/// ```
/// # use sim_kernel::{Claim, ClaimKind, Ref, Symbol};
/// let claim = Claim::new(
///     Ref::Symbol(Symbol::qualified("core", "List")),
///     Symbol::new("kind"),
///     Ref::Symbol(Symbol::qualified("core", "class")),
/// );
/// assert_eq!(claim.kind, ClaimKind::Asserted);
/// // The claim has a deterministic content datum.
/// let _ = claim.canonical_datum();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    /// Content id of the claim once interned; `None` before insertion.
    pub id: Option<ContentId>,
    /// The subject the claim is about.
    pub subject: Ref,
    /// The predicate relating subject to object.
    pub predicate: Symbol,
    /// The object of the claim.
    pub object: Ref,
    /// How the claim was established.
    pub kind: ClaimKind,
    /// Supporting references for the claim.
    pub evidence: Vec<Ref>,
    /// Capabilities required to see the claim under gated or private visibility.
    pub requires: Vec<CapabilityName>,
    /// Read visibility policy for the claim.
    pub visibility: Visibility,
}

/// How a [`Claim`] was established.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClaimKind {
    /// Stated directly by a caller.
    Asserted,
    /// Inferred from other claims.
    Derived,
    /// Recorded from observation.
    Observed,
    /// Withdrawn; hidden from queries unless revoked claims are requested.
    Revoked,
}

/// Read visibility policy for a [`Claim`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Visibility {
    /// Readable by anyone.
    Public,
    /// Readable only when the caller holds the claim's required capabilities.
    CapabilityGated,
    /// Readable only with the fact-private capability and required capabilities.
    Private,
}

/// Query pattern matched against stored [`Claim`]s; `None` fields are wildcards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaimPattern {
    /// Subject to match, or any subject when `None`.
    pub subject: Option<Ref>,
    /// Predicate to match, or any predicate when `None`.
    pub predicate: Option<Symbol>,
    /// Object to match, or any object when `None`.
    pub object: Option<Ref>,
    /// Whether revoked claims are included in results.
    pub include_revoked: bool,
}

impl Claim {
    /// Creates an asserted, public claim with no evidence or requirements.
    pub fn new(subject: Ref, predicate: Symbol, object: Ref) -> Self {
        Self {
            id: None,
            subject,
            predicate,
            object,
            kind: ClaimKind::Asserted,
            evidence: Vec::new(),
            requires: Vec::new(),
            visibility: Visibility::Public,
        }
    }

    /// Creates a public claim; alias for [`Claim::new`].
    pub fn public(subject: Ref, predicate: Symbol, object: Ref) -> Self {
        Self::new(subject, predicate, object)
    }

    /// Builds a claim whose object is a [`Datum`] interned into `store` and
    /// referenced by content.
    pub fn content_object(
        store: &mut dyn DatumStore,
        subject: Ref,
        predicate: Symbol,
        object: Datum,
    ) -> Result<Self> {
        Ok(Self::new(
            subject,
            predicate,
            Self::intern_object(store, object)?,
        ))
    }

    /// Interns `object` into `store` and returns a content [`Ref`] to it.
    pub fn intern_object(store: &mut dyn DatumStore, object: Datum) -> Result<Ref> {
        store.intern(object).map(Ref::Content)
    }

    /// Returns the claim with its [`ClaimKind`] set.
    pub fn with_kind(mut self, kind: ClaimKind) -> Self {
        self.kind = kind;
        self
    }

    /// Returns the claim with its [`Visibility`] set.
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Returns the claim with its evidence references replaced.
    pub fn with_evidence(mut self, evidence: Vec<Ref>) -> Self {
        self.evidence = evidence;
        self
    }

    /// Returns the claim with one required capability appended.
    pub fn requiring(mut self, capability: CapabilityName) -> Self {
        self.requires.push(capability);
        self
    }

    /// Returns the claim with its required capabilities replaced.
    pub fn with_requirements(mut self, requires: Vec<CapabilityName>) -> Self {
        self.requires = requires;
        self
    }

    /// Returns the deterministic [`Datum`] form used to derive the content id.
    pub fn canonical_datum(&self) -> Datum {
        Datum::Node {
            tag: core_symbol("claim"),
            fields: vec![
                (Symbol::new("subject"), ref_datum(&self.subject)),
                (
                    Symbol::new("predicate"),
                    Datum::Symbol(self.predicate.clone()),
                ),
                (Symbol::new("object"), ref_datum(&self.object)),
                (Symbol::new("kind"), Datum::Symbol(self.kind.as_symbol())),
                (
                    Symbol::new("evidence"),
                    Datum::Vector(self.evidence.iter().map(ref_datum).collect()),
                ),
                (
                    Symbol::new("requires"),
                    Datum::Set(requirement_data(&self.requires)),
                ),
                (
                    Symbol::new("visibility"),
                    Datum::Symbol(self.visibility.as_symbol()),
                ),
            ],
        }
    }

    /// Interns the claim's canonical datum and returns its content id.
    pub fn content_id(&self, store: &mut dyn DatumStore) -> Result<ContentId> {
        store.intern(self.canonical_datum())
    }
}

impl ClaimKind {
    /// Returns the `core/*` symbol naming this kind.
    pub fn as_symbol(self) -> Symbol {
        match self {
            Self::Asserted => core_symbol("asserted"),
            Self::Derived => core_symbol("derived"),
            Self::Observed => core_symbol("observed"),
            Self::Revoked => core_symbol("revoked"),
        }
    }
}

impl Visibility {
    /// Returns the `core/*` symbol naming this visibility.
    pub fn as_symbol(self) -> Symbol {
        match self {
            Self::Public => core_symbol("public"),
            Self::CapabilityGated => core_symbol("capability-gated"),
            Self::Private => core_symbol("private"),
        }
    }
}

impl ClaimPattern {
    /// Returns a pattern that matches every claim (all fields wildcard).
    pub fn any() -> Self {
        Self::default()
    }

    /// Returns a pattern fixing subject, predicate, and object exactly.
    pub fn exact(subject: Ref, predicate: Symbol, object: Ref) -> Self {
        Self {
            subject: Some(subject),
            predicate: Some(predicate),
            object: Some(object),
            include_revoked: false,
        }
    }

    /// Returns the pattern with revoked claims included in results.
    pub fn include_revoked(mut self) -> Self {
        self.include_revoked = true;
        self
    }
}

fn requirement_data(requires: &[CapabilityName]) -> Vec<Datum> {
    let mut requirements = requires
        .iter()
        .map(|capability| Datum::String(capability.as_str().to_owned()))
        .collect::<Vec<_>>();
    requirements.sort_by_key(|datum| datum.canonical_bytes().unwrap_or_default());
    requirements.dedup();
    requirements
}

fn ref_datum(reference: &Ref) -> Datum {
    match reference {
        Ref::Symbol(symbol) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("symbol"))),
                (Symbol::new("symbol"), Datum::Symbol(symbol.clone())),
            ],
        },
        Ref::Content(content) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("content"))),
                (Symbol::new("content"), content_id_datum(content)),
            ],
        },
        Ref::Handle(handle) => Datum::Node {
            tag: core_symbol("ref"),
            fields: vec![
                (Symbol::new("kind"), Datum::Symbol(core_symbol("handle"))),
                (Symbol::new("id"), handle_id_datum(*handle)),
            ],
        },
        Ref::Coord(coordinate) => coordinate_datum(coordinate),
    }
}

fn coordinate_datum(coordinate: &Coordinate) -> Datum {
    Datum::Node {
        tag: core_symbol("ref"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(core_symbol("coord"))),
            (
                Symbol::new("space"),
                Datum::Symbol(coordinate.space.clone()),
            ),
            (
                Symbol::new("ordinal"),
                content_id_datum(&coordinate.ordinal),
            ),
        ],
    }
}

fn content_id_datum(content: &ContentId) -> Datum {
    Datum::Node {
        tag: core_symbol("content-id"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(content.algorithm.clone()),
            ),
            (Symbol::new("bytes"), Datum::Bytes(content.bytes.to_vec())),
        ],
    }
}

fn handle_id_datum(handle: HandleId) -> Datum {
    Datum::Bytes(handle.0.to_be_bytes().to_vec())
}

fn core_symbol(name: &str) -> Symbol {
    Symbol::qualified("core", name)
}
