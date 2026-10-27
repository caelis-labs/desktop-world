//! DTW-owned contracts. Native implementation types stay behind `Backend`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppIdentity(pub String);

/// Evidence supplied by trusted discovery, never a PID-only user assertion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInstance {
    pub identity: AppIdentity,
    pub pid: u32,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTarget {
    pub app: AppInstance,
    pub native_id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Observe,
    Press,
    Capture,
    Click,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderStamp {
    pub id: String,
    /// Changes when native state/worker ownership is replaced.
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub id: ElementId,
    pub label: String,
    pub can_press: bool,
}

/// Opaque to callers; context identity prevents cross-runtime reuse.
/// This is an in-process reference, not an authentication secret or wire format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservationRef {
    pub(crate) context: u64,
    pub(crate) generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationKind {
    Semantic,
    Pixels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageFrame {
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub target: WindowTarget,
    pub title: String,
    pub visible: bool,
}

/// A live view of the execution authority; providers poll it during IPC waits.
/// Remaining time is a monotonic budget, not authority the provider can extend.
pub struct CallContext<'a> {
    pub(crate) poll: &'a (dyn Fn() -> Result<std::time::Duration, Error> + Sync),
}
impl CallContext<'_> {
    pub fn remaining(&self) -> Result<std::time::Duration, Error> {
        (self.poll)()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub reference: ObservationRef,
    pub target: WindowTarget,
    pub elements: Vec<Element>,
    pub image: Option<ImageFrame>,
    pub kind: ObservationKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Closed,
    Cancelled,
    Busy,
    GrantExpired,
    WrongApp,
    CapabilityDenied,
    UnsupportedCapability,
    StaleObservation,
    WrongProvider,
    TargetChanged,
    InvalidObservation,
    InvalidPoint,
    ElementNotFound,
    ElementNotPressable,
    TimedOut,
    BackendFailed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchState {
    NotDispatched,
    Dispatched,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectEvidence {
    /// Describes the specific postcondition observed, never overall task success.
    Confirmed(String),
    Unverified,
    Mismatch {
        expected: String,
        observed: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionOutcome {
    pub dispatch: DispatchState,
    pub evidence: EffectEvidence,
    pub error: Option<Error>,
}

impl ActionOutcome {
    pub fn rejected(error: Error) -> Self {
        Self {
            dispatch: DispatchState::NotDispatched,
            evidence: EffectEvidence::Unverified,
            error: Some(error),
        }
    }

    pub fn unknown(error: Error) -> Self {
        Self {
            dispatch: DispatchState::Unknown,
            evidence: EffectEvidence::Unverified,
            error: Some(error),
        }
    }
}

pub struct BackendObservation<B> {
    /// Actual verified target, checked against the requested target on return.
    pub target: WindowTarget,
    pub elements: Vec<Element>,
    pub image: Option<ImageFrame>,
    pub binding: B,
}

/// Statically selected capability groups; no generic tool escape.
///
/// Implementations revalidate OS ownership and retain handles through completion.
/// Native adapters isolate blocking APIs in supervised workers and poll the call
/// context. Cancellation cannot undo dispatched input. `stamp` and `supports`
/// are nonblocking. Binding cleanup must be bounded and safe on any calling thread.
pub trait Backend: Send + Sync {
    type Binding: Send;

    /// Stop and reap native resources. Failure does not reopen admission.
    fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }
    fn stamp(&self) -> ProviderStamp;
    fn supports(&self, capability: Capability) -> bool;
    fn windows(
        &self,
        _app: &AppInstance,
        _call: &CallContext<'_>,
    ) -> Result<Vec<WindowInfo>, Error> {
        Err(Error::UnsupportedCapability)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error>;
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome;
    fn capture(
        &self,
        _target: &WindowTarget,
        _call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        Err(Error::UnsupportedCapability)
    }
    fn click(
        &self,
        _target: &WindowTarget,
        _binding: &Self::Binding,
        _point: PixelPoint,
        _call: &CallContext<'_>,
    ) -> ActionOutcome {
        ActionOutcome::rejected(Error::UnsupportedCapability)
    }
}
