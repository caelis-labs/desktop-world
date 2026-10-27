//! One app-bound execution context, with a single live observation.
//!
//! This module owns grants, reference lifetime and admission. Trusted embedding
//! code creates grants; this API is not yet exposed to agent-controlled input.

use crate::computer::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, MutexGuard,
};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

/// Immutable authority supplied by a trusted Harness integration.
/// Discovery must prove `app`; constructing this value does not prove OS identity.
pub struct Grant {
    app: AppInstance,
    capabilities: Vec<Capability>,
    expires_at: Instant,
}

impl Grant {
    pub fn new(app: AppInstance, capabilities: Vec<Capability>, expires_at: Instant) -> Self {
        Self {
            app,
            capabilities,
            expires_at,
        }
    }
}

/// Context-level conclusion plus the provider's original evidence. A cancelled
/// context can be uncertain even when a late provider reply reports success.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionReceipt {
    pub outcome: ActionOutcome,
    pub provider_outcome: Option<ActionOutcome>,
    pub interruption: Option<Error>,
}

impl ActionReceipt {
    fn rejected(error: Error) -> Self {
        Self {
            outcome: ActionOutcome::rejected(error),
            provider_outcome: None,
            interruption: None,
        }
    }

    fn from_provider(provider_outcome: ActionOutcome, interruption: Option<Error>) -> Self {
        let outcome = match &interruption {
            Some(error) if provider_outcome.dispatch != DispatchState::NotDispatched => {
                ActionOutcome::unknown(error.clone())
            }
            _ => provider_outcome.clone(),
        };
        Self {
            outcome,
            provider_outcome: Some(provider_outcome),
            interruption,
        }
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub struct MonotonicClock;
impl Clock for MonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

struct LiveObservation<B> {
    reference: ObservationRef,
    provider: ProviderStamp,
    target: WindowTarget,
    expires_at: Instant,
    elements: Vec<Element>,
    native: B,
}

struct State<B> {
    busy: bool,
    cancelled: bool,
    closed: bool,
    generation: u64,
    observation: Option<LiveObservation<B>>,
}

pub struct Runtime<B: Backend, C: Clock = MonotonicClock> {
    // Rust drops fields in declaration order. Release native bindings before
    // shutting down the provider they may need for cleanup.
    state: Mutex<State<B::Binding>>,
    backend: B,
    grant: Grant,
    clock: C,
    observation_ttl: Duration,
    context: u64,
}

impl<B: Backend> Runtime<B> {
    pub fn new(backend: B, grant: Grant, observation_ttl: Duration) -> Self {
        Self::with_clock(backend, grant, observation_ttl, MonotonicClock)
    }
}

impl<B: Backend, C: Clock> Runtime<B, C> {
    pub fn with_clock(backend: B, grant: Grant, observation_ttl: Duration, clock: C) -> Self {
        let context = NEXT_CONTEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("execution context identity exhausted");
        Self {
            backend,
            grant,
            clock,
            observation_ttl,
            context,
            state: Mutex::new(State {
                busy: false,
                cancelled: false,
                closed: false,
                generation: 0,
                observation: None,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State<B::Binding>> {
        self.state.lock().expect("execution state poisoned")
    }

    fn authorize(
        &self,
        state: &State<B::Binding>,
        target: &WindowTarget,
        capability: Capability,
    ) -> Result<(), Error> {
        self.live_context(state)?;
        if self.grant.app != target.app {
            return Err(Error::WrongApp);
        }
        if !self.grant.capabilities.contains(&capability) {
            return Err(Error::CapabilityDenied);
        }
        if !self.backend.supports(capability) {
            return Err(Error::UnsupportedCapability);
        }
        Ok(())
    }

    fn live_context(&self, state: &State<B::Binding>) -> Result<(), Error> {
        if state.closed {
            return Err(Error::Closed);
        }
        if state.cancelled {
            return Err(Error::Cancelled);
        }
        if self.clock.now() >= self.grant.expires_at {
            return Err(Error::GrantExpired);
        }
        Ok(())
    }

    pub fn observe(&self, target: &WindowTarget) -> Result<Observation, Error> {
        let (flight, reference, provider, started_at, retired) = {
            let mut state = self.state();
            if state.busy {
                return Err(Error::Busy);
            }
            self.authorize(&state, target, Capability::Observe)?;
            state.generation = state
                .generation
                .checked_add(1)
                .expect("observation identity exhausted");
            let reference = ObservationRef {
                context: self.context,
                generation: state.generation,
            };
            let retired = state.observation.take();
            let provider = self.backend.stamp();
            let started_at = self.clock.now();
            state.busy = true;
            (
                Flight::new(&self.state),
                reference,
                provider,
                started_at,
                retired,
            )
        };
        let expires_at = started_at
            .checked_add(self.observation_ttl)
            .unwrap_or(self.grant.expires_at)
            .min(self.grant.expires_at);
        // Native bindings can have meaningful destructors. Never drop them under
        // the state lock; cancellation must remain able to acquire that lock.
        drop(retired);
        // Cleanup can block. Recheck authority before starting a new native read,
        // rather than reading after cancellation and merely discarding its data.
        let ready = {
            let state = self.state();
            self.authorize(&state, target, Capability::Observe)
                .and_then(|_| {
                    if self.backend.stamp() != provider {
                        Err(Error::WrongProvider)
                    } else if self.clock.now() >= expires_at {
                        Err(Error::StaleObservation)
                    } else {
                        Ok(())
                    }
                })
        };
        if let Err(error) = ready {
            drop(flight.finish());
            return Err(error);
        }
        let received = self.backend.observe(target).and_then(|observed| {
            // Validate/drop rejected native replies outside the lock, while the
            // operation is still busy. Cleanup is part of this admission slot.
            if observed.target != *target {
                return Err(Error::TargetChanged);
            }
            let mut ids = std::collections::HashSet::new();
            if observed.elements.iter().any(|e| !ids.insert(e.id.0)) {
                return Err(Error::InvalidObservation);
            }
            Ok(observed)
        });
        let observed = match received {
            Ok(observed) => observed,
            Err(error) => {
                let state = flight.finish();
                self.live_context(&state)?;
                return Err(error);
            }
        };
        let mut state = self.state();
        let invalidated = self
            .live_context(&state)
            .err()
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider))
            .or_else(|| (self.clock.now() >= expires_at).then_some(Error::StaleObservation));
        if let Some(error) = invalidated {
            drop(state);
            drop(observed);
            // Do not release admission until native cleanup really returned.
            let state = flight.finish();
            self.live_context(&state)?;
            return Err(error);
        }
        let result = Observation {
            reference,
            target: target.clone(),
            elements: observed.elements.clone(),
        };
        state.observation = Some(LiveObservation {
            reference,
            provider,
            target: target.clone(),
            expires_at,
            elements: observed.elements,
            native: observed.binding,
        });
        flight.finish_locked(&mut state);
        Ok(result)
    }

    pub fn press(
        &self,
        target: &WindowTarget,
        reference: ObservationRef,
        element: ElementId,
    ) -> ActionReceipt {
        let prepared = {
            let mut state = self.state();
            self.prepare_press(&mut state, target, reference, element)
        };
        let (flight, observation) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return ActionReceipt::rejected(error),
        };
        let outcome = self.backend.press(target, &observation.native, element);
        let provider = observation.provider.clone();
        // Retain the binding through the call, then clean up outside the lock
        // without permitting another operation to overlap that cleanup.
        drop(observation);
        let state = flight.finish();
        let interruption = self
            .live_context(&state)
            .err()
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider));
        ActionReceipt::from_provider(outcome, interruption)
    }

    fn prepare_press<'a>(
        &'a self,
        state: &mut State<B::Binding>,
        target: &WindowTarget,
        reference: ObservationRef,
        element: ElementId,
    ) -> Result<(Flight<'a, B::Binding>, LiveObservation<B::Binding>), Error> {
        if state.busy {
            return Err(Error::Busy);
        }
        self.authorize(state, target, Capability::Press)?;
        let live = state.observation.as_ref().ok_or(Error::StaleObservation)?;
        if live.reference != reference || self.clock.now() >= live.expires_at {
            return Err(Error::StaleObservation);
        }
        if live.target != *target {
            return Err(Error::TargetChanged);
        }
        if live.provider != self.backend.stamp() {
            return Err(Error::WrongProvider);
        }
        let found = live
            .elements
            .iter()
            .find(|e| e.id == element)
            .ok_or(Error::ElementNotFound)?;
        if !found.can_press {
            return Err(Error::ElementNotPressable);
        }
        // Consume before dispatch. Any provider outcome requires fresh observation.
        let observation = state.observation.take().expect("observation validated");
        state.busy = true;
        Ok((Flight::new(&self.state), observation))
    }

