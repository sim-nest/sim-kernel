# sim-kernel

`sim-kernel` is the small protocol kernel of the SIM constellation: the contract
layer that defines the shared type vocabulary, trait contracts, identity/storage
substrate, and transport framing that every SIM library and host plugs into.

New to SIM? You probably want to run it first -- see sim-run (the `sim` command,
`cargo install sim-run`) and sim-say for the tour. This repo is the kernel: the
small contract layer everything else plugs into, not something you run directly.

## Smallest example

The kernel's most basic type is `Symbol`, a name with an optional namespace:

```bash
cargo add sim-kernel
```

```rust
use sim_kernel::id::Symbol;

let plain = Symbol::new("car");
assert_eq!(plain.as_qualified_str(), "car");

let qualified = Symbol::qualified("core", "Bool");
assert_eq!(qualified.as_qualified_str(), "core/Bool");
```

This is a verbatim passing doctest (`src/id.rs:146`); it asserts the qualified
rendering rather than printing output. Every first-reach kernel type carries a
similar `# Examples` doctest -- those are the authoritative usage references.

SIM is an expandable Rust runtime: the kernel defines contracts, and loadable
libraries provide behavior. The kernel is a protocol surface, not a closed
runtime container. It carries the type vocabulary, trait contracts, identity and
storage substrate, and transport framing that every library and host agrees on;
it does not carry concrete syntax, arithmetic, codecs, or application policy.

The data flow the kernel frames is:

```text
tokens -> checked forms -> objects -> checked calls -> objects -> encoded forms
```

Do not describe SIM as a Lisp runtime. It is a Rust runtime with multiple codec
surfaces; Lisp is one codec, not the system identity.

## What this crate is

This crate is the protocol/contract layer. Its public surface is re-exported
from `src/lib.rs`. Concrete behavior -- the standard distribution, number
domains, codecs, list and table backends, browse/help/test implementations,
server and agent surfaces -- lives in sibling repositories that load against
these contracts.

The full per-repo feature and package map lives in each repo's `Cargo.toml` and
is not this README's concern. This README records the three kernel-level
contracts that have no other home: the catalog-backed registry substrate, the
Card record schema, and the kernel feature contract.

## The kernel boundary

The boundary is the central architecture discipline of the constellation. Keep
the kernel small and protocol-only; concrete behavior belongs in libraries.

### What the kernel MAY define

- **Core types**: `Symbol`, `Value`, `Expr` (the codec-neutral expression
  graph), `NumberLiteral`, `Origin`/`Span`/`Trivia`, `Error`, `Diagnostic`.
- **Identity and storage**: `Ref` (symbol, content id, process-local handle,
  ranked coordinate), `Datum` and the content-addressable datum store, claim and
  fact stores, handle store, `Card` records, operation keys (`OpKey`), and the
  stable id types (`ClassId`, `FunctionId`, `MacroId`, `CaseId`, `ShapeId`,
  `CodecId`, `NumberDomainId`, `LibId`, `RuntimeId`).
- **Runtime coordination**: `Cx`, `Registry`, `Lib`/`LibManifest`, `Linker`,
  `ExportRecord`/`ExportState`, `CapabilityName`/`CapabilitySet`/`ReadPolicy`/
  `TrustLevel`, the event and effect ledgers, control policy, and rank metadata.
- **Behavior contracts**: the `Object`/`ObjectCompat`/`Op` traits, plus the
  `Callable`, `Class`, `Factory`, `NumberDomain`, `EvalPolicy`, `MacroExpander`,
  and `EvalFabric` contracts.
- **The `Shape` protocol**: one shared engine for parsing, checking, binding,
  dispatch, macro syntax, codec grammar, lambda locals, and overload selection
  (`Shape`, `ShapeMatch`, `ShapeBindings`, object-accessible via `as_shape`).
- **Eval-policy and macro-expander contracts**: `Phase`, `Demand`,
  `PreparedArgs`, the predefined policies, and the macro expansion interface.
