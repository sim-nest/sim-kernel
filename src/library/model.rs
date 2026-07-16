use std::sync::Arc;

use crate::{
    capability::CapabilityName,
    env::Cx,
    error::Result,
    id::{
        ClassId, CodecId, FunctionId, LibId, MacroId, NumberDomainId, RuntimeId, ShapeId, Symbol,
    },
    value::Value,
};

/// A library version string, compared component-wise by dotted numeric
/// components, ignoring trailing zero components.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub String);

/// The ABI version a library targets, as a major/minor pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AbiVersion {
    /// Major ABI version; incompatible changes bump this.
    pub major: u16,
    /// Minor ABI version; backward-compatible additions bump this.
    pub minor: u16,
}

/// The kind of artifact a library is loaded from.
///
/// Every variant is codec-agnostic: the kernel never names a concrete codec.
/// A library defined by decoding source through some codec is
/// [`LibTarget::CodecSource`], carrying that codec's [`Symbol`] as open data,
/// so a new source dialect is expressible without editing this enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibTarget {
    /// A native Rust library linked into the host.
    Native,
    /// A Wasm component loaded through the ABI transport.
    WasmComponent,
    /// A library defined by source decoded through the named codec (open data,
    /// e.g. the symbol `codec/lisp`).
    CodecSource(Symbol),
    /// A library that contributes only data exports, no executable behavior.
    DataOnly,
    /// A library registered directly by the host (trusted).
    HostRegistered,
}

impl LibTarget {
    /// Renders the target as its stable serialized [`Symbol`].
    ///
    /// The closed variants serialize to unqualified tags (`native`,
    /// `wasm-component`, `data-only`, `host-registered`); a
    /// [`LibTarget::CodecSource`] serializes to its codec symbol verbatim
    /// (e.g. `codec/lisp`), keeping the codec identity as open data rather than
    /// a closed kernel string.
    pub fn to_symbol(&self) -> Symbol {
        match self {
            LibTarget::Native => Symbol::new("native"),
            LibTarget::WasmComponent => Symbol::new("wasm-component"),
            LibTarget::CodecSource(codec) => codec.clone(),
            LibTarget::DataOnly => Symbol::new("data-only"),
            LibTarget::HostRegistered => Symbol::new("host-registered"),
        }
    }

    /// Reconstructs a target from its serialized [`Symbol`].
    ///
    /// The unqualified closed tags map to their variants. The legacy
    /// `lisp-source` tag is accepted for backward compatibility and decodes to
    /// `CodecSource(codec/lisp)` so existing serialized manifests still load.
    /// Any other symbol is treated as an open [`LibTarget::CodecSource`].
    pub fn from_symbol(symbol: &Symbol) -> Self {
        if symbol.namespace.is_none() {
            match symbol.name.as_ref() {
                "native" => return LibTarget::Native,
                "wasm-component" => return LibTarget::WasmComponent,
                "data-only" => return LibTarget::DataOnly,
                "host-registered" => return LibTarget::HostRegistered,
                // Legacy tag: pre-CodecSource manifests named the lisp codec by
                // the closed string "lisp-source".
                "lisp-source" => return LibTarget::CodecSource(Symbol::qualified("codec", "lisp")),
                _ => {}
            }
        }
        LibTarget::CodecSource(symbol.clone())
    }
}

/// A dependency on another library, optionally pinned to a minimum version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Symbol of the required library.
    pub id: Symbol,
    /// Lowest acceptable version, if any.
    pub minimum_version: Option<Version>,
}

