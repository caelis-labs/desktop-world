//! One app-bound execution context, with window- and kind-scoped observations.
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
const MAX_OBSERVATION_SLOTS: usize = 32;
const MAX_QUERY_ELEMENTS: usize = 4096;
const MAX_QUERY_VISITED: u32 = 8192;
const MAX_OBSERVATION_BYTES: usize = 8 * 1024 * 1024;

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
    query: Option<ObserveQuery>,
    discovery: Option<DiscoveryPage>,
    native: B,
}

struct State<B> {
    busy: bool,
    cancelled: bool,
    closed: bool,
    generation: u64,
    observations: Vec<LiveObservation<B>>,
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
                observations: Vec::new(),
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

    /// Reports retained ownership for public reference-cache cleanup only.
    /// This does not call the provider or validate whether an action can dispatch.
    pub fn retains_observation(&self, reference: ObservationRef) -> bool {
        let state = self.state();
        !state.closed
            && !state.cancelled
            && state
                .observations
                .iter()
                .any(|live| live.reference == reference && self.clock.now() < live.expires_at)
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
            let now = self.clock.now();
            let retired = take_observations(&mut state, |o| o.expires_at <= now);
            (Flight::new(&self.state), retired, self.backend.stamp())
        };
        drop(retired);
        let poll = || self.remaining(deadline.expect("bounded call deadline"), deadline);
        let call = CallContext { poll: &poll };
        let result = call
            .remaining()
            .and_then(|_| {
                if self.backend.stamp() == provider {
                    Ok(())
                } else {
                    Err(Error::WrongProvider)
                }
            })
            .and_then(|_| self.backend.windows(&self.app, &call));
        let validated = (|| {
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
        })();
        let mut state = self.state();
        let result = self
            .live_context(&state)
            .and_then(|_| self.check_deadline(deadline))
            .and_then(|_| {
                if self.backend.stamp() == provider {
                    Ok(())
                } else {
                    Err(Error::WrongProvider)
                }
            })
            .and(validated);
        // Only a complete validated inventory proves a target disappeared.
        let retired = match &result {
            Ok(inventory) => take_observations(&mut state, |o| {
                o.expires_at <= self.clock.now()
                    || !inventory
                        .windows
                        .iter()
                        .any(|window| window.target == o.target)
            }),
            Err(_) => Vec::new(),
        };
        drop(state);
        drop(retired);
        let state = flight.finish();
        self.live_context(&state)?;
        self.check_deadline(deadline)?;
        if self.backend.stamp() != provider {
            return Err(Error::WrongProvider);
        }
        result
    }

    pub fn observe(&self, target: &WindowTarget) -> Result<Observation, Error> {
        self.read(
            target,
            ObservationKind::Semantic,
            Some(&ObserveQuery::default()),
            None,
        )
    }

    pub fn observe_until(
        &self,
        target: &WindowTarget,
        deadline: Instant,
    ) -> Result<Observation, Error> {
        self.observe_query_until(target, &ObserveQuery::default(), deadline)
    }

    pub fn observe_query_until(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        deadline: Instant,
    ) -> Result<Observation, Error> {
        self.read(
            target,
            ObservationKind::Semantic,
            Some(query),
            Some(deadline),
        )
    }

    pub fn capture(&self, target: &WindowTarget) -> Result<Observation, Error> {
        self.read(target, ObservationKind::Pixels, None, None)
    }

    pub fn capture_until(
        &self,
        target: &WindowTarget,
        deadline: Instant,
    ) -> Result<Observation, Error> {
        self.read(target, ObservationKind::Pixels, None, Some(deadline))
    }

    fn read(
        &self,
        target: &WindowTarget,
        kind: ObservationKind,
        query: Option<&ObserveQuery>,
        deadline: Option<Instant>,
    ) -> Result<Observation, Error> {
        if let Some(query) = query {
            validate_query(query)?;
        }
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
            let now = self.clock.now();
            if state
                .observations
                .iter()
                .filter(|o| o.expires_at > now && !(o.target == *target && o.kind == kind))
                .count()
                >= MAX_OBSERVATION_SLOTS
            {
                return Err(resource_limit("Observation slot limit reached"));
            }
            state.generation = state
                .generation
                .checked_add(1)
                .expect("observation identity exhausted");
            let reference = ObservationRef {
                context: self.context,
                generation: state.generation,
            };
            let retired = take_observations(&mut state, |o| {
                o.expires_at <= now || (o.target == *target && o.kind == kind)
            });
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
            ObservationKind::Semantic => self.backend.observe_page(target, query.unwrap(), &call),
            ObservationKind::Pixels => self.backend.capture(target, &call),
        }
        .and_then(|observed| {
            // Validate/drop rejected native replies outside the lock, while the
            // operation is still busy. Cleanup is part of this admission slot.
            if observed.target != *target {
                return Err(Error::TargetChanged);
            }
            match (kind, &observed.image) {
                (ObservationKind::Semantic, None) => {
                    validate_page(
                        query.unwrap(),
                        &observed.elements,
                        observed.discovery.as_ref(),
                        None,
                    )?;
                }
                (ObservationKind::Pixels, Some(image))
                    if image.width > 0
                        && image.height > 0
                        && !image.png.is_empty()
                        && observed.elements.is_empty()
                        && observed.discovery.is_none() => {}
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
            .or_else(|| (self.clock.now() >= expires_at).then_some(Error::StaleObservation))
            .or_else(|| {
                (observation_bytes(&state) + element_bytes(&observed.elements)
                    > MAX_OBSERVATION_BYTES)
                    .then(|| resource_limit("Observation metadata limit reached"))
            });
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
            discovery: observed.discovery.clone(),
        };
        state.observations.push(LiveObservation {
            reference,
            provider,
            target: target.clone(),
            expires_at,
            elements: observed.elements,
            kind,
            image_size: observed.image.as_ref().map(|i| (i.width, i.height)),
            query: query.cloned(),
            discovery: observed.discovery,
            native: observed.binding,
        });
        flight.finish_locked(&mut state);
        Ok(result)
    }

    /// Continue the original native traversal without refreshing its lifetime.
    /// Every returned page belongs to the same observation and element ID space.
    pub fn continue_observation_until(
        &self,
        target: &WindowTarget,
        reference: ObservationRef,
        deadline: Instant,
    ) -> Result<Observation, Error> {
        let deadline = self.bounded_deadline(Some(deadline));
        let (flight, mut live) = {
            let mut state = self.state();
            if state.busy {
                return Err(Error::Busy);
            }
            self.validate_target(&state, &target.app, Capability::Observe)?;
            self.check_deadline(deadline)?;
            let index = state
                .observations
                .iter()
                .position(|o| o.reference == reference)
                .ok_or(Error::StaleObservation)?;
            let live = &state.observations[index];
            if self.clock.now() >= live.expires_at {
                return Err(Error::StaleObservation);
            }
            if live.target != *target {
                return Err(Error::TargetChanged);
            }
            if live.provider != self.backend.stamp() {
                return Err(Error::WrongProvider);
            }
            if live.kind != ObservationKind::Semantic
                || !live.discovery.as_ref().is_some_and(|page| page.has_more)
            {
                return Err(Error::InvalidObservation);
            }
            let live = state.observations.swap_remove(index);
            state.busy = true;
            (Flight::new(&self.state), live)
        };
        let poll = || self.remaining(live.expires_at, deadline);
        let call = CallContext { poll: &poll };
        let received = call
            .remaining()
            .and_then(|_| {
                self.backend
                    .continue_observation(target, &live.native, &call)
            })
            .and_then(|page| {
                if page.target != *target {
                    return Err(Error::TargetChanged);
                }
                validate_page(
                    live.query.as_ref().expect("semantic query"),
                    &page.elements,
                    Some(&page.discovery),
                    Some((&live.elements, live.discovery.as_ref().unwrap().visited)),
                )?;
                Ok(page)
            });
        let page = match received {
            Ok(page) => page,
            Err(error) => {
                // Native progress may already have advanced. Never restore this
                // query after a failed continuation or automatically replay it.
                drop(live);
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
            .or_else(|| (self.backend.stamp() != live.provider).then_some(Error::WrongProvider))
            .or_else(|| (self.clock.now() >= live.expires_at).then_some(Error::StaleObservation))
            .or_else(|| {
                (observation_bytes(&state)
                    + element_bytes(&live.elements)
                    + element_bytes(&page.elements)
                    > MAX_OBSERVATION_BYTES)
                    .then(|| resource_limit("Observation metadata limit reached"))
            });
        if let Some(error) = invalidated {
            drop(state);
            drop(live);
            let state = flight.finish();
            self.live_context(&state)?;
            self.check_deadline(deadline)?;
            return Err(error);
        }
        let result = Observation {
            reference: live.reference,
            target: live.target.clone(),
            elements: page.elements.clone(),
            image: None,
            kind: ObservationKind::Semantic,
            discovery: Some(page.discovery.clone()),
        };
        live.elements.extend(page.elements);
        live.discovery = Some(page.discovery);
        state.observations.push(live);
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
        let (flight, observation, retired) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return ActionReceipt::rejected(error),
        };
        drop(retired);
        let poll = || self.remaining(observation.expires_at, deadline);
        let call = CallContext { poll: &poll };
        let ready = call.remaining().and_then(|_| {
            if self.backend.stamp() == observation.provider {
                Ok(())
            } else {
                Err(Error::WrongProvider)
            }
        });
        let outcome = match ready {
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
    ) -> Result<
        (
            Flight<'a, B::Binding>,
            LiveObservation<B::Binding>,
            Vec<LiveObservation<B::Binding>>,
        ),
        Error,
    > {
        if state.busy {
            return Err(Error::Busy);
        }
        let (capability, kind) = match action {
            Action::Press(_) => (Capability::Press, ObservationKind::Semantic),
            Action::Click(_) => (Capability::Click, ObservationKind::Pixels),
        };
        self.validate_target(state, &target.app, capability)?;
        self.check_deadline(deadline)?;
        let index = state
            .observations
            .iter()
            .position(|o| o.reference == reference)
            .ok_or(Error::StaleObservation)?;
        let live = &state.observations[index];
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
        let observation = state.observations.swap_remove(index);
        // An admitted action can change this window's semantics and pixels.
        // Other windows retain their independent, revalidated observations.
        let retired = take_observations(state, |o| o.target == *target);
        state.busy = true;
        Ok((Flight::new(&self.state), observation, retired))
    }

    /// Terminal for this context. Native providers observe cancellation through
    /// CallContext; busy remains held until the call and cleanup return.
    pub fn cancel(&self) -> Result<(), Error> {
        let retired = {
            let mut state = self.state();
            state.cancelled = true;
            std::mem::take(&mut state.observations)
        };
        drop(retired);
        self.backend.shutdown()
    }

    pub fn close(&self) -> Result<(), Error> {
        let retired = {
            let mut state = self.state();
            state.closed = true;
            std::mem::take(&mut state.observations)
        };
        drop(retired);
        self.backend.shutdown()
    }
}

fn resource_limit(message: &str) -> Error {
    Error::NativeFailure {
        code: "resource_limit".into(),
        message: message.into(),
    }
}

fn validate_query(query: &ObserveQuery) -> Result<(), Error> {
    if !(1..=100).contains(&query.limit)
        || query
            .role
            .as_ref()
            .is_some_and(|role| role.is_empty() || role.chars().count() > 64)
        || query
            .text
            .as_ref()
            .is_some_and(|text| text.is_empty() || text.chars().count() > 256)
    {
        return Err(Error::NativeFailure {
            code: "invalid_query".into(),
            message: "Observe query exceeds its declared bounds".into(),
        });
    }
    Ok(())
}

fn validate_page(
    query: &ObserveQuery,
    elements: &[Element],
    discovery: Option<&DiscoveryPage>,
    prior: Option<(&[Element], u32)>,
) -> Result<(), Error> {
    let page = discovery.ok_or(Error::InvalidObservation)?;
    let (previous, visited) = prior.unwrap_or((&[], 0));
    if elements.len() > query.limit as usize
        || page.visited < visited
        || page.visited < (previous.len() + elements.len()) as u32
        || (page.complete && (page.has_more || page.reason.is_some()))
        || (!page.complete && !page.has_more && page.reason.is_none())
        || page
            .reason
            .as_ref()
            .is_some_and(|reason| reason.is_empty() || reason.len() > 128)
    {
        return Err(Error::InvalidObservation);
    }
    if previous.len() + elements.len() > MAX_QUERY_ELEMENTS || page.visited > MAX_QUERY_VISITED {
        return Err(resource_limit(
            "Native discovery exceeds the query retention limit",
        ));
    }
    let mut ids: std::collections::HashSet<_> =
        previous.iter().map(|element| element.id.0).collect();
    if elements.iter().any(|element| !ids.insert(element.id.0)) {
        return Err(Error::InvalidObservation);
    }
    Ok(())
}

fn take_observations<B>(
    state: &mut State<B>,
    mut retire: impl FnMut(&LiveObservation<B>) -> bool,
) -> Vec<LiveObservation<B>> {
    let mut retired = Vec::new();
    let mut index = 0;
    while index < state.observations.len() {
        if retire(&state.observations[index]) {
            retired.push(state.observations.swap_remove(index));
        } else {
            index += 1;
        }
    }
    retired
}

fn element_bytes(elements: &[Element]) -> usize {
    elements
        .iter()
        .map(|element| {
            std::mem::size_of::<Element>()
                + element.label.len()
                + element.role.len()
                + element.name.as_ref().map_or(0, String::len)
                + element.value.as_ref().map_or(0, String::len)
                + element.actions.iter().map(String::len).sum::<usize>()
        })
        .sum()
}

fn observation_bytes<B>(state: &State<B>) -> usize {
    state
        .observations
        .iter()
        .map(|observation| element_bytes(&observation.elements))
        .sum()
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
                std::mem::take(&mut state.observations)
            };
            drop(retired);
        }
    }
}
