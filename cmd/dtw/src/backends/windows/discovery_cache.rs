//! One-element UIA metadata batches. No descendant FindAll, retained stale
//! frontier cache, password/value contents or action pattern is requested here.
use windows::{
    core::VARIANT,
    Win32::{System::Variant::VT_BOOL, UI::Accessibility::*},
};

pub(super) struct Metadata {
    pub control: UIA_CONTROLTYPE_ID,
    pub name: String,
    pub enabled: bool,
    pub patterns: Option<Patterns>,
}
pub(super) struct Patterns {
    pub invoke: bool,
    pub toggle: bool,
    pub selection: bool,
    pub value: bool,
}
pub(super) struct Cache(Option<IUIAutomationCacheRequest>);
impl Cache {
    pub fn new(uia: &IUIAutomation) -> Self {
        let prepare = || -> windows::core::Result<IUIAutomationCacheRequest> {
            unsafe {
                let cache = uia.CreateCacheRequest()?;
                cache.SetTreeScope(TreeScope_Element)?;
                for property in [
                    UIA_ControlTypePropertyId,
                    UIA_NamePropertyId,
                    UIA_IsEnabledPropertyId,
                    UIA_IsInvokePatternAvailablePropertyId,
                    UIA_IsTogglePatternAvailablePropertyId,
                    UIA_IsSelectionItemPatternAvailablePropertyId,
                    UIA_IsValuePatternAvailablePropertyId,
                ] {
                    cache.AddProperty(property)?;
                }
                Ok(cache)
            }
        };
        // Older/custom providers can keep using the exact live getter path.
        Self(prepare().ok())
    }
    pub fn read(&self, original: &IUIAutomationElement) -> Option<Metadata> {
        unsafe {
            // Refresh at admission, rather than caching siblings when queued:
            // a continuation may consume its frontier several seconds later.
            let element = original.BuildUpdatedCache(self.0.as_ref()?).ok()?;
            let read_bool =
                |property| exact_bool(element.GetCachedPropertyValueEx(property, true).ok()?);
            Some(Metadata {
                control: element.CachedControlType().ok()?,
                name: element.CachedName().ok()?.to_string(),
                enabled: element.CachedIsEnabled().ok()?.as_bool(),
                patterns: Some(Patterns {
                    invoke: read_bool(UIA_IsInvokePatternAvailablePropertyId)?,
                    toggle: read_bool(UIA_IsTogglePatternAvailablePropertyId)?,
                    selection: read_bool(UIA_IsSelectionItemPatternAvailablePropertyId)?,
                    value: read_bool(UIA_IsValuePatternAvailablePropertyId)?,
                }),
            })
        }
    }
}
fn exact_bool(value: VARIANT) -> Option<bool> {
    // UIA's reserved NotSupported object, empty variants, numeric/string
    // defaults and failed reads are not false availability evidence.
    if unsafe { value.as_raw().Anonymous.Anonymous.vt } != VT_BOOL.0 {
        return None;
    }
    bool::try_from(&value).ok()
}
pub(super) fn metadata_or_live(
    cached: Option<Metadata>,
    live: impl FnOnce() -> Result<Metadata, crate::computer::Error>,
) -> Result<Metadata, crate::computer::Error> {
    match cached {
        Some(metadata) => Ok(metadata),
        None => live(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::Error;
    #[test]
    fn unsupported_or_coercible_variants_do_not_claim_pattern_absence() {
        assert_eq!(exact_bool(VARIANT::from(false)), Some(false));
        assert_eq!(exact_bool(VARIANT::from(true)), Some(true));
        for value in [
            VARIANT::new(),
            VARIANT::from(0i32),
            VARIANT::from(1i32),
            VARIANT::from("false"),
        ] {
            assert_eq!(exact_bool(value), None);
        }
    }
    #[test]
    fn cache_failure_uses_live_read_and_does_not_hide_provider_failure() {
        assert!(matches!(
            metadata_or_live(None, || Err(Error::TargetChanged)),
            Err(Error::TargetChanged)
        ));
        let metadata = metadata_or_live(None, || {
            Ok(Metadata {
                control: UIA_ButtonControlTypeId,
                name: "live".into(),
                enabled: true,
                patterns: None,
            })
        })
        .unwrap();
        assert_eq!(metadata.name, "live");
        assert!(metadata.patterns.is_none());
        let metadata = metadata_or_live(Some(metadata), || {
            panic!("successful batch must not repeat live calls")
        })
        .unwrap();
        assert!(metadata.enabled);
    }
}