/// A single export declared by a library manifest, by export kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Export {
    /// A class export; `class_id` is present once a stable id is reserved.
    Class {
        /// Symbol the class is exported under.
        symbol: Symbol,
        /// Reserved stable class id, if known.
        class_id: Option<ClassId>,
    },
    /// A function export; `function_id` is present once a stable id is reserved.
    Function {
        /// Symbol the function is exported under.
        symbol: Symbol,
        /// Reserved stable function id, if known.
        function_id: Option<FunctionId>,
    },
    /// A macro export; `macro_id` is present once a stable id is reserved.
    Macro {
        /// Symbol the macro is exported under.
        symbol: Symbol,
        /// Reserved stable macro id, if known.
        macro_id: Option<MacroId>,
    },
    /// A shape export; `shape_id` is present once a stable id is reserved.
    Shape {
        /// Symbol the shape is exported under.
        symbol: Symbol,
        /// Reserved stable shape id, if known.
        shape_id: Option<ShapeId>,
    },
    /// A codec export; `codec_id` is present once a stable id is reserved.
    Codec {
        /// Symbol the codec is exported under.
        symbol: Symbol,
        /// Reserved stable codec id, if known.
        codec_id: Option<CodecId>,
    },
    /// A number-domain export; `number_domain_id` is present once reserved.
    NumberDomain {
        /// Symbol the number domain is exported under.
        symbol: Symbol,
        /// Reserved stable number-domain id, if known.
        number_domain_id: Option<NumberDomainId>,
    },
    /// A plain value export.
    Value {
        /// Symbol the value is exported under.
        symbol: Symbol,
    },
    /// An opaque placement-site export.
    ///
    /// The symbol is the placement key. The kernel stores only the runtime
    /// value and stable id; libraries outside the kernel decide whether that
    /// value behaves as an evaluation site.
    Site {
        /// Symbol the site is exported under.
        symbol: Symbol,
        /// Reserved opaque runtime id, if known.
        runtime_id: Option<RuntimeId>,
    },
    /// An open export declaration with no kernel runtime-value model.
    ///
    /// Open declarations let manifests name export kinds that the kernel does
    /// not resolve itself. They can commit as declared, unsupported, or invalid
    /// export records, but they do not carry stable runtime ids.
    Open {
        /// Open export kind.
        kind: ExportKind,
        /// Symbol the export is declared under.
        symbol: Symbol,
    },
}

/// An open, symbol-keyed export kind tag.
///
/// Export kinds are carried as data rather than a closed kernel enum so that
/// libraries can introduce new kinds without a kernel change; the well-known
/// kinds are named by the associated constants.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ExportKind(Symbol);

impl ExportKind {
    /// Well-known kind name for class exports.
    pub const CLASS: &'static str = "class";
    /// Well-known kind name for function exports.
    pub const FUNCTION: &'static str = "function";
    /// Well-known kind name for macro exports.
    pub const MACRO: &'static str = "macro";
    /// Well-known kind name for shape exports.
    pub const SHAPE: &'static str = "shape";
    /// Well-known kind name for codec exports.
    pub const CODEC: &'static str = "codec";
    /// Well-known kind name for number-domain exports.
    pub const NUMBER_DOMAIN: &'static str = "number-domain";
    /// Well-known kind name for plain value exports.
    pub const VALUE: &'static str = "value";
    /// Well-known kind name for opaque site exports.
    pub const SITE: &'static str = "site";

    /// Wraps an arbitrary symbol as an export kind.
    pub fn new(symbol: Symbol) -> Self {
        Self(symbol)
    }

    /// Builds an export kind from a well-known static kind name.
    pub fn named(name: &'static str) -> Self {
        Self(Symbol::new(name))
    }

    /// Returns the underlying symbol.
    pub fn symbol(&self) -> &Symbol {
        &self.0
    }

    /// Returns the unqualified kind name, or `None` if the symbol is namespaced.
    pub fn name(&self) -> Option<&str> {
        match &self.0.namespace {
            Some(_) => None,
            None => Some(self.0.name.as_ref()),
        }
    }

    /// Maps a well-known kind to its static label for duplicate-export errors,
    /// falling back to `"export"` for unrecognized kinds.
    pub fn duplicate_error_kind(&self) -> &'static str {
        match self.name() {
            Some(Self::CLASS) => Self::CLASS,
            Some(Self::FUNCTION) => Self::FUNCTION,
            Some(Self::MACRO) => Self::MACRO,
            Some(Self::SHAPE) => Self::SHAPE,
            Some(Self::CODEC) => Self::CODEC,
            Some(Self::NUMBER_DOMAIN) => Self::NUMBER_DOMAIN,
            Some(Self::VALUE) => Self::VALUE,
            Some(Self::SITE) => Self::SITE,
            _ => "export",
        }
    }
}