- **ABI byte-frame and manifest transport**: the native (`NativeLibAbiV1`) and
  Wasm frame/manifest types for crossing the host boundary.

### What the kernel MUST NOT define

- Concrete Lisp, JSON, or Algol parsing as hardwired kernel behavior.
- Concrete number domains or arithmetic.
- Concrete help, browse, test, server, or agent implementations.
- Wasm guest object semantics beyond ABI transport.
- Transport, trigger, or loader policy.

### The extension rule

When metadata exposure grows, prefer `ExportRecord`-style data and `Object::op`,
claims, snapshots, and refs over new closed kernel enums plus parallel maps. A
new behavior should reach for the open, data-driven path before it adds another
closed variant to the kernel.

The location-transparent distributed eval surface is `realize` plus
`EvalFabric`; server and agent code targets these, never transport-specific
APIs. Codecs are first-class runtime objects (split into decoders and encoders)
provided by libraries, not kernel behavior. Pluggable backends (list, table,
number domains) and the standard distribution are libraries loaded by default.

## Contract: registry catalog substrate

The `Registry` is the public typed facade for libraries, exports, runtime
values, tests, number dispatch, and promotion rules. Its authoritative storage
is a private catalog (`catalog::CatalogStore`). Borrowed public APIs keep
projection caches where Rust borrowing requires stable map or slice references,
but those caches are rebuilt from catalog rows and are not the authority.

### Catalog core types (`catalog`)

- `CatalogStore`: `Cx`-free, lock-free table storage owned by the registry.
- `CatalogTableSpec`: table name, write policy, owner, required fields, and
  unique fields.
- `CatalogRow`: deterministic `Expr` data plus private live payload cells.
- `CatalogTx`: atomic put, delete, and sequence transactions.
- `CatalogEvent`: append-only audit entries for committed operations.

Write policies (`CatalogWritePolicy`): `Mutable` (insert, replace, delete),
`Sealed` (insert a key once), `AppendOnly` (insert but never change or delete),
`Derived` (direct writes fail).

Registry id generation uses catalog sequences. Direct registration and library
load commit catalog rows before projection caches update. Duplicate libs and
exports are rejected by catalog conflicts mapped back to the existing
`DuplicateLib` and `DuplicateExport` errors.

### Registry schema

Registry catalog table names are open symbols:

```text
registry/schema
registry/sequences
registry/libs
registry/exports
registry/runtime
registry/tests
registry/tests-by-lib
registry/number-ops
registry/promotion-rules
registry/value-promotion-rules
```

`registry/libs` stores loaded library manifests. `registry/exports` stores
resolved, declared, unsupported, and invalid export records. `registry/runtime`
stores runtime values by runtime kind and stable id, with live value payloads
kept out of serializable row data. `registry/tests` stores test metadata and
live test payloads. Number operations and promotion rules are append-only.

### Snapshots and deltas

`CatalogSnapshot::to_expr()` emits deterministic data for table specs, row data,
sequences, and catalog epoch; `CatalogSnapshot::from_expr(...)` parses that shape
and `CatalogStore::from_snapshot(...)` restores data-only rows. Live payloads
(runtime values, tests) are not serialized; snapshot data uses
`catalog/unresolved-live` markers that carry stable table, key, field, epoch,
kind, symbol, and display information for unresolved host payloads.

`CatalogStore::delta_since(...)` returns a `CatalogDelta` with source and target
epochs, compatible table specs, changed rows, deleted rows, and sequence
changes. `CatalogStore::apply_delta(...)` validates source epoch, table
compatibility, sealed-row conflicts, change epochs, and target epoch before it
mutates the store. Catalog deltas are catalog-level data outside the `realize`
event payload.

Boot receipts capture the loaded library surface without serializing live Rust
objects. `LibBootReceipt` records the assigned `LibId`, requested and resolved
`LibSourceSpec`, manifest identity, loaded dependency edges, and committed
`ExportRecord`s. `RegistryBootState` stores load-order receipts as `Datum`, and
`LoaderRegistry` replays that state through the resolved sources.

