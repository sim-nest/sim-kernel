use std::sync::Mutex;

use crate::{
    env::Cx,
    error::{Error, Result},
    eval::Demand,
    expr::Expr,
    id::{CORE_THUNK_CLASS_ID, Symbol},
    object::Object,
    value::Value,
};

/// A delayed computation that produces a value when forced.
///
/// Thunks are how lazy and call-by-need eval policies defer argument
/// evaluation: the value is computed only when [`force`](Thunk::force) is
/// called to satisfy a [`Demand`]. The kernel defines the contract and two
/// reference objects ([`LazyThunkObject`] and [`ThunkObject`]); libraries may
/// supply their own.
pub trait Thunk: Send + Sync {
    /// Forces the delayed computation, returning its value.
    fn force(&self, cx: &mut Cx, demand: Demand) -> Result<Value>;
}

enum ThunkState {
    Pending { input: Expr, env: crate::env::Env },
    Forcing,
    Forced(Value),
}

struct LazyThunkState {
    input: Expr,
    env: crate::env::Env,
    forcing: bool,
}

// sim-non-citizen(reason = "live delayed-evaluation handle; reconstruct the source expression and environment separately", kind = "handle", descriptor = "core/Expr")
/// A non-memoizing thunk that re-evaluates its expression on every force.
///
/// Used by call-by-name style policies: forcing always re-runs the captured
/// expression in its captured environment and never caches the result.
pub struct LazyThunkObject {
    state: Mutex<LazyThunkState>,
}

impl LazyThunkObject {
    /// Creates a thunk capturing `input` and the environment `env`.
    pub fn new(input: Expr, env: crate::env::Env) -> Self {
        Self {
            state: Mutex::new(LazyThunkState {
                input,
                env,
                forcing: false,
            }),
        }
    }
}

impl Object for LazyThunkObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<lazy-thunk>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for LazyThunkObject {
    fn class(&self, cx: &mut Cx) -> Result<Value> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Thunk"))
        {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(CORE_THUNK_CLASS_ID, Symbol::qualified("core", "Thunk"))
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("lazy thunk state"))?;
        if state.forcing {
            return Err(Error::RecursiveThunkForce);
        }
        Ok(state.input.clone())
    }
    fn as_thunk(&self) -> Option<&dyn Thunk> {
        Some(self)
    }
}

impl Thunk for LazyThunkObject {
    fn force(&self, cx: &mut Cx, _demand: Demand) -> Result<Value> {
        let (input, env) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("lazy thunk state"))?;
            if state.forcing {
                return Err(Error::RecursiveThunkForce);
            }
            state.forcing = true;
            (state.input.clone(), state.env.clone())
        };
        let evaluated = eval_input_with_env(cx, input, env);

        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("lazy thunk state"))?;
        state.forcing = false;
        evaluated
    }
}

// sim-non-citizen(reason = "live delayed-evaluation handle; reconstruct the source expression and environment separately", kind = "handle", descriptor = "core/Expr")
/// A memoizing call-by-need thunk: evaluated at most once, then cached.
///
/// Used by [`NeedPolicy`](crate::NeedPolicy): the first successful force
/// records the value for all later forces; a failed force leaves the thunk
/// pending so it can be retried.
pub struct ThunkObject {
    state: Mutex<ThunkState>,
}

impl ThunkObject {
    /// Creates a pending thunk capturing `input` and the environment `env`.
    pub fn new(input: Expr, env: crate::env::Env) -> Self {
        Self {
            state: Mutex::new(ThunkState::Pending { input, env }),
        }
    }
}

impl Object for ThunkObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<thunk>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::ObjectCompat for ThunkObject {
    fn class(&self, cx: &mut Cx) -> Result<Value> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Thunk"))
        {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(CORE_THUNK_CLASS_ID, Symbol::qualified("core", "Thunk"))
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("thunk state"))?;
        match &*state {
            ThunkState::Pending { input, .. } => Ok(input.clone()),
            ThunkState::Forcing => Err(Error::RecursiveThunkForce),
            ThunkState::Forced(value) => value.object().as_expr(cx),
        }
    }
    fn as_thunk(&self) -> Option<&dyn Thunk> {
        Some(self)
    }
}

impl Thunk for ThunkObject {
    fn force(&self, cx: &mut Cx, _demand: Demand) -> Result<Value> {
        let (input, env) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("thunk state"))?;
            match &*state {
                ThunkState::Forced(value) => return Ok(value.clone()),
                ThunkState::Forcing => return Err(Error::RecursiveThunkForce),
                ThunkState::Pending { input, env } => {
                    let pair = (input.clone(), env.clone());
                    *state = ThunkState::Forcing;
                    pair
                }
            }
        };
        let evaluated = eval_input_with_env(cx, input.clone(), env.clone());

        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("thunk state"))?;
        match evaluated {
            Ok(value) => {
                *state = ThunkState::Forced(value.clone());
                Ok(value)
            }
            Err(err) => {
                *state = ThunkState::Pending { input, env };
                Err(err)
            }
        }
    }
}

fn eval_input_with_env(cx: &mut Cx, input: Expr, env: crate::env::Env) -> Result<Value> {
    cx.with_env(env, |cx| crate::eval::eval_expr_default(cx, input))
}
