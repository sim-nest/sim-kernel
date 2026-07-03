//! Stable ids and the [`Symbol`] type used across the kernel.
//!
//! Defines the typed id wrappers (lib, class, codec, shape, ...), the
//! well-known core class ids, and the symbol contract that names every runtime
//! entity. Symbols are reference-counted (`Arc<str>`) rather than interned:
//! there is no global dedup table, so equal names may back distinct
//! allocations.

use std::sync::Arc;

use thiserror::Error;

/// Stable id of a loaded library.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LibId(pub u32);

/// Stable id of a registered class.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ClassId(pub u32);

/// [`ClassId`] of the core `class` metaclass.
pub const CORE_CLASS_CLASS_ID: ClassId = ClassId(0);
/// [`ClassId`] of the core `nil` class.
pub const CORE_NIL_CLASS_ID: ClassId = ClassId(1);
/// [`ClassId`] of the core `bool` class.
pub const CORE_BOOL_CLASS_ID: ClassId = ClassId(2);
/// [`ClassId`] of the core `number` class.
pub const CORE_NUMBER_CLASS_ID: ClassId = ClassId(3);
/// [`ClassId`] of the core `symbol` class.
pub const CORE_SYMBOL_CLASS_ID: ClassId = ClassId(4);
/// [`ClassId`] of the core `string` class.
pub const CORE_STRING_CLASS_ID: ClassId = ClassId(5);
/// [`ClassId`] of the core `bytes` class.
pub const CORE_BYTES_CLASS_ID: ClassId = ClassId(6);
/// [`ClassId`] of the core `list` class.
pub const CORE_LIST_CLASS_ID: ClassId = ClassId(7);
/// [`ClassId`] of the core `table` class.
pub const CORE_TABLE_CLASS_ID: ClassId = ClassId(8);
/// [`ClassId`] of the core `expr` class.
pub const CORE_EXPR_CLASS_ID: ClassId = ClassId(9);
/// [`ClassId`] of the core `function` class.
pub const CORE_FUNCTION_CLASS_ID: ClassId = ClassId(10);
/// [`ClassId`] of the core `shape` class.
pub const CORE_SHAPE_CLASS_ID: ClassId = ClassId(11);
/// [`ClassId`] of the core `thunk` class.
pub const CORE_THUNK_CLASS_ID: ClassId = ClassId(12);
/// [`ClassId`] of the core `eval-request` class.
pub const CORE_EVAL_REQUEST_CLASS_ID: ClassId = ClassId(13);
/// [`ClassId`] of the core `eval-reply` class.
pub const CORE_EVAL_REPLY_CLASS_ID: ClassId = ClassId(14);
/// [`ClassId`] of the core `macro` class.
pub const CORE_MACRO_CLASS_ID: ClassId = ClassId(15);
/// [`ClassId`] of the core `shape-match` class.
pub const CORE_SHAPE_MATCH_CLASS_ID: ClassId = ClassId(16);
/// [`ClassId`] of the core `codec` class.
pub const CORE_CODEC_CLASS_ID: ClassId = ClassId(17);
/// [`ClassId`] of the core `help` class.
pub const CORE_HELP_CLASS_ID: ClassId = ClassId(18);
/// [`ClassId`] of the core `test` class.
pub const CORE_TEST_CLASS_ID: ClassId = ClassId(19);
/// [`ClassId`] of the core `number-domain` class.
pub const CORE_NUMBER_DOMAIN_CLASS_ID: ClassId = ClassId(20);
/// [`ClassId`] of the core local `eval-fabric` class.
pub const CORE_LOCAL_EVAL_FABRIC_CLASS_ID: ClassId = ClassId(21);
/// [`ClassId`] of the core `sequence` class.
pub const CORE_SEQUENCE_CLASS_ID: ClassId = ClassId(22);
/// [`ClassId`] of the core `card` class.
pub const CORE_CARD_CLASS_ID: ClassId = ClassId(23);

/// Stable id of a registered function.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FunctionId(pub u32);

/// Stable id of a registered macro.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MacroId(pub u32);

/// Stable id of a single overload case within a function.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CaseId(pub u32);

/// Stable id of a registered shape.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ShapeId(pub u32);

/// Stable id of a registered codec.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CodecId(pub u32);

/// Stable id of a registered number domain.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NumberDomainId(pub u32);

/// Stable id of a registered opaque site value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SiteId(pub u32);

/// Process-local id of a runtime value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ValueId(pub u64);

/// Tagged union over the stable id kinds a runtime entity may carry.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum RuntimeId {
    /// A class id.
    Class(ClassId),
    /// A function id.
    Function(FunctionId),
    /// A macro id.
    Macro(MacroId),
    /// A shape id.
    Shape(ShapeId),
    /// A codec id.
    Codec(CodecId),
    /// A number-domain id.
    NumberDomain(NumberDomainId),
    /// An opaque site id.
    Site(SiteId),
    /// A plain runtime value with no registered id kind.
    Value,
}

