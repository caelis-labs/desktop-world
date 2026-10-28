//! One list for script exposure, admission and generated declarations.
use crate::computer::Capability;
#[derive(Clone, Copy)]
pub(super) enum Method {
    Windows,
    Observe,
    Capture,
    Press,
    Click,
}
impl Method {
    pub const ALL: [Self; 5] = [
        Self::Windows,
        Self::Observe,
        Self::Capture,
        Self::Press,
        Self::Click,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Observe => "observe",
            Self::Capture => "capture",
            Self::Press => "press",
            Self::Click => "click",
        }
    }
    pub fn arity(self) -> usize {
        if matches!(self, Self::Windows) {
            0
        } else {
            1
        }
    }
    pub fn window_target(self) -> bool {
        match self {
            Self::Windows => false,
            Self::Observe | Self::Capture | Self::Press | Self::Click => true,
        }
    }
    pub fn capability(self) -> Capability {
        match self {
            Self::Windows | Self::Observe => Capability::Observe,
            Self::Capture => Capability::Capture,
            Self::Press => Capability::Press,
            Self::Click => Capability::Click,
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name)
    }
}
pub(super) fn facade_metadata() -> String {
    serde_json::to_string(
        &Method::ALL
            .into_iter()
            .map(|m| serde_json::json!({"name":m.name(),"arity":m.arity()}))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
