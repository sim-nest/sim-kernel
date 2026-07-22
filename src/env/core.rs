use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::{
    ContentId,
    capability::{
        CapabilityName, CapabilitySet, GrantSeat, macro_expansion_capability_for_phase,
        read_construct_capability,
    },
    control::{ControlPolicy, ControlPolicyRef, NoopControlPolicy},
    datum_store::BTreeDatumStore,
    effect_ledger::EffectLedger,
    error::{Diagnostic, Error, Result},
    eval::{EvalPolicy, EvalPolicyRef, MacroExpanderRef, Phase},
    expr::{Expr, SourceRegistry},
    fact_store::{BTreeFactStore, FactStore},
    factory::Factory,
    handle_store::BTreeHandleStore,
    id::{LibId, Symbol},
    library::{LoadCx, Registry},
    list::ListRegistry,
    number_domain::PromotionSearchLimits,
    object::Args,
    table::TableRegistry,
    value::Value,
};

use super::{Diagnostics, Env, load_ledger::LibLoadLedger};

/// Monotonic source of per-context grant ids. Only used to bind a [`GrantSeat`]
/// to the context it was minted with; the value is never observable in output,
/// so it does not affect evaluation determinism.
static NEXT_GRANT_ID: AtomicU64 = AtomicU64::new(1);

fn unknown_symbol(symbol: &Symbol) -> Error {
    Error::UnknownSymbol {
        symbol: symbol.clone(),
    }
}

fn class_protocol(value: &Value) -> Result<&dyn crate::Class> {
    value.object().as_class().ok_or(Error::TypeMismatch {
        expected: "class",
        found: "non-class",
    })
}

fn read_constructor_protocol(value: &Value) -> Result<&dyn crate::ReadConstructor> {
    value
        .object()
        .as_read_constructor()
        .ok_or(Error::TypeMismatch {
            expected: "read-constructor",
            found: "non-read-constructor",
        })
}

/// The capability state of a [`Cx`]; an alias for [`CapabilitySet`].
pub type Capabilities = CapabilitySet;

/// The evaluation context threaded through every checked call.
///
/// `Cx` bundles the registry handle, factory, capability set, eval and control
/// policies, the data substrate (datum/fact/handle stores and ledgers), and the
/// diagnostic sink. The kernel defines this context; libraries supply the
/// behavior reached through it (registered classes, functions, number domains,
/// list/table backends, and so on). See the README sections "Library system"
/// and "Capabilities and trust".
pub struct Cx {
    /// Per-context identity that binds a [`GrantSeat`] to exactly this context.
    /// Minted at construction; a seat can only grant into the context it shares
    /// this id with, so minting a fresh `(Cx, GrantSeat)` pair cannot be used to
    /// escalate a different context.
    grant_id: u64,
    env: Env,
    diagnostics: Diagnostics,
    capabilities: Capabilities,
    eval_policy: EvalPolicyRef,
    macro_expander: Option<MacroExpanderRef>,
    factory: Arc<dyn Factory>,
    pub(crate) registry: Registry,
    list_registry: ListRegistry,
    table_registry: TableRegistry,
    promotion_search_limits: PromotionSearchLimits,
    sources: SourceRegistry,
    datum_store: BTreeDatumStore,
    handles: BTreeHandleStore,
    facts: BTreeFactStore,
    lib_load_ledger: LibLoadLedger,
    effect_ledger: EffectLedger,
    control_policy: ControlPolicyRef,
}

impl Cx {
    /// Builds a fresh context with the given eval policy and factory.
    ///
    /// The registry, capability set, and stores start empty (boot claims aside);
    /// libraries register behavior into the returned context.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::sync::Arc;
    /// # use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    /// let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    /// assert!(cx.capabilities().iter().next().is_none());
    /// ```
    pub fn new(eval_policy: EvalPolicyRef, factory: Arc<dyn Factory>) -> Self {
        let mut datum_store = BTreeDatumStore::default();
        let mut facts = BTreeFactStore::default();
        facts.insert_boot_claims(&mut datum_store);

        Self {
            grant_id: NEXT_GRANT_ID.fetch_add(1, Ordering::Relaxed),
            env: Env::default(),
            diagnostics: Diagnostics::default(),
            capabilities: Capabilities::default(),
            eval_policy,
            macro_expander: None,
            factory,
            registry: Registry::default(),
            list_registry: ListRegistry::default(),
            table_registry: TableRegistry::default(),
            promotion_search_limits: PromotionSearchLimits::default(),
            sources: SourceRegistry::default(),
            datum_store,
            handles: BTreeHandleStore::default(),
            facts,
            lib_load_ledger: LibLoadLedger::default(),
            effect_ledger: EffectLedger::default(),
            control_policy: Arc::new(NoopControlPolicy),
        }
    }

