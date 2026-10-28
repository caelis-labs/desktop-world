//! One app-bound execution context, with a single live observation.
//!
//! This module owns app identity, reference lifetime and execution admission.
//! Harness approval policy is outside DTW; no authorization state is kept here.

use crate::computer::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, MutexGuard,
};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

/// Maximum budget of one atomic backend call; scripts may tighten it further.
const MAX_CALL_DURATION: Duration = Duration::from_secs(15);

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
    kind: ObservationKind,
    image_size: Option<(u32, u32)>,
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
    app: AppInstance,
    clock: C,
    observation_ttl: Duration,
    context: u64,
}

impl<B: Backend> Runtime<B> {
    pub fn new(backend: B, app: AppInstance, observation_ttl: Duration) -> Self {
        Self::with_clock(backend, app, observation_ttl, MonotonicClock)
    }
}

impl<B: Backend, C: Clock> Runtime<B, C> {
    pub fn with_clock(backend: B, app: AppInstance, observation_ttl: Duration, clock: C) -> Self {
        let context = NEXT_CONTEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("execution context identity exhausted");
        Self {
            backend,
            app,
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

    /// Resource cleanup diagnostics remain separate from native action evidence.
    pub fn cleanup_error(&self) -> Option<Error> {
        self.backend.cleanup_error()
    }

    fn validate_target(
        &self,
        state: &State<B::Binding>,
        app: &AppInstance,
        capability: Capability,
    ) -> Result<(), Error> {
        self.live_context(state)?;
        if self.app != *app {
            return Err(Error::WrongApp);
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
        Ok(())
    }

    fn bounded_deadline(&self, requested: Option<Instant>) -> Option<Instant> {
        let maximum = self.clock.now() + MAX_CALL_DURATION;
        Some(requested.map_or(maximum, |deadline| deadline.min(maximum)))
    }

    // This check takes no state lock: callers also use it while holding admission.
    fn check_deadline(&self, deadline: Option<Instant>) -> Result<(), Error> {
        if deadline.is_some_and(|until| self.clock.now() >= until) {
            Err(Error::TimedOut)
        } else {
            Ok(())
        }
    }

    fn remaining(&self, expires_at: Instant, deadline: Option<Instant>) -> Result<Duration, Error> {
        self.live_context(&self.state())?;
        let expires_at = deadline.map_or(expires_at, |until| until.min(expires_at));
        let remaining = expires_at.saturating_duration_since(self.clock.now());
        if remaining.is_zero() {
            Err(Error::TimedOut)
        } else {
            Ok(remaining)
        }
    }

    /// Discovers windows for the bound app using the provider's Observe support.
    pub fn windows(&self) -> Result<WindowInventory, Error> {
        self.list_windows(None)
    }

    /// Tightens this call's execution budget.
    pub fn windows_until(&self, deadline: Instant) -> Result<WindowInventory, Error> {
        self.list_windows(Some(deadline))
    }

    fn list_windows(&self, deadline: Option<Instant>) -> Result<WindowInventory, Error> {
        let deadline = self.bounded_deadline(deadline);
        let (flight, retired, provider) = {
            let mut state = self.state();
            if state.busy {
                return Err(Error::Busy);
            }
            self.validate_target(&state, &self.app, Capability::Observe)?;
            self.check_deadline(deadline)?;
            state.busy = true;
            (
                Flight::new(&self.state),
                state.observation.take(),
                self.backend.stamp(),
            )
        };
        drop(retired);
        let poll = || self.remaining(deadline.expect("bounded call deadline"), deadline);
        let call = CallContext { poll: &poll };
        let result = call
            .remaining()
            .and_then(|_| self.backend.windows(&self.app, &call));
        let state = flight.finish();
        self.live_context(&state)?;
        self.check_deadline(deadline)?;
        if self.backend.stamp() != provider {
            return Err(Error::WrongProvider);
        }
        let inventory = match result {
            Err(Error::InventoryUnproven(report)) if !report.valid() => {
                return Err(Error::BackendFailed("Invalid inventory diagnostics".into()));
            }
            other => other?,
        };
        let windows = &inventory.windows;
        if windows.iter().any(|w| w.target.app != self.app) {
            return Err(Error::WrongApp);
        }
        let report = &inventory.report;
        if !report.valid()
            || windows.len() != report.supported_count as usize
            || report.supported_count + report.excluded_count != report.candidate_count
            || windows.iter().enumerate().any(|(index, window)| {
                window.target.native_id == 0
                    || window.target.generation == 0
                    || window.title.len() > 4096
                    || windows[..index]
                        .iter()
                        .any(|prior| prior.target.native_id == window.target.native_id)
            })
        {
            return Err(Error::BackendFailed(
                "Invalid or incomplete window inventory".into(),
            ));
        }
        Ok(inventory)
    }

    pub fn observe(&self, target: &WindowTarget) -> Result<Observation, Error> {
        self.read(target, ObservationKind::Semantic, None)
    }

    pub fn observe_until(
        &self,
        target: &WindowTarget,
        deadline: Instant,
    ) -> Result<Observation, Error> {
        self.read(target, ObservationKind::Semantic, Some(deadline))
    }

    pub fn capture(&self, target: &WindowTarget) -> Result<Observation, Error> {
        self.read(target, ObservationKind::Pixels, None)
    }

    pub fn capture_until(
        &self,
        target: &WindowTarget,
        deadline: Instant,
    ) -> Result<Observation, Error> {
        self.read(target, ObservationKind::Pixels, Some(deadline))
    }

    fn read(
        &self,
        target: &WindowTarget,
        kind: ObservationKind,
        deadline: Option<Instant>,
    ) -> Result<Observation, Error> {
        let deadline = self.bounded_deadline(deadline);
        let capability = match kind {
            ObservationKind::Semantic => Capability::Observe,
            ObservationKind::Pixels => Capability::Capture,
        };
        let (flight, reference, provider, started_at, retired) = {
            let mut state = self.state();
            if state.busy {
                return Err(Error::Busy);
            }
            self.validate_target(&state, &target.app, capability)?;
            self.check_deadline(deadline)?;
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
            .unwrap_or(started_at);
        // Native bindings can have meaningful destructors. Never drop them under
        // the state lock; cancellation must remain able to acquire that lock.
        drop(retired);
        // Cleanup can block. Recheck the target before starting a new native read,
        // rather than reading after cancellation and merely discarding its data.
        let ready = {
            let state = self.state();
            self.validate_target(&state, &target.app, capability)
                .and_then(|_| self.check_deadline(deadline))
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
        let poll = || self.remaining(expires_at, deadline);
        let call = CallContext { poll: &poll };
        let received = match kind {
            ObservationKind::Semantic => self.backend.observe(target, &call),
            ObservationKind::Pixels => self.backend.capture(target, &call),
        }
        .and_then(|observed| {
            // Validate/drop rejected native replies outside the lock, while the
            // operation is still busy. Cleanup is part of this admission slot.
            if observed.target != *target {
                return Err(Error::TargetChanged);
            }
            match (kind, &observed.image) {
                (ObservationKind::Semantic, None) => {}
                (ObservationKind::Pixels, Some(image))
                    if image.width > 0
                        && image.height > 0
                        && !image.png.is_empty()
                        && observed.elements.is_empty() => {}
                _ => return Err(Error::InvalidObservation),
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
                self.check_deadline(deadline)?;
                return Err(error);
            }
        };
        let mut state = self.state();
        let invalidated = self
            .live_context(&state)
            .err()
            .or_else(|| self.check_deadline(deadline).err())
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider))
            .or_else(|| (self.clock.now() >= expires_at).then_some(Error::StaleObservation));
        if let Some(error) = invalidated {
            drop(state);
            drop(observed);
            // Do not release admission until native cleanup really returned.
            let state = flight.finish();
            self.live_context(&state)?;
            self.check_deadline(deadline)?;
            return Err(error);
        }
        let result = Observation {
            reference,
            target: target.clone(),
            elements: observed.elements.clone(),
            image: observed.image.clone(),
            kind,
        };
        state.observation = Some(LiveObservation {
            reference,
            provider,
            target: target.clone(),
            expires_at,
            elements: observed.elements,
            kind,
            image_size: observed.image.as_ref().map(|i| (i.width, i.height)),
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
        self.act(target, reference, Action::Press(element), None)
    }

    pub fn press_until(
        &self,
        target: &WindowTarget,
        reference: ObservationRef,
        element: ElementId,
        deadline: Instant,
    ) -> ActionReceipt {
        self.act(target, reference, Action::Press(element), Some(deadline))
    }

    pub fn click(
        &self,
        target: &WindowTarget,
        reference: ObservationRef,
        point: PixelPoint,
    ) -> ActionReceipt {
        self.act(target, reference, Action::Click(point), None)
    }

    pub fn click_until(
        &self,
        target: &WindowTarget,
        reference: ObservationRef,
        point: PixelPoint,
        deadline: Instant,
    ) -> ActionReceipt {
        self.act(target, reference, Action::Click(point), Some(deadline))
    }

    fn act(
        &self,
        target: &WindowTarget,
        reference: ObservationRef,
        action: Action,
        deadline: Option<Instant>,
    ) -> ActionReceipt {
        let deadline = self.bounded_deadline(deadline);
        let prepared = {
            let mut state = self.state();
            self.prepare_action(&mut state, target, reference, action, deadline)
        };
        let (flight, observation) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return ActionReceipt::rejected(error),
        };
        let poll = || self.remaining(observation.expires_at, deadline);
        let call = CallContext { poll: &poll };
        let outcome = match call.remaining() {
            Err(error) => ActionOutcome::rejected(error),
            Ok(_) => match action {
                Action::Press(element) => {
                    self.backend
                        .press(target, &observation.native, element, &call)
                }
                Action::Click(point) => {
                    self.backend
                        .click(target, &observation.native, point, &call)
                }
            },
        };
        let provider = observation.provider.clone();
        let expires_at = observation.expires_at;
        // Cleanup remains in the admission slot and outside the state lock.
        drop(observation);
        let state = flight.finish();
        let interruption = self
            .live_context(&state)
            .err()
            .or_else(|| self.check_deadline(deadline).err())
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider))
            .or_else(|| (self.clock.now() >= expires_at).then_some(Error::TimedOut));
        ActionReceipt::from_provider(outcome, interruption)
    }

    fn prepare_action<'a>(
        &'a self,
        state: &mut State<B::Binding>,
        target: &WindowTarget,
        reference: ObservationRef,
        action: Action,
        deadline: Option<Instant>,
    ) -> Result<(Flight<'a, B::Binding>, LiveObservation<B::Binding>), Error> {
        if state.busy {
            return Err(Error::Busy);
        }
        let (capability, kind) = match action {
            Action::Press(_) => (Capability::Press, ObservationKind::Semantic),
            Action::Click(_) => (Capability::Click, ObservationKind::Pixels),
        };
        self.validate_target(state, &target.app, capability)?;
        self.check_deadline(deadline)?;
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
        if live.kind != kind {
            return Err(Error::InvalidObservation);
        }
        match action {
            Action::Press(element) => {
                let found = live
                    .elements
                    .iter()
                    .find(|e| e.id == element)
                    .ok_or(Error::ElementNotFound)?;
                if !found.can_press {
                    return Err(Error::ElementNotPressable);
                }
            }
            Action::Click(point) => {
                let (width, height) = live.image_size.ok_or(Error::InvalidObservation)?;
                if !point.x.is_finite()
                    || !point.y.is_finite()
                    || point.x < 0.0
                    || point.y < 0.0
                    || point.x >= width as f64
                    || point.y >= height as f64
                {
                    return Err(Error::InvalidPoint);
                }
            }
        }
        // Consume before dispatch. No outcome permits replay of this binding.
        let observation = state.observation.take().expect("observation validated");
        state.busy = true;
        Ok((Flight::new(&self.state), observation))
    }

    /// Terminal for this context. Native providers observe cancellation through
    /// CallContext; busy remains held until the call and cleanup return.
    pub fn cancel(&self) -> Result<(), Error> {
        let retired = {
            let mut state = self.state();
            state.cancelled = true;
            state.observation.take()
        };
        drop(retired);
        self.backend.shutdown()
    }

    pub fn close(&self) -> Result<(), Error> {
        let retired = {
            let mut state = self.state();
            state.closed = true;
            state.observation.take()
        };
        drop(retired);
        self.backend.shutdown()
    }
}

#[derive(Clone, Copy)]
enum Action {
    Press(ElementId),
    Click(PixelPoint),
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
