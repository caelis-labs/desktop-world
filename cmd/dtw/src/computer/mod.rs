//! DTW-owned contracts. Native implementation types stay behind `Backend`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppIdentity(pub String);

/// Exact identity returned by discovery; a PID alone cannot identify an instance.
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

impl Capability {
    pub const ALL: [Self; 4] = [Self::Observe, Self::Press, Self::Capture, Self::Click];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Press => "press",
            Self::Capture => "capture",
            Self::Click => "click",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|capability| capability.as_str() == name)
    }
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
    /// Semantic role reported independently of the human-readable label.
    pub role: String,
    pub name: Option<String>,
    pub value: Option<String>,
    /// None means the provider did not expose a readable enabled state.
    pub enabled: Option<bool>,
    /// DTW operations available on this element, not unimplemented native actions.
    pub actions: Vec<String>,
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
pub struct ObserveQuery {
    pub role: Option<String>,
    pub text: Option<String>,
    pub limit: u32,
}

impl Default for ObserveQuery {
    fn default() -> Self {
        Self {
            role: None,
            text: None,
            limit: 40,
        }
    }
}

/// Progress through a live native traversal, not an atomic UI snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryPage {
    pub has_more: bool,
    pub complete: bool,
    pub reason: Option<String>,
    /// Cumulative native nodes visited by this query, including prior pages.
    pub visited: u32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryScope {
    SameProcessStandardWindows,
}

/// Bounded provider evidence, not an authorization or a retry instruction.
/// Names and error codes are provider-specific; no native handles or titles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryDiagnostics {
    pub provider: String,
    pub counters: Vec<(String, u64)>,
    pub error_codes: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryReport {
    pub scope: InventoryScope,
    pub candidate_count: u32,
    pub supported_count: u32,
    pub excluded_count: u32,
    pub diagnostics: InventoryDiagnostics,
}

impl InventoryReport {
    pub(crate) fn valid(&self) -> bool {
        let names = &self.diagnostics.counters;
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        };
        self.candidate_count <= 256
            && u64::from(self.supported_count) + u64::from(self.excluded_count)
                <= u64::from(self.candidate_count)
            && valid_name(&self.diagnostics.provider)
            && names.len() <= 32
            && names.iter().enumerate().all(|(index, (name, count))| {
                valid_name(name)
                    && *count <= 1_000_000
                    && !names[..index].iter().any(|(previous, _)| previous == name)
            })
            && self.diagnostics.error_codes.len() <= 8
    }
}

/// A successful inventory proves its declared subset, never that an app has
/// no windows. Unsupported windows may remain even when `windows` is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInventory {
    pub windows: Vec<WindowInfo>,
    pub report: InventoryReport,
}

/// A live execution budget and cancellation view; providers poll it during IPC waits.
/// Remaining time is monotonic and cannot be extended by the provider.
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
    pub discovery: Option<DiscoveryPage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Closed,
    Cancelled,
    Busy,
    WrongApp,
    UnsupportedCapability,
    StaleObservation,
    WrongProvider,
    TargetChanged,
    InvalidObservation,
    InvalidPoint,
    ElementNotFound,
    ElementNotPressable,
    TimedOut,
    InventoryUnproven(InventoryReport),
    BackendFailed(String),
    NativeFailure { code: String, message: String },
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
    pub discovery: Option<DiscoveryPage>,
}

/// A continuation updates the original binding; ownership is never duplicated.
pub struct BackendObservationPage {
    pub target: WindowTarget,
    pub elements: Vec<Element>,
    pub discovery: DiscoveryPage,
}

/// Statically selected capability groups; no generic tool escape.
///
/// Implementations revalidate OS ownership and retain handles through completion.
/// Native adapters isolate blocking APIs in supervised workers and poll the call
/// context. Cancellation cannot undo dispatched input. `stamp` and `supports`
/// are nonblocking. Binding cleanup must be bounded and safe on any calling thread.
pub trait Backend: Send + Sync {
    type Binding: Send;

    /// Sticky failure to settle owned native resources after an operation.
    /// Reading this never performs cleanup and never replaces the operation's
    /// original result. Execution owners must seal admission and report it
    /// separately, including when a script catches the operation's own error.
    fn cleanup_error(&self) -> Option<Error> {
        None
    }

    /// Stop and reap native resources. Failure does not reopen admission.
    fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }
    fn stamp(&self) -> ProviderStamp;
    fn supports(&self, capability: Capability) -> bool;
    /// Stable provider coverage and runtime conditions for tool discovery.
    fn coverage(&self) -> &'static str {
        "Provider-defined application window subset"
    }
    fn conditions(&self) -> &'static [&'static str] {
        &[]
    }
    fn windows(
        &self,
        _app: &AppInstance,
        _call: &CallContext<'_>,
    ) -> Result<WindowInventory, Error> {
        Err(Error::UnsupportedCapability)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error>;
    fn observe_page(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        if query.role.is_some() || query.text.is_some() {
            return Err(Error::UnsupportedCapability);
        }
        let mut observed = self.observe(target, call)?;
        if observed.elements.len() > query.limit as usize {
            return Err(Error::UnsupportedCapability);
        }
        if observed.discovery.is_none() {
            observed.discovery = Some(DiscoveryPage {
                has_more: false,
                complete: true,
                reason: None,
                visited: observed.elements.len() as u32,
            });
        }
        Ok(observed)
    }
    fn continue_observation(
        &self,
        _target: &WindowTarget,
        _binding: &Self::Binding,
        _call: &CallContext<'_>,
    ) -> Result<BackendObservationPage, Error> {
        Err(Error::UnsupportedCapability)
    }
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

#[cfg(test)]
pub(crate) fn test_inventory(windows: Vec<WindowInfo>) -> WindowInventory {
    WindowInventory {
        report: InventoryReport {
            scope: InventoryScope::SameProcessStandardWindows,
            candidate_count: windows.len() as u32,
            supported_count: windows.len() as u32,
            excluded_count: 0,
            diagnostics: InventoryDiagnostics {
                provider: "test".into(),
                counters: vec![],
                error_codes: vec![],
            },
        },
        windows,
    }
}