    /// Returns the active lexical environment.
    pub fn env(&self) -> &Env {
        &self.env
    }

    /// Returns the active lexical environment mutably.
    pub fn env_mut(&mut self) -> &mut Env {
        &mut self.env
    }

    /// Runs `f` with `env` installed as the active environment, then restores it.
    pub fn with_env<T>(&mut self, env: Env, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let saved = std::mem::replace(&mut self.env, env);
        let result = f(self);
        self.env = saved;
        result
    }

    /// Returns the active object [`Factory`].
    pub fn factory(&self) -> &dyn Factory {
        self.factory.as_ref()
    }

    /// Returns a shared handle to the active object factory.
    pub fn factory_ref(&self) -> Arc<dyn Factory> {
        self.factory.clone()
    }

    /// Runs `f` with `factory` installed as the active factory, then restores it.
    pub fn with_factory<T>(
        &mut self,
        factory: Arc<dyn Factory>,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let saved = std::mem::replace(&mut self.factory, factory);
        let result = f(self);
        self.factory = saved;
        result
    }

    /// Returns the behavior [`Registry`].
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Returns the behavior registry mutably, for registering exports.
    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    /// Returns the registered list backend.
    pub fn list_registry(&self) -> &ListRegistry {
        &self.list_registry
    }

    /// Returns the list backend mutably.
    pub fn list_registry_mut(&mut self) -> &mut ListRegistry {
        &mut self.list_registry
    }

    /// Returns the registered table backend.
    pub fn table_registry(&self) -> &TableRegistry {
        &self.table_registry
    }

    /// Returns the table backend mutably.
    pub fn table_registry_mut(&mut self) -> &mut TableRegistry {
        &mut self.table_registry
    }

    /// Runs `f` with `registry` installed as the active registry, then restores it.
    pub fn with_registry<T>(
        &mut self,
        registry: Registry,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let saved = std::mem::replace(&mut self.registry, registry);
        let result = f(self);
        self.registry = saved;
        result
    }

    /// Builds a list value through the registered list backend.
    pub fn new_list(&mut self, items: Vec<Value>) -> Result<Value> {
        let registry = std::mem::take(&mut self.list_registry);
        let result = registry.new_list(self, items);
        self.list_registry = registry;
        result
    }

    /// Builds a cons cell through the registered list backend.
    pub fn new_cons(&mut self, car: Value, cdr: Value) -> Result<Value> {
        let registry = std::mem::take(&mut self.list_registry);
        let result = registry.new_cons(self, car, cdr);
        self.list_registry = registry;
        result
    }

    /// Builds a table value through the registered table backend.
    pub fn new_table(&mut self, entries: Vec<(Symbol, Value)>) -> Result<Value> {
        let registry = std::mem::take(&mut self.table_registry);
        let result = registry.new_table(self, entries);
        self.table_registry = registry;
        result
    }

    /// Returns the source registry.
    pub fn sources(&self) -> &SourceRegistry {
        &self.sources
    }

    /// Returns the source registry mutably.
    pub fn sources_mut(&mut self) -> &mut SourceRegistry {
        &mut self.sources
    }

    /// Returns the datum store.
    pub fn datum_store(&self) -> &BTreeDatumStore {
        &self.datum_store
    }

    /// Returns the datum store mutably.
    pub fn datum_store_mut(&mut self) -> &mut BTreeDatumStore {
        &mut self.datum_store
    }

    /// Returns the handle store.
    pub fn handles(&self) -> &BTreeHandleStore {
        &self.handles
    }

    /// Returns the handle store mutably.
    pub fn handles_mut(&mut self) -> &mut BTreeHandleStore {
        &mut self.handles
    }

    /// Returns the fact store.
    pub fn facts(&self) -> &BTreeFactStore {
        &self.facts
    }

    /// Returns the fact store mutably.
    pub fn facts_mut(&mut self) -> &mut BTreeFactStore {
        &mut self.facts
    }

    /// Returns the effect ledger.
    pub fn effect_ledger(&self) -> &EffectLedger {
        &self.effect_ledger
    }

    /// Returns the effect ledger mutably.
    pub fn effect_ledger_mut(&mut self) -> &mut EffectLedger {
        &mut self.effect_ledger
    }