/// An optionally namespaced name for a runtime entity.
///
/// The kernel defines the symbol contract; it is the name that appears in
/// [`Expr`](crate::expr::Expr) forms, refs, and registry keys. Names are
/// reference-counted (`Arc<str>`), not interned: cloning is cheap, but there is
/// no global dedup table, so two symbols with equal text are distinct
/// allocations that merely compare equal.
///
/// # Name grammar
///
/// A symbol is a `namespace` (optional) plus a bare `name`. Bare names are
/// treated as opaque text by the kernel; the qualified rendering is
/// `namespace/name` (or just `name` when unqualified). [`as_qualified_str`] is
/// therefore **not injective** when a component contains `/`:
/// `Symbol::new("a/b")` and `Symbol::qualified("a", "b")` render identically yet
/// are unequal. Construct symbols at untrusted boundaries with
/// [`Symbol::checked`], which rejects `/`, NUL, and control characters so the
/// qualified rendering stays unambiguous.
///
/// [`as_qualified_str`]: Symbol::as_qualified_str
///
/// # Examples
///
/// ```
/// # use sim_kernel::id::Symbol;
/// let plain = Symbol::new("car");
/// assert_eq!(plain.as_qualified_str(), "car");
///
/// let qualified = Symbol::qualified("core", "Bool");
/// assert_eq!(qualified.as_qualified_str(), "core/Bool");
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Symbol {
    /// The optional namespace component (the part before `/`).
    pub namespace: Option<Arc<str>>,
    /// The bare name component.
    pub name: Arc<str>,
}

impl Symbol {
    /// Creates an unqualified symbol with no namespace.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            namespace: None,
            name: name.into(),
        }
    }

    /// Creates an unqualified symbol after validating the name.
    ///
    /// Unlike [`Symbol::new`], this rejects names that would make the qualified
    /// rendering ambiguous or unprintable: names containing `/` (the
    /// namespace separator), NUL, or any other control character. Codecs and
    /// other untrusted boundaries should adopt this constructor; trusted kernel
    /// call sites may keep using the infallible [`Symbol::new`].
    pub fn checked(name: impl Into<Arc<str>>) -> Result<Self, SymbolError> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self {
            namespace: None,
            name,
        })
    }

    /// Creates a symbol qualified by `namespace`.
    pub fn qualified(namespace: impl Into<Arc<str>>, name: impl Into<Arc<str>>) -> Self {
        Self {
            namespace: Some(namespace.into()),
            name: name.into(),
        }
    }

    /// Renders the symbol as `namespace/name`, or just `name` when unqualified.
    pub fn as_qualified_str(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}/{}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// Reason a name was rejected by [`Symbol::checked`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SymbolError {
    /// The name contained `/`, which is reserved as the namespace separator.
    #[error("symbol name {0:?} contains the reserved namespace separator '/'")]
    ContainsSeparator(String),
    /// The name contained a NUL or other control character.
    #[error("symbol name {0:?} contains a control character")]
    ContainsControl(String),
}

fn validate_name(name: &str) -> Result<(), SymbolError> {
    if name.contains('/') {
        return Err(SymbolError::ContainsSeparator(name.to_owned()));
    }
    if name.chars().any(|ch| ch.is_control()) {
        return Err(SymbolError::ContainsControl(name.to_owned()));
    }
    Ok(())
}

impl core::fmt::Display for Symbol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.as_qualified_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_class_ids_are_unique() {
        let ids = [
            CORE_CLASS_CLASS_ID,
            CORE_NIL_CLASS_ID,
            CORE_BOOL_CLASS_ID,
            CORE_NUMBER_CLASS_ID,
            CORE_SYMBOL_CLASS_ID,
            CORE_STRING_CLASS_ID,
            CORE_BYTES_CLASS_ID,
            CORE_LIST_CLASS_ID,
            CORE_TABLE_CLASS_ID,
            CORE_EXPR_CLASS_ID,
            CORE_FUNCTION_CLASS_ID,
            CORE_SHAPE_CLASS_ID,
            CORE_THUNK_CLASS_ID,
            CORE_EVAL_REQUEST_CLASS_ID,
            CORE_EVAL_REPLY_CLASS_ID,
            CORE_MACRO_CLASS_ID,
            CORE_SHAPE_MATCH_CLASS_ID,
            CORE_CODEC_CLASS_ID,
            CORE_HELP_CLASS_ID,
            CORE_TEST_CLASS_ID,
            CORE_NUMBER_DOMAIN_CLASS_ID,
            CORE_LOCAL_EVAL_FABRIC_CLASS_ID,
            CORE_SEQUENCE_CLASS_ID,
            CORE_CARD_CLASS_ID,
        ];
        let unique = ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn checked_accepts_a_plain_name() {
        let symbol = Symbol::checked("car").expect("plain name is valid");
        assert_eq!(symbol, Symbol::new("car"));
    }

    #[test]
    fn checked_rejects_the_namespace_separator() {
        assert_eq!(
            Symbol::checked("a/b"),
            Err(SymbolError::ContainsSeparator("a/b".to_owned()))
        );
    }

    #[test]
    fn checked_rejects_nul_and_control_characters() {
        assert_eq!(
            Symbol::checked("a\0b"),
            Err(SymbolError::ContainsControl("a\0b".to_owned()))
        );
        assert_eq!(
            Symbol::checked("a\tb"),
            Err(SymbolError::ContainsControl("a\tb".to_owned()))
        );
    }
}