### Read-only table views

Registry catalog table exposure is read-only. `registry_catalog_view` creates a
`CatalogDirView` over a catalog snapshot. Opening a table returns a
`CatalogTableView`; writes fail closed with catalog read-only errors. Missing
`get` returns `nil`, and keys and entries are sorted by `Symbol`.

Exposure through a browse Card is capability-gated: the `registry/catalog` facet
value is the read-only catalog `Dir`/`Table` view only when the caller holds
`registry.catalog.read`, and is a Redaction value otherwise. Ordinary runtime
users who want catalog semantics without registry authority opt into a mutable
table backend that wraps a private `CatalogStore` -- that backend is a library,
not kernel behavior.

## Contract: Card records and the browse/help/test schema

The kernel owns the `Card` record (`src/card.rs`). A `Card` is an ordinary
runtime object: a `subject` `Ref` plus ordered `(Symbol, Value)` entries,
projected from claims and fallback table data. It is the stable, machine-readable
record that agents walk. Browse output is ordinary runtime data; consumers must
not parse display strings.

The kernel fixes the Card schema, in this order:

```text
subject
kind
help
args
result
tests
ops
requires
see-also
shape-known
```

- `subject`: ref-like value being described.
- `kind`: open symbol (e.g. `core/function`, `core/shape`, `browse/catalog`).
- `help`: help payload.
- `args` / `result`: shape ref or `core/Any`; never omitted.
- `tests`: list of test records.
- `ops`: operation keys.
- `requires`: required capability symbols.
- `see-also`: refs an agent may browse next.
- `shape-known`: bool.

The kernel owns these records; libraries implement browse *over* them. The
full agent-facing Card -- the fixed fields extended with `facets`, `coverage`,
`provenance`, and `freshness`, and the `browse/Help`, `browse/Test`,
`browse/Coverage`, `browse/Facet`, `browse/Redaction`, and `browse/TestReport`
tables -- is a library schema layered on the kernel fields, reached through the
`core/browse` entry point. Those fixed schema values, the
graph-walking helpers (`core/browse-neighbors`, `core/browse-path`), and
capability-gated test execution live in the browse library, not in this kernel.

### Browse schema rules (for library implementations)

- The fixed Card field order is stable.
- Facets are the extension mechanism for domains and runtime surfaces; new facet
  names and kinds are open symbols. New Card extension data should be added as
  facets rather than new fixed fields.
- Hidden data is represented by Redaction values, never by silently omitting a
  known field. Replacing structured data with display-only strings is a breaking
  change.
- Browse and test surfaces are capability-gated by library-owned tokens.
  Browse libraries mint and enforce:
  - `browse.read`: optional ordinary browse gate.
  - `browse.run-tests`: required to execute tests (otherwise test descriptions
    stay visible but execution fails closed with `CapabilityDenied`).
  - `browse.internal`: required to reveal private runtime internals and host
    detail.
  The kernel-owned `registry.catalog.read` capability gates only the registry
  catalog view exposed through the `registry/catalog` facet.

## Contract: kernel feature contract

The kernel defines a protocol surface that any conforming implementation must
provide. The contracts below are the kernel-level material; the concrete
behaviors they enable are library responsibilities.

### Core data types

- `Value`: a cheap, cloneable handle wrapping `Arc<dyn RuntimeObject>`, where
  `RuntimeObject: Object + ObjectCompat + Any + Send + Sync`. Equality and
  hashing are `Arc` pointer identity. Stable runtime identity comes from
  `ObjectHeader`, `Ref`, claims, snapshots, and operation specs -- not from new
  closed value variants.
- `Expr`: the codec-neutral expression graph all codecs transcode through. It
  carries canonical equality (`Expr::canonical_eq`) and a canonical key
  (`Expr::canonical_key`) so map and set entries sort deterministically and
  round-trips can be validated structurally. `LocatedExpr` and `LocatedExprTree`
  attach `Origin` for lossless source coverage.
