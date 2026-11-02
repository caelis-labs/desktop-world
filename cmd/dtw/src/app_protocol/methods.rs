//! One list for script exposure, admission and generated declarations.
use crate::computer::Capability;
#[derive(Clone, Copy)]
pub(super) enum Method {
    Window,
    Launch,
    Quit,
    CloseWindow,
    Windows,
    WaitForWindow,
    Wait,
    Find,
    Observe,
    Capture,
    Press,
    Click,
    TypeText,
    Keypress,
    Scroll,
    Focus,
    SetValue,
    ReadText,
}
impl Method {
    pub const ALL: [Self; 18] = [
        Self::Window,
        Self::Launch,
        Self::Quit,
        Self::CloseWindow,
        Self::Windows,
        Self::WaitForWindow,
        Self::Wait,
        Self::Find,
        Self::Observe,
        Self::Capture,
        Self::Press,
        Self::Click,
        Self::TypeText,
        Self::Keypress,
        Self::Scroll,
        Self::Focus,
        Self::SetValue,
        Self::ReadText,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Launch => "launch",
            Self::Quit => "quit",
            Self::CloseWindow => "closeWindow",
            Self::Windows => "windows",
            Self::WaitForWindow => "waitForWindow",
            Self::Wait => "wait",
            Self::Find => "find",
            Self::Observe => "observe",
            Self::Capture => "capture",
            Self::Press => "press",
            Self::Click => "click",
            Self::TypeText => "typeText",
            Self::Keypress => "keypress",
            Self::Scroll => "scroll",
            Self::Focus => "focus",
            Self::SetValue => "setValue",
            Self::ReadText => "readText",
        }
    }
    pub fn arity(self) -> usize {
        if matches!(self, Self::Windows | Self::Launch) {
            0
        } else {
            1
        }
    }
    pub fn window_target(self) -> bool {
        !matches!(
            self,
            Self::Windows | Self::Window | Self::Launch | Self::Quit
        )
    }
    pub fn capability(self) -> Capability {
        match self {
            Self::Window => Capability::Windows,
            Self::Launch => Capability::Launch,
            Self::Quit => Capability::Quit,
            Self::CloseWindow => Capability::CloseWindow,
            Self::Windows | Self::WaitForWindow => Capability::Windows,
            Self::Find | Self::Wait => Capability::Observe,
            Self::Observe => Capability::Observe,
            Self::Capture => Capability::Capture,
            Self::Press => Capability::Press,
            Self::Click => Capability::Click,
            Self::TypeText => Capability::TypeText,
            Self::Keypress => Capability::Keypress,
            Self::Scroll => Capability::Scroll,
            Self::Focus => Capability::Focus,
            Self::SetValue => Capability::SetValue,
            Self::ReadText => Capability::ReadText,
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
            .map(|m| serde_json::json!({"name":m.name(),"arity":m.arity(),"optional":matches!(m, Method::Quit | Method::Window),"window":m.window_target()}))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