    /// Terminal for this execution context. Does not stop a native call, release
    /// its binding or clear busy until the actual call has returned.
    pub fn cancel(&self) {
        let retired = {
            let mut state = self.state();
            state.cancelled = true;
            state.observation.take()
        };
        drop(retired);
    }

    pub fn close(&self) {
        let retired = {
            let mut state = self.state();
            state.closed = true;
            state.observation.take()
        };
        drop(retired);
    }
}

/// Releases admission on every return path. A panicking backend closes the
/// context rather than permitting reuse of potentially inconsistent native state.
struct Flight<'a, B> {
    state: &'a Mutex<State<B>>,
    completed: bool,
}
impl<'a, B> Flight<'a, B> {
    fn new(state: &'a Mutex<State<B>>) -> Self {
        Self {
            state,
            completed: false,
        }
    }
    fn finish_locked(mut self, state: &mut State<B>) {
        state.busy = false;
        self.completed = true;
    }
    fn finish(mut self) -> MutexGuard<'a, State<B>> {
        let mut state = self.state.lock().expect("execution state poisoned");
        state.busy = false;
        self.completed = true;
        state
    }
}
impl<B> Drop for Flight<'_, B> {
    fn drop(&mut self) {
        if !self.completed {
            let retired = {
                let mut state = self.state.lock().expect("execution state poisoned");
                state.closed = true;
                state.busy = false;
                state.observation.take()
            };
            drop(retired);
        }
    }
}