- `Symbol`: a name with optional namespace (`namespace/name`), with stable
  canonical-key computation.
- `NumberLiteral`: a `{ domain, canonical }` pair preserving the exact value as a
  domain-tagged canonical string.
- `Origin`: `codec`, `source`, `span`, and `trivia` metadata for lossless
  round-trips; canonical encoding may drop trivia.
- `Error` and `Diagnostic`: the closed failure vocabulary and the structured,
  severity-tagged diagnostic record accumulated through parsing, eval, and codec
  operations.

### Runtime context: `Cx`

`Cx` is the mutable execution context threaded through eval, parsing, and codec
operations. It carries the environment, accumulated diagnostics, granted
capabilities, eval policy, optional macro expander, object `Factory`, the
`Registry`, list and table backends, promotion-search limits, the source
registry, the datum/handle/fact stores, the effect ledger, and the active
control policy. It exposes resolution (`resolve_class`, `resolve_function`,
`resolve_shape`, `resolve_codec`, ...), invocation (`eval_expr`, `call_value`,
`call_class`), capability checks (`require`, `grant`), diagnostic draining,
fact insertion and query, and number-literal parse/encode against the active
domains.

### Behavior contracts

- `Object` / `ObjectCompat` / `Op`: every runtime value implements `Object`
  (`display`, `as_any`, and defaulted `header`/`op`/`claims`/`snapshot`) and
  `ObjectCompat` (class, `as_expr`, truth, `as_table`, and optional adapters such
  as `as_shape`, `as_callable`, `as_number_domain`, `as_eval_fabric`,
  `as_stream`, `as_sequence`, `as_thunk`, `as_list`, `as_table_impl`, `as_dir`).
  New behavior prefers `op`, claims, snapshots, and refs over new kernel enums.
- `Callable`: `call` (pre-evaluated args) and `call_exprs` (raw exprs, default
  eval-then-call), plus optional browse arg/result shapes.
- `Class`: callable constructors carrying id, symbol, parents, subclass query,
  constructor and instance shapes, optional read constructor, and members.
- `Factory`: the object construction boundary that enforces kernel invariants
  and yields the only path to constructing core `Value`s and opaque host objects.
- `EvalPolicy`: injectable evaluation strategy (`NoopEvalPolicy`, `EagerPolicy`,
  `NeedPolicy`/`LazyPolicy`, `HybridPolicy`, `StrictByShapePolicy`) driven by
  `Phase` (Read/Expand/Compile/Eval) and `Demand`.
- `MacroExpander`: phase-gated expression transformation governed by eval policy.
- `NumberDomain` and `PromotionRule`/`PromotionSearchLimits`: domains declare
  parse/encode/promotion, and cross-domain operations resolve by lowest-cost
  promotion-graph search bounded by `PromotionSearchLimits`.

### Distributed evaluation

`EvalFabric::realize` is the location-transparent eval endpoint, framed by
`EvalRequest` (expr, optional result shape, required capabilities, deadline,
`Consistency`, `EvalMode`, answer/stream bounds, trace flag) and `EvalReply`
(value, diagnostics, optional trace). The same surface serves local and remote
eval; transport selection is library and policy concern.

### Library system

`Lib` is the contract for every extension point. A `LibManifest` carries id,
`Version`, `AbiVersion`, `LibTarget`, dependencies, requested capabilities, and
`Export` declarations. `ExportKind` covers classes, functions, macros, shapes,
codecs, number domains, values, opaque sites, and open declarations; resolution
produces `ExportRecord`/`ExportState` rows. `Registry` and `Linker` handle
registration, lookup, topological dependency resolution, and catalog-backed
identity storage.

### Capabilities and trust

