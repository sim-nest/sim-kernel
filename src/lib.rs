//! sim-kernel is the small protocol kernel of the SIM Rust runtime.
//!
//! The kernel defines contracts; libraries supply behavior. This crate holds
//! the protocol and contract types -- forms, values, stores, registry and
//! callable surfaces, the [`shape`] protocol, evaluation policy, and the ABI
//! transport -- while concrete behavior (parsers, number domains, the standard
//! distribution, and server/agent/browse implementations) lives in separate
//! library crates loaded against these contracts. SIM is a Rust runtime, not a
//! Lisp runtime; Lisp is one codec surface among several.
//!
//! The one deliberate exception is the pair of built-in *reference backends*:
//! [`list::VecList`] (a [`list::ListValue`]) and [`table::AssocTable`] (a
//! [`table::Table`]). These are the kernel's bootstrap list and table
//! implementations -- minimal, correct, and self-annotated `sim-non-citizen` --
//! so the runtime can stand up collections before any library loads. They are
//! reference implementations, not the only backends: alternatives load as libs
//! against the [`list::ListBackend`]/[`table::TableBackend`] contracts. (Future
//! cleanup: move even these into a default-loaded distribution lib so the kernel
//! ships pure contracts; tracked against the constellation reorg.)
//!
//! The kernel data flow is:
//!
//! ```text
//! tokens -> checked forms -> objects -> checked calls -> objects -> encoded forms
//! ```
//!
//! # Module map
//!
//! Core forms and values: [`value`], [`expr`], [`term`], [`encode`],
//! [`error`], [`id`], [`ref_id`].
//!
//! Data substrate: [`datum`], [`datum_store`], [`fact_store`], [`claim`],
//! [`card`], [`catalog`], [`handle_store`], [`hint`], [`ref_resolver`].
//!
//! Registry and behavior contracts: [`library`], [`op`], [`capability`],
//! [`mod@env`], [`standard`], [`factory`].
//!
//! Callable, object, control, ledgers: [`callable`], [`class`], [`object`],
//! [`control`], [`event`], [`event_ledger`], [`effect`], [`effect_ledger`],
//! [`rank`].
//!
//! Shape protocol: [`shape`], [`shape_report`], [`pratt`].
//!
//! Evaluation: [`eval`], [`realize`].
//!
//! ABI, stream transport, collections: [`native_abi`], [`stream`],
//! [`stream_surface`], [`list`], [`seq`], [`table`], [`number_domain`].
#![forbid(unsafe_code)]
#![deny(missing_docs)]
pub mod callable;
pub mod capability;
pub mod card;
pub mod catalog;
pub mod claim;
pub mod class;
pub mod control;
pub mod datum;
pub mod datum_store;
pub mod effect;
pub mod effect_ledger;
pub mod encode;
pub mod env;
pub mod error;
pub mod eval;
pub mod event;
pub mod event_ledger;
#[cfg(test)]
mod event_tests;
pub mod expr;
pub mod fact_store;
#[cfg(test)]
mod fact_store_tests;
pub mod factory;
pub mod handle_store;
pub mod hint;
#[cfg(test)]
mod hint_tests;
pub mod id;
pub mod library;
pub mod list;
pub mod native_abi;
pub mod number_domain;
pub mod object;
pub mod op;
#[cfg(test)]
mod op_adapter_tests;
mod op_adapters;
#[cfg(test)]
mod op_tests;
pub mod pratt;
pub mod rank;
pub mod realize;
#[cfg(test)]
mod realize_tests;
pub mod ref_id;
pub mod ref_resolver;
pub mod seq;
#[cfg(test)]
mod seq_tests;
pub mod shape;
pub mod shape_report;
pub mod standard;
pub mod stream;
pub mod stream_surface;
pub mod table;
pub mod term;
#[cfg(test)]
mod term_tests;
pub mod testing;
pub mod value;
pub use callable::Callable;
pub use capability::{
    CapabilityName, CapabilitySet, GrantSeat, ReadPolicy, TrustLevel, browse_internal_capability,
    browse_read_capability, browse_run_tests_capability, config_list_impl_capability,
    config_table_impl_capability, eval_fabric_capability, eval_remote_capability,
    fact_private_capability, list_force_unbounded_capability, logic_consult_file_capability,
    logic_db_write_capability, logic_tool_call_capability, macro_expand_capability,
    macro_expand_compile_capability, macro_expand_eval_capability, macro_expand_read_capability,
    native_dynamic_load_capability, read_construct_capability, read_eval_capability,
    registry_catalog_read_capability, table_remote_capability,
};
pub use claim::{Claim, ClaimKind, ClaimPattern, Visibility};
pub use class::{Class, class_is_subclass_of};
pub use datum::{
    DATUM_CONTENT_ALGORITHM_NAME, DATUM_CONTENT_ALGORITHM_NAMESPACE, Datum, datum_content_algorithm,
};
pub use datum_store::{BTreeDatumStore, DatumStore};
pub use effect::{Effect, effect_abort_op_key, effect_resume_op_key, effect_test_run_kind};
pub use encode::{
    CanonicalPolicy, ConstructorSurface, EncodeOptions, EncodePosition, ObjectEncode,
    ObjectEncoding, ReadConstructEncodePolicy, ReadConstructor, ReadEvalEncodePolicy, WriteCx,
};
pub use env::{Capabilities, Cx, Diagnostics, Env};
pub use error::{Diagnostic, Error, Result, Severity};
pub use eval::{
    Consistency, Demand, EagerPolicy, EvalFabric, EvalFabricRef, EvalMode, EvalPolicy,
    EvalPolicyRef, EvalReply, EvalRequest, HybridPolicy, LazyPolicy, LazyThunkObject,
    MacroExpander, MacroExpanderRef, NeedPolicy, NoopEvalPolicy, Phase, PreparedArgs,
    StrictByShapePolicy, StrictNames, Thunk, ThunkObject, eval_expr_default, force_default,
};
pub use event::{Event, EventKind, EventSource, Tick, validate_ticks};
pub use event_ledger::EventLedger;
pub use expr::{
    CanonicalKey, Expr, LocatedExpr, LocatedExprTree, NumberLiteral, Origin, QuoteMode, SourceId,
    SourceRegistry, Span, Trivia,
};
pub use fact_store::{BTreeFactStore, FactStore};
pub use factory::{DefaultFactory, Factory};
pub use handle_store::{BTreeHandleStore, HandleStore};
pub use hint::HintMetadata;
#[rustfmt::skip]
pub use id::{CORE_BOOL_CLASS_ID, CORE_BYTES_CLASS_ID, CORE_CARD_CLASS_ID, CORE_CLASS_CLASS_ID, CORE_CODEC_CLASS_ID, CORE_EVAL_REPLY_CLASS_ID, CORE_EVAL_REQUEST_CLASS_ID, CORE_EXPR_CLASS_ID, CORE_FUNCTION_CLASS_ID, CORE_HELP_CLASS_ID, CORE_LIST_CLASS_ID, CORE_LOCAL_EVAL_FABRIC_CLASS_ID, CORE_MACRO_CLASS_ID, CORE_NIL_CLASS_ID, CORE_NUMBER_CLASS_ID, CORE_NUMBER_DOMAIN_CLASS_ID, CORE_SEQUENCE_CLASS_ID, CORE_SHAPE_CLASS_ID, CORE_SHAPE_MATCH_CLASS_ID, CORE_STRING_CLASS_ID, CORE_SYMBOL_CLASS_ID, CORE_TABLE_CLASS_ID, CORE_TEST_CLASS_ID, CORE_THUNK_CLASS_ID, CaseId, ClassId, CodecId, FunctionId, LibId, MacroId, NumberDomainId, RuntimeId, ShapeId, SiteId, Symbol, ValueId};
pub use library::{
    AbiVersion, CatalogSource, Dependency, Export, ExportKind, ExportRecord, ExportState, Lib,
    LibBootDependency, LibBootReceipt, LibLoader, LibManifest, LibSource, LibSourceSpec, LibTarget,
    Linker, LoadCx, LoadedLib, LoaderRegistry, Registry, RegistryBootState, Test, TestReport,
    Version,
};
pub use list::{
    DEFAULT_FORCE_BOUND, LengthResult, ListBackend, ListRegistry, ListValue, VecList,
    force_list_bound, force_list_to_vec, spine_len_cmp,
};
pub use native_abi::{
    NATIVE_DYLIB_ENTRYPOINT_V1, NATIVE_LIB_ABI_V1_MAJOR, NATIVE_LIB_ABI_V1_MINOR,
    NativeAbiBorrowedBytes, NativeAbiCall, NativeAbiCallResponse, NativeAbiDestroyBytes,
    NativeAbiDestroyError, NativeAbiDestroyInstance, NativeAbiError, NativeAbiInstantiate,
    NativeAbiManifest, NativeAbiOwnedBytes, NativeLibAbiHeaderV1, NativeLibAbiV1,
    native_abi_owned_bytes,
};
pub use number_domain::{
    NumberBinaryOp, NumberDomain, NumberReductionOp, NumberUnaryOp, NumberValue, NumberValueRef,
    PromotionRule, PromotionSearchLimits, ValueNumberBinaryOp, ValueNumberReductionOp,
    ValueNumberUnaryOp, ValuePromotionRule,
};
pub use object::{
    Args, ClaimSink, ClassRef, Object, ObjectCompat, ObjectHeader, RawArgs, ReadConstructorRef,
    ShapeRef, TableRef, default_object_header, is_default_object_header,
};
#[rustfmt::skip]
pub use op::{Op, OpSpec, ResolvedOp, Step, check_shape_if_available, core_any_ref, core_call_op_key, core_class_symbol_op_key, core_dir_is_dir_op_key, core_expr_snapshot_op_key, core_force_op_key, core_list_items_op_key, core_number_domain_symbol_op_key, core_number_value_op_key, core_object_encoding_op_key, core_read_construct_op_key, core_realize_start_op_key, core_seq_close_op_key, core_seq_next_op_key, core_shape_check_term_op_key, core_shape_check_value_op_key, core_shape_describe_op_key, core_table_entries_op_key, invoke_op, resolve_op};
pub use pratt::{
    Fixity, PrattOperator, PrattResult, PrattTable, PrattTableObject, Token as PrattToken,
    parse_symbol as parse_pratt_symbol, pratt_table_value,
};
pub use realize::{
    BufferedEventSource, ObserveMode, RealizeRequest, drain_events_to_reply, realize_events,
    realize_final,
};
pub use ref_id::{ContentId, Coordinate, HandleId, Ref};
pub use ref_resolver::{
    RefResolver, ResolvedRef, TemporaryRefResolver, value_from_datum, value_from_ref,
};
#[rustfmt::skip]
pub use seq::{EventSourceSequence, ListSequence, Sequence, SequenceItem, seq_close, seq_close_value, seq_is_done, seq_next, seq_next_value, seq_peek, sequence_item_from_event, sequence_item_value};
pub use shape::{
    ExprKind, MatchScore, Shape, ShapeBindings, ShapeCallTarget, ShapeDoc, ShapeMatch,
    ShapeMatchObject, call_shape, shape_is_subshape_of, shape_match_value,
};
pub use stream::Stream;
pub use table::{AssocTable, Dir, Table, TableBackend, TableRegistry};
pub use term::{OpKey, Term};
pub use value::{RuntimeObject, Value};
