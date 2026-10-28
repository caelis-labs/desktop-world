//! One list for script exposure, admission and generated declarations.
use crate::computer::Capability;
#[derive(Clone, Copy)]
pub(super) enum Method {
    Windows,
    Observe,
    Capture,
    Press,
    Click,
    TypeText,
    Keypress,
    Scroll,
    Focus,
    SetValue,
}
impl Method {
    pub const ALL: [Self; 10] = [
        Self::Windows,
        Self::Observe,
        Self::Capture,
        Self::Press,
        Self::Click,
        Self::TypeText,
        Self::Keypress,
        Self::Scroll,
        Self::Focus,
        Self::SetValue,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Observe => "observe",
            Self::Capture => "capture",
            Self::Press => "press",
            Self::Click => "click",
            Self::TypeText => "typeText",
            Self::Keypress => "keypress",
            Self::Scroll => "scroll",
            Self::Focus => "focus",
            Self::SetValue => "setValue",
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
        !matches!(self, Self::Windows)
    }
    pub fn capability(self) -> Capability {
        match self {
            Self::Windows => Capability::Windows,
            Self::Observe => Capability::Observe,
            Self::Capture => Capability::Capture,
            Self::Press => Capability::Press,
            Self::Click => Capability::Click,
            Self::TypeText => Capability::TypeText,
            Self::Keypress => Capability::Keypress,
            Self::Scroll => Capability::Scroll,
            Self::Focus => Capability::Focus,
            Self::SetValue => Capability::SetValue,
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