`CapabilityName` wraps `Arc<str>`; `CapabilitySet` is an ordered `BTreeSet`.
`ReadPolicy` carries a `TrustLevel` (`Untrusted`, `TrustedSource`,
`HostInternal`): an untrusted context denies `read-eval` even when the
capability is granted. Kernel capability tokens include `read-construct`,
`read-eval`, `loader.native`, the `macro.expand*` family, `eval.fabric` /
`eval.remote`, the `control.*` family, `kernel.fact.private`,
`list.force.unbounded`, and `registry.catalog.read`. Libraries mint
behavior-specific tokens such as browse, storage, configured-backend,
remote-table, and logic capabilities. Capabilities are ordinary, inspectable
runtime data, not static strings, and must be checked before privileged
operations. `CapabilitySet::intersect` and `diminish(current, allowed)` compute
monotone subsets for explicit narrowed runs: the result contains only
capabilities present in both inputs and never adds authority the caller did not
already hold.

### Stable ids and ABI transport

The kernel assigns stable ids (`ClassId`, `FunctionId`, `MacroId`, `CaseId`,
`ShapeId`, `CodecId`, `NumberDomainId`, `LibId`, summed by `RuntimeId`) reserved
from registry catalog sequences. `Value` equality remains `Arc` pointer
identity; ids are for registry-resolved objects, not the public `Value` equality
contract.

The kernel defines the byte-frame and manifest transport for crossing the host
boundary: the native ABI (`NativeLibAbiV1` header with manifest, exports, and
entrypoints) and the Wasm export contract. Frame decoding is resource-limited,
rejects oversized or malformed input, and returns errors rather than panicking.
Concrete binary codec grammar and wasm guest semantics beyond this transport are
library concerns.

## Validation

`sim-kernel` builds and tests from a lone clone (it is the dependency root); the rest of the constellation builds together in the workspace.

```bash
cargo fmt --all --check && cargo test && cargo clippy --all-targets -- -D warnings && cargo doc --no-deps
cargo run -p xtask -- simdoc --check
cargo run -p xtask -- check-file-sizes
```

## Documentation lanes

`cargo run -p xtask -- simdoc` builds the public documentation lanes:

- API docs: `target/doc/`
- Agent cards: `docs/agents/cards.jsonl` and `docs/agents/card-index.json`
- Human docs: `docs/humans/`
- Diagrams: `docs/diagrams/src/` and `docs/diagrams/generated/`

Generated browse cards for the kernel protocol appear in
`docs/generated/card-index.json`. The `docs/agents/` lane is the authored-card
input lane; it is empty because the kernel publishes protocol cards from source
facts and generated contract data instead of separate authored card files.

The same command writes split contract files under `docs/generated/`. Everything
under `docs/` is generated; do not hand-edit it.

### Rustdoc conventions

Public API documentation in `src/` follows one house style:

- Every public item opens with a one-line summary sentence, then context.
- Where an item is a contract that libraries implement, state the
  protocol-versus-behavior boundary explicitly: the kernel defines the contract;
  libraries supply the behavior.
- The types and constructors an implementer reaches for first carry a
  `# Examples` doctest. Doctests compile and pass under `cargo test`.
- Cross-reference with intra-doc links so the rustdoc index and the generated
  card graph connect, and link back to the contract sections of this README
  rather than restating them.

The public API is documentation-gated: `lib.rs` denies `missing_docs`, so every
public item, field, variant, and method must be documented for the crate to
build.

### Worked examples and recipes

The kernel's runnable, verified examples are its rustdoc doctests: every
first-reach type and constructor carries a `# Examples` block that compiles and
passes under `cargo test`. These are the authoritative "how do I use this"
references for the kernel contracts.

The kernel intentionally ships no `recipes/` tree. SIM recipes are runnable
source in a codec (for example `setup.siml`) with a `requires` list of libraries
such as `core` and `codec/lisp`. The kernel defines those contracts but supplies
none of that behavior and takes no library dependency (it is the root of the
dependency graph), so it cannot load a codec or evaluate SIM source. Recipe
books that exercise kernel contracts therefore live in the library crates that
can load codecs and run the standard distribution, not here.