    /// Returns the active control policy.
    pub fn control_policy(&self) -> &dyn ControlPolicy {
        self.control_policy.as_ref()
    }

    /// Returns a shared handle to the active control policy.
    pub fn control_policy_ref(&self) -> ControlPolicyRef {
        self.control_policy.clone()
    }

    /// Returns the name of the active control policy.
    pub fn control_policy_name(&self) -> &'static str {
        self.control_policy.name()
    }

    /// Replaces the active control policy.
    pub fn set_control_policy(&mut self, control_policy: ControlPolicyRef) {
        self.control_policy = control_policy;
    }

    pub(crate) fn with_effect_ledger<T>(
        &mut self,
        f: impl FnOnce(&mut Self, &mut EffectLedger) -> Result<T>,
    ) -> Result<T> {
        let mut ledger = std::mem::take(&mut self.effect_ledger);
        let result = f(self, &mut ledger);
        self.effect_ledger = ledger;
        result
    }

    /// Inserts a claim into the fact store, subject to capability authorization.
    pub fn insert_fact(&mut self, claim: crate::Claim) -> Result<crate::Ref> {
        self.facts
            .insert_authorized(&self.capabilities, &mut self.datum_store, claim)
    }

    /// Inserts a claim and records it as part of a loaded library's receipt.
    ///
    /// When the claim did not already exist, `unload_lib` retracts it with the
    /// rest of `lib_id`'s recorded load effects. Pre-existing identical claims
    /// are left owned by their original publisher.
    pub fn insert_fact_for_lib(
        &mut self,
        lib_id: LibId,
        claim: crate::Claim,
    ) -> Result<crate::Ref> {
        let (reference, inserted) = self.insert_recorded_fact(claim)?;
        if let Some(inserted) = inserted {
            self.record_load_claims(lib_id, vec![inserted]);
        }
        Ok(reference)
    }

    pub(crate) fn insert_recorded_fact(
        &mut self,
        claim: crate::Claim,
    ) -> Result<(crate::Ref, Option<ContentId>)> {
        let id = claim.content_id(&mut self.datum_store)?;
        let existed = self.facts.get(&id).is_some();
        let inserted = self.insert_fact(claim)?;
        let crate::Ref::Content(inserted_id) = inserted else {
            return Err(Error::Lib(
                "fact insertion returned a non-content reference".to_owned(),
            ));
        };
        debug_assert_eq!(inserted_id, id);
        Ok((
            crate::Ref::Content(inserted_id.clone()),
            (!existed).then_some(inserted_id),
        ))
    }

    /// Queries the fact store for claims matching `pattern`, applying read policy.
    pub fn query_facts(&self, pattern: crate::ClaimPattern) -> Result<Vec<crate::Claim>> {
        self.facts.query_authorized(self, pattern)
    }

    pub(crate) fn record_load_claims(&mut self, lib_id: LibId, claim_ids: Vec<ContentId>) {
        self.lib_load_ledger.record_claims(lib_id, claim_ids);
    }

    pub(crate) fn remove_load_claims(&mut self, lib_ids: &[LibId]) {
        self.lib_load_ledger.remove_claims(lib_ids, &mut self.facts);
    }

    /// Returns the limits bounding number-domain promotion search.
    pub fn promotion_search_limits(&self) -> PromotionSearchLimits {
        self.promotion_search_limits
    }

    /// Sets the limits bounding number-domain promotion search.
    pub fn set_promotion_search_limits(&mut self, limits: PromotionSearchLimits) {
        self.promotion_search_limits = limits;
    }

    pub(crate) fn load_cx(&self) -> LoadCx {
        LoadCx::new(
            self.capabilities.clone(),
            self.factory.clone(),
            self.registry.clone(),
        )
    }

    /// Returns the active evaluation policy.
    pub fn eval_policy(&self) -> &dyn EvalPolicy {
        self.eval_policy.as_ref()
    }

    /// Returns a shared handle to the active evaluation policy.
    pub fn eval_policy_ref(&self) -> EvalPolicyRef {
        self.eval_policy.clone()
    }

    /// Returns the name of the active evaluation policy.
    pub fn eval_policy_name(&self) -> &'static str {
        self.eval_policy.name()
    }

    /// Replaces the active evaluation policy.
    pub fn set_eval_policy(&mut self, eval_policy: EvalPolicyRef) {
        self.eval_policy = eval_policy;
    }

    /// Installs a macro expander.
    pub fn set_macro_expander(&mut self, macro_expander: MacroExpanderRef) {
        self.macro_expander = Some(macro_expander);
    }

    /// Removes any installed macro expander.
    pub fn clear_macro_expander(&mut self) {
        self.macro_expander = None;
    }

    /// Returns the installed macro expander, if any.
    pub fn macro_expander_ref(&self) -> Option<MacroExpanderRef> {
        self.macro_expander.clone()
    }

    /// Expands macros in `expr` for the given phase, or returns it unchanged.
    pub fn expand_macros(&mut self, phase: Phase, expr: Expr) -> Result<Expr> {
        let Some(expander) = self.macro_expander.clone() else {
            return Ok(expr);
        };
        let eval_policy = self.eval_policy_ref();
        if !eval_policy.allow_macro_expansion(phase) {
            return Err(Error::Eval(format!(
                "macro expansion denied by eval policy {} for {phase:?}",
                eval_policy.name()
            )));
        }
        self.require(&macro_expansion_capability_for_phase(phase))?;
        expander.expand_expr(self, phase, expr)
    }

    /// Returns the accumulated diagnostics.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Drains and returns the accumulated diagnostics.
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.take()
    }

    /// Records an already-built diagnostic.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push_diagnostic(diagnostic);
    }

    /// Records an info-level diagnostic from a message.
    pub fn push_info(&mut self, message: impl Into<String>) {
        self.diagnostics.push_info(message);
    }

    /// Constructs a context together with its host [`GrantSeat`].
    ///
    /// The caller (a bootloader, a server session, a loader) keeps the seat and
    /// grants capabilities through it. The seat is never handed to a callable, so
    /// loaded behavior cannot grant itself a capability. Use this instead of
    /// [`Cx::new`] wherever the host needs to grant capabilities.
    pub fn new_seated(eval_policy: EvalPolicyRef, factory: Arc<dyn Factory>) -> (Self, GrantSeat) {
        let cx = Self::new(eval_policy, factory);
        let seat = GrantSeat::for_cx(cx.grant_id);
        (cx, seat)
    }

    /// The identity that binds a [`GrantSeat`] to this context.
    pub(crate) fn grant_id(&self) -> u64 {
        self.grant_id
    }

    /// Grants a capability into this context under host authority.
    ///
    /// This is the single internal insert point; the only ways to reach it are a
    /// host-held [`GrantSeat`] (production) or the `test-support` grant methods
    /// (tests). A loaded callable holds a `&mut Cx` but no seat, so it cannot
    /// grant itself a capability.
    pub(crate) fn grant_from_host(&mut self, capability: CapabilityName) {
        self.capabilities.insert(capability);
    }

    /// Grants a capability to this context. TEST-ONLY: available only under the
    /// `test-support` feature. Production code grants through a host [`GrantSeat`]
    /// ([`Cx::new_seated`]); a loaded callable has no grant path at all.
    #[cfg(feature = "test-support")]
    pub fn grant(&mut self, capability: CapabilityName) {
        self.grant_from_host(capability);
    }

    /// Grants a capability named by a static string. TEST-ONLY: available only
    /// under the `test-support` feature.
    #[cfg(feature = "test-support")]
    pub fn grant_named(&mut self, capability: &'static str) {
        self.grant_from_host(CapabilityName::new(capability));
    }

    /// Returns the granted capability set.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Runs `f` with `capabilities` installed, then restores the prior set.
    pub fn with_capabilities<T>(
        &mut self,
        capabilities: Capabilities,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let saved = std::mem::replace(&mut self.capabilities, capabilities);
        let result = f(self);
        self.capabilities = saved;
        result
    }

    /// Resolves a registered class by symbol.
    pub fn resolve_class(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.class_by_symbol(symbol),
            Error::UnknownClass {
                class: symbol.clone(),
            },
        )
    }

    /// Resolves a registered function by symbol.
    pub fn resolve_function(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.function_by_symbol(symbol),
            Error::UnknownFunction {
                function: symbol.clone(),
            },
        )
    }

    /// Resolves a registered macro by symbol.
    pub fn resolve_macro(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.macro_by_symbol(symbol),
            unknown_symbol(symbol),
        )
    }

    /// Resolves a registered shape by symbol.
    pub fn resolve_shape(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.shape_by_symbol(symbol),
            unknown_symbol(symbol),
        )
    }

    /// Resolves a registered codec by symbol.
    pub fn resolve_codec(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.codec_by_symbol(symbol),
            unknown_symbol(symbol),
        )
    }

    /// Resolves a registered number domain by symbol.
    pub fn resolve_number_domain(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.number_domain_by_symbol(symbol),
            unknown_symbol(symbol),
        )
    }

    /// Resolves a registered value binding by symbol.
    pub fn resolve_value(&self, symbol: &Symbol) -> Result<Value> {
        self.resolve_registered_value(
            |registry| registry.value_by_symbol(symbol),
            unknown_symbol(symbol),
        )
    }

    fn resolve_registered_value(
        &self,
        lookup: impl FnOnce(&Registry) -> Option<&Value>,
        not_found: Error,
    ) -> Result<Value> {
        lookup(self.registry()).cloned().ok_or(not_found)
    }

    /// Calls a callable value with already-evaluated arguments.
    pub fn call_value(&mut self, value: Value, args: Args) -> Result<Value> {
        let Some(callable) = value.object().as_callable() else {
            return Err(Error::TypeMismatch {
                expected: "callable",
                found: "non-callable",
            });
        };
        callable.call(self, args)
    }

    /// Calls a callable value with raw, unevaluated argument expressions.
    pub fn call_exprs(&mut self, value: Value, args: Vec<crate::expr::Expr>) -> Result<Value> {
        let Some(callable) = value.object().as_callable() else {
            return Err(Error::TypeMismatch {
                expected: "callable",
                found: "non-callable",
            });
        };
        callable.call_exprs(self, crate::object::RawArgs::new(args))
    }

    /// Resolves a function by symbol and calls it.
    pub fn call_function(&mut self, symbol: &Symbol, args: Args) -> Result<Value> {
        let function = self.resolve_function(symbol)?;
        self.call_value(function, args)
    }

    /// Resolves a class by symbol and calls its constructor.
    pub fn call_class(&mut self, symbol: &Symbol, args: Args) -> Result<Value> {
        let class = self.resolve_class(symbol)?;
        self.call_value(class, args)
    }

    /// Constructs an instance of `class` from read-time arguments.
    ///
    /// Requires the [`read_construct_capability`](crate::capability::read_construct_capability).
    pub fn read_construct(&mut self, class: &Symbol, args: Vec<Value>) -> Result<Value> {
        self.require(&read_construct_capability())?;
        let class_value = self.resolve_class(class)?;
        let constructor = class_protocol(&class_value)?
            .read_constructor(self)?
            .ok_or_else(|| Error::Eval(format!("class {class} has no read constructor")))?;
        read_constructor_protocol(&constructor)?.construct_read(self, args)
    }

    /// Forces a value to the requested demand through the active eval policy.
    pub fn force(&mut self, value: Value, demand: crate::eval::Demand) -> Result<Value> {
        let eval_policy = self.eval_policy.clone();
        eval_policy.force(self, value, demand)
    }

    /// Evaluates an expression through the active eval policy.
    pub fn eval_expr(&mut self, expr: crate::expr::Expr) -> Result<Value> {
        let eval_policy = self.eval_policy.clone();
        eval_policy.eval_expr(self, expr)
    }

    /// Returns true when `name` resolves across environment or registry layers.
    pub fn symbol_is_bound(&mut self, name: &Symbol) -> bool {
        self.env().get(name).is_some()
            || self.resolve_function(name).is_ok()
            || self.resolve_class(name).is_ok()
            || self.resolve_shape(name).is_ok()
            || self.resolve_value(name).is_ok()
    }

    /// Resolves an unbound value-position symbol through the active eval policy.
    pub fn resolve_unbound_symbol(&mut self, symbol: Symbol) -> Result<Value> {
        let eval_policy = self.eval_policy_ref();
        eval_policy.resolve_unbound_symbol(self, symbol)
    }

    /// Resolves an unbound call through the active eval policy.
    pub fn resolve_unbound_call(
        &mut self,
        operator: Symbol,
        args: Vec<crate::expr::Expr>,
    ) -> Result<Value> {
        let eval_policy = self.eval_policy_ref();
        eval_policy.resolve_unbound_call(self, operator, args)
    }

    /// Demands a capability, returning [`Error::CapabilityDenied`] when absent.
    pub fn require(&self, capability: &CapabilityName) -> Result<()> {
        if self.capabilities.contains(capability) {
            Ok(())
        } else {
            Err(Error::CapabilityDenied {
                capability: capability.clone(),
            })
        }
    }

    /// Demands every capability in turn, failing on the first absent one.
    pub fn require_all(&self, capabilities: &[CapabilityName]) -> Result<()> {
        for capability in capabilities {
            self.require(capability)?;
        }
        Ok(())
    }
}