/// The resolution state of an export within a loaded library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportState {
    /// Resolved to a registry-assigned stable runtime id.
    Resolved {
        /// The runtime id the export resolved to.
        id: RuntimeId,
    },
    /// Declared in the manifest but not yet resolved to behavior.
    Declared,
    /// Recognized but not supported in this host, with a human-readable reason.
    Unsupported {
        /// Why the export is unsupported here.
        reason: String,
    },
    /// Rejected as invalid, with a human-readable error.
    Invalid {
        /// Why the export was rejected.
        error: String,
    },
}

/// One resolved export row: its kind, symbol, and resolution state.
///
/// `ExportRecord` is the open metadata surface the kernel prefers over closed
/// enums for reporting what a library contributes (see the README "Library
/// system" section).
///
/// # Examples
///
/// ```
/// use sim_kernel::library::{Export, ExportKind, ExportRecord, ExportState};
/// use sim_kernel::Symbol;
///
/// let export = Export::Value {
///     symbol: Symbol::new("answer"),
/// };
/// let record: ExportRecord = export.declared_record();
/// assert_eq!(record.kind, ExportKind::named(ExportKind::VALUE));
/// assert_eq!(record.symbol, Symbol::new("answer"));
/// assert_eq!(record.state, ExportState::Declared);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRecord {
    /// The export kind.
    pub kind: ExportKind,
    /// The symbol the export is bound under.
    pub symbol: Symbol,
    /// The current resolution state of the export.
    pub state: ExportState,
}

impl Export {
    /// Returns the symbol this export is declared under.
    pub fn symbol(&self) -> &Symbol {
        match self {
            Self::Class { symbol, .. }
            | Self::Function { symbol, .. }
            | Self::Macro { symbol, .. }
            | Self::Shape { symbol, .. }
            | Self::Codec { symbol, .. }
            | Self::NumberDomain { symbol, .. }
            | Self::Value { symbol }
            | Self::Site { symbol, .. }
            | Self::Open { symbol, .. } => symbol,
        }
    }

    /// Returns the static kind label for this export.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Class { .. } => "class",
            Self::Function { .. } => "function",
            Self::Macro { .. } => "macro",
            Self::Shape { .. } => "shape",
            Self::Codec { .. } => "codec",
            Self::NumberDomain { .. } => "number-domain",
            Self::Value { .. } => "value",
            Self::Site { .. } => "site",
            Self::Open { .. } => "export",
        }
    }

    /// Returns this export's kind as an [`ExportKind`] tag.
    pub fn kind_symbol(&self) -> ExportKind {
        match self {
            Self::Open { kind, .. } => kind.clone(),
            _ => ExportKind::named(self.kind()),
        }
    }

    /// Builds a [`Declared`](ExportState::Declared) [`ExportRecord`] for this
    /// export.
    pub fn declared_record(&self) -> ExportRecord {
        ExportRecord {
            kind: self.kind_symbol(),
            symbol: self.symbol().clone(),
            state: ExportState::Declared,
        }
    }
}

/// The self-description a library presents at load time.
///
/// The manifest names the library, its version and ABI, how it is loaded, what
/// it requires, what capabilities it requests, and what it exports. The kernel
/// validates and registers against this; the library supplies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibManifest {
    /// Symbol identifying the library.
    pub id: Symbol,
    /// The library's version.
    pub version: Version,
    /// The ABI version the library targets.
    pub abi: AbiVersion,
    /// How the library is loaded.
    pub target: LibTarget,
    /// Other libraries this one depends on.
    pub requires: Vec<Dependency>,
    /// Capabilities the library requests at load time.
    pub capabilities: Vec<CapabilityName>,
    /// The exports the library declares.
    pub exports: Vec<Export>,
}

