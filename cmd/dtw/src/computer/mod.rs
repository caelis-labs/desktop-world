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

/// Catalog admission is about exact native members, not path equality. A stable
/// public application ID may cover more than one installed copy of an app.
pub fn validate_app_instances(apps: &[AppInstance]) -> Result<(), Error> {
    if apps.is_empty() || apps.len() > 32 {
        return Err(Error::NativeFailure {
            code: "invalid_app_instances".into(),
            message: "An application context requires between 1 and 32 verified instances".into(),
        });
    }
    if apps.iter().enumerate().any(|(index, app)| {
        apps[..index]
            .iter()
            .any(|prior| prior.pid == app.pid && prior != app)
    }) {
        return Err(Error::NativeFailure {
            code: "invalid_app_instances".into(),
            message: "One process cannot have conflicting native identities in the same catalog"
                .into(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTarget {
    pub app: AppInstance,
    pub native_id: u64,
    pub generation: u64,
}

/// Service launch policy; callers cannot override it in individual scripts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputPolicy {
    #[default]
    Auto,
    BackgroundOnly,
}
impl InputPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::BackgroundOnly => "background_only",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRoute {
    Semantic,
    Background,
    Foreground,
}
impl InputRoute {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Background => "background",
            Self::Foreground => "foreground",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusReturn {
    Pending,
    Restored,
    ApplicationRestored,
    ChangedExternally,
    PreviousAppClosed,
    Unchanged,
    Unconfirmed,
}
impl FocusReturn {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Restored => "restored",
            Self::ApplicationRestored => "application_restored",
            Self::ChangedExternally => "changed_externally",
            Self::PreviousAppClosed => "previous_app_closed",
            Self::Unchanged => "unchanged",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}

/// Provider-reported routing evidence, independent of effect confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDelivery {
    pub route: Option<InputRoute>,
    pub reason: Option<String>,
    pub foreground_borrowed: bool,
    pub focus_return: Option<FocusReturn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Windows,
    Observe,
    Press,
    Capture,
    Click,
    TypeText,
    Keypress,
    Scroll,
    Focus,
    SetValue,
    ReadText,
}

impl Capability {
    pub const ALL: [Self; 11] = [
        Self::Windows,
        Self::Observe,
        Self::Press,
        Self::Capture,
        Self::Click,
        Self::TypeText,
        Self::Keypress,
        Self::Scroll,
        Self::Focus,
        Self::SetValue,
        Self::ReadText,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Observe => "observe",
            Self::Press => "press",
            Self::Capture => "capture",
            Self::Click => "click",
            Self::TypeText => "typeText",
            Self::Keypress => "keypress",
            Self::Scroll => "scroll",
            Self::Focus => "focus",
            Self::SetValue => "setValue",
            Self::ReadText => "readText",
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

/// A region in pixels of one retained capture, never desktop coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveScopeKind {
    Window,
    Menu,
    Subtree,
    Region,
}
impl ObserveScopeKind {
    pub const ALL: [Self; 4] = [Self::Window, Self::Menu, Self::Subtree, Self::Region];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Menu => "menu",
            Self::Subtree => "subtree",
            Self::Region => "region",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObserveScope {
    Window,
    Menu,
    Subtree(ElementId),
    Region(PixelRegion),
}
impl ObserveScope {
    pub fn kind(self) -> ObserveScopeKind {
        match self {
            Self::Window => ObserveScopeKind::Window,
            Self::Menu => ObserveScopeKind::Menu,
            Self::Subtree(_) => ObserveScopeKind::Subtree,
            Self::Region(_) => ObserveScopeKind::Region,
        }
    }
    pub fn observation_kind(self) -> Option<ObservationKind> {
        match self {
            Self::Subtree(_) => Some(ObservationKind::Semantic),
            Self::Region(_) => Some(ObservationKind::Pixels),
            _ => None,
        }
    }
}

/// UTF-16 offsets follow native text APIs and JS strings. Providers never split
/// surrogate pairs and pin the first read's content in the existing binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRequest {
    pub element: ElementId,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPage {
    pub text: String,
    pub offset: u32,
    pub next_offset: Option<u32>,
    pub total_length: u32,
}
pub struct BackendTextPage {
    pub target: WindowTarget,
    pub page: TextPage,
}

/// A scroll point bound to a retained observation, never a global coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollAnchor {
    Pixels(PixelPoint),
    Element(ElementId),
}
impl ScrollAnchor {
    pub fn kind(self) -> ObservationKind {
        match self {
            Self::Pixels(_) => ObservationKind::Pixels,
            Self::Element(_) => ObservationKind::Semantic,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRequest {
    pub anchor: ScrollAnchor,
    pub delta_x: i32,
    pub delta_y: i32,
}

/// One complete input transaction. No held keys or buttons escape an atomic call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowInput {
    Focus,
    TypeText { text: String },
    Keypress { keys: Vec<String> },
    Scroll { delta_x: i32, delta_y: i32 },
}
impl WindowInput {
    pub fn capability(&self) -> Capability {
        match self {
            Self::Focus => Capability::Focus,
            Self::TypeText { .. } => Capability::TypeText,
            Self::Keypress { .. } => Capability::Keypress,
            Self::Scroll { .. } => Capability::Scroll,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseButton {
    #[default]
    Left,
    Right,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClickOptions {
    pub button: MouseButton,
    pub count: u8,
}
impl Default for ClickOptions {
    fn default() -> Self {
        Self {
            button: MouseButton::Left,
            count: 1,
        }
    }
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
    SameProcessApplicationSurfaces,
    /// The same-process inventory plus individually proven associated surfaces.
    /// This never claims that every possible association was discovered.
    ApplicationAndProvenAssociatedSurfaces {
        associated_count: u32,
    },
}

impl InventoryScope {
    pub(crate) fn associated_count(self) -> Option<u32> {
        match self {
            Self::ApplicationAndProvenAssociatedSurfaces { associated_count } => {
                Some(associated_count)
            }
            _ => None,
        }
    }
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
            && self
                .scope
                .associated_count()
                .is_none_or(|count| count <= self.supported_count)
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

/// Each provider inventory retains its exact process owner. A failed member
/// cannot prove that its previous windows disappeared or erase a healthy peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceInventory {
    pub app: AppInstance,
    pub report: Result<InventoryReport, Error>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppWindowInventory {
    pub windows: Vec<WindowInfo>,
    pub instances: Vec<InstanceInventory>,
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
    AppInventoryUnproven(Vec<Error>),
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
    pub delivery: Option<InputDelivery>,
    pub dispatch: DispatchState,
    pub evidence: EffectEvidence,
    pub error: Option<Error>,
}

impl ActionOutcome {
    pub fn rejected(error: Error) -> Self {
        Self {
            delivery: None,
            dispatch: DispatchState::NotDispatched,
            evidence: EffectEvidence::Unverified,
            error: Some(error),
        }
    }

    pub fn unknown(error: Error) -> Self {
        Self {
            delivery: None,
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

    /// Whether this native owner can admit future work. This is a nonblocking
    /// lifecycle read, not a target validation or permission check. A false
    /// result does not erase the original error or prove cleanup succeeded;
    /// the host must retire this owner before replacing it for a new request.
    fn is_available(&self) -> bool {
        true
    }

    /// Sticky failure to settle owned native resources after an operation.
    /// Reading this never performs cleanup and never replaces the operation's
    /// original result. Execution owners must seal admission and report it
    /// separately, including when a script catches the operation's own error.
    fn cleanup_error(&self) -> Option<Error> {
        None
    }

    /// Release temporary resources borrowed by one script, including foreground
    /// focus. App state and native observations remain available to later scripts.
    fn finish_execution(&self) -> Result<Option<FocusReturn>, Error> {
        Ok(None)
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
    fn supports_observe_scope(&self, scope: ObserveScopeKind) -> bool {
        scope == ObserveScopeKind::Window
    }
    fn observe_scoped(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        scope: ObserveScope,
        _binding: Option<&Self::Binding>,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        if scope != ObserveScope::Window {
            return Err(Error::UnsupportedCapability);
        }
        self.observe_page(target, query, call)
    }
    fn read_text(
        &self,
        _target: &WindowTarget,
        _binding: &Self::Binding,
        _request: TextRequest,
        _call: &CallContext<'_>,
    ) -> Result<BackendTextPage, Error> {
        Err(Error::UnsupportedCapability)
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
    fn input(
        &self,
        _target: &WindowTarget,
        _input: &WindowInput,
        _call: &CallContext<'_>,
    ) -> ActionOutcome {
        ActionOutcome::rejected(Error::UnsupportedCapability)
    }
    /// Separate from center scrolling, so unsupported providers reject before
    /// Core consumes the observation.
    fn supports_scroll_anchor(&self, _kind: ObservationKind) -> bool {
        false
    }
    fn scroll_at(
        &self,
        _target: &WindowTarget,
        _binding: &Self::Binding,
        _scroll: ScrollRequest,
        _call: &CallContext<'_>,
    ) -> ActionOutcome {
        ActionOutcome::rejected(Error::UnsupportedCapability)
    }
    fn set_value(
        &self,
        _target: &WindowTarget,
        _binding: &Self::Binding,
        _element: ElementId,
        _value: &str,
        _call: &CallContext<'_>,
    ) -> ActionOutcome {
        ActionOutcome::rejected(Error::UnsupportedCapability)
    }
    fn click_with_options(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        point: PixelPoint,
        options: ClickOptions,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        if options == ClickOptions::default() {
            self.click(target, binding, point, call)
        } else {
            ActionOutcome::rejected(Error::UnsupportedCapability)
        }
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