impl LibManifest {
    /// Returns a [`Declared`](ExportState::Declared) [`ExportRecord`] for each
    /// declared export.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_kernel::library::{
    ///     AbiVersion, Export, ExportKind, LibManifest, LibTarget, Version,
    /// };
    /// use sim_kernel::Symbol;
    ///
    /// let manifest = LibManifest {
    ///     id: Symbol::new("demo"),
    ///     version: Version("0.1.0".to_owned()),
    ///     abi: AbiVersion { major: 0, minor: 1 },
    ///     target: LibTarget::HostRegistered,
    ///     requires: Vec::new(),
    ///     capabilities: Vec::new(),
    ///     exports: vec![Export::Value { symbol: Symbol::new("answer") }],
    /// };
    ///
    /// let records = manifest.declared_export_records();
    /// assert_eq!(records.len(), 1);
    /// assert_eq!(records[0].kind, ExportKind::named(ExportKind::VALUE));
    /// ```
    pub fn declared_export_records(&self) -> Vec<ExportRecord> {
        self.exports.iter().map(Export::declared_record).collect()
    }
}

/// A library that has been loaded and committed into the [`Registry`].
///
/// [`Registry`]: crate::library::Registry
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedLib {
    /// The stable id assigned at load time.
    pub id: LibId,
    /// The manifest the library was loaded from.
    pub manifest: LibManifest,
    /// The resolved export records produced during load.
    pub exports: Vec<ExportRecord>,
    /// Whether the library was loaded as trusted (host-registered).
    pub trusted: bool,
}

/// The outcome of running a library-supplied [`Test`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestReport {
    /// Symbol naming the test.
    pub name: Symbol,
    /// Whether the test passed.
    pub passed: bool,
    /// Optional human-readable detail (e.g. a failure message).
    pub detail: Option<String>,
    /// The mode the test ran under.
    pub mode: Symbol,
    /// Events recorded while running the test.
    pub events: Vec<Value>,
    /// The effect produced by the test, if any.
    pub effect: Option<Value>,
    /// A shape-level report value, if the test produced one.
    pub shape_report: Option<Value>,
    /// Whether the test was skipped rather than run.
    pub skipped: bool,
}

impl TestReport {
    /// Builds a report from a pass/fail result with default (unknown) mode and
    /// no events.
    pub fn from_result(name: Symbol, passed: bool, detail: Option<String>) -> Self {
        Self {
            name,
            passed,
            detail,
            mode: Symbol::new("unknown"),
            events: Vec::new(),
            effect: None,
            shape_report: None,
            skipped: false,
        }
    }

    /// Builds a report marking the named test as skipped.
    pub fn skipped(name: Symbol, detail: Option<String>) -> Self {
        Self {
            name,
            passed: false,
            detail,
            mode: Symbol::new("unknown"),
            events: Vec::new(),
            effect: None,
            shape_report: None,
            skipped: true,
        }
    }
}

/// A library-supplied test the registry can hold and run.
///
/// The kernel defines the contract; the test body is library behavior.
pub trait Test: Send + Sync {
    /// Symbol naming this test.
    fn symbol(&self) -> Symbol;
    /// Symbol of the library that owns this test.
    fn lib(&self) -> Symbol;
    /// Produces a value describing this test without running it.
    fn describe(&self, cx: &mut Cx) -> Result<Value>;
    /// Runs the test and returns its [`TestReport`].
    fn run(&self, cx: &mut Cx) -> Result<TestReport>;
}

/// A registered test with its owning library and the subjects it covers.
#[derive(Clone)]
pub struct RegisteredTest {
    /// Symbol naming the test.
    pub symbol: Symbol,
    /// Symbol of the owning library.
    pub lib: Symbol,
    /// The test implementation.
    pub test: Arc<dyn Test>,
    /// Symbols of the exports this test exercises.
    pub subjects: Vec<Symbol>,
}
