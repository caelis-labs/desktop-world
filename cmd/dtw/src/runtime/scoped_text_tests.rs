use super::*;
use std::sync::atomic::AtomicBool;
#[path = "../backends/text_page.rs"]
mod text_page;

struct RichBackend {
    inner: PixelBackend,
    fail_text: AtomicBool,
    unsupported_text: AtomicBool,
    scopes: Mutex<Vec<ObserveScope>>,
    advance_scope: Mutex<Option<ManualClock>>,
}
impl Backend for RichBackend {
    type Binding = NativeBinding;
    fn stamp(&self) -> ProviderStamp {
        self.inner.stamp()
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn supports_observe_scope(&self, scope: ObserveScopeKind) -> bool {
        scope != ObserveScopeKind::Menu
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.inner.observe(target, call)
    }
    fn observe_scoped(
        &self,
        target: &WindowTarget,
        _: &ObserveQuery,
        scope: ObserveScope,
        binding: Option<&Self::Binding>,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        assert_eq!(binding.is_some(), scope.observation_kind().is_some());
        self.scopes.lock().unwrap().push(scope);
        if let Some(clock) = self.advance_scope.lock().unwrap().take() {
            clock.advance(Duration::from_secs(2));
        }
        let mut read = self.inner.observe(target, call)?;
        read.discovery = Some(DiscoveryPage {
            has_more: false,
            complete: false,
            reason: Some("native_read_gap".into()),
            visited: 1,
        });
        Ok(read)
    }
    fn read_text(
        &self,
        target: &WindowTarget,
        _: &Self::Binding,
        request: TextRequest,
        call: &CallContext<'_>,
    ) -> Result<BackendTextPage, Error> {
        call.remaining()?;
        if self.unsupported_text.load(Ordering::SeqCst) {
            return Err(Error::UnsupportedCapability);
        }
        if self.fail_text.load(Ordering::SeqCst) {
            return Err(Error::StaleObservation);
        }
        Ok(BackendTextPage {
            target: target.clone(),
            page: text_page::page(
                &format!("{}😀end", "content".repeat(300)),
                request.offset,
                request.limit,
            )?,
        })
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.inner.capture(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.inner.press(target, binding, element, call)
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        point: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.inner.click(target, binding, point, call)
    }
}
fn setup() -> Runtime<RichBackend> {
    Runtime::new(
        RichBackend {
            inner: PixelBackend(TestBackend::new()),
            fail_text: AtomicBool::new(false),
            unsupported_text: AtomicBool::new(false),
            scopes: Mutex::new(vec![]),
            advance_scope: Mutex::new(None),
        },
        target().app,
        Duration::from_secs(30),
    )
}
fn until() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn request(offset: u32) -> TextRequest {
    TextRequest {
        element: ElementId(7),
        offset,
        limit: 256,
    }
}

#[test]
fn a_derived_query_cannot_outlive_its_source_during_native_resolution() {
    let clock = ManualClock::new();
    let runtime = Runtime::with_clock(
        RichBackend {
            inner: PixelBackend(TestBackend::new()),
            fail_text: AtomicBool::new(false),
            unsupported_text: AtomicBool::new(false),
            scopes: Mutex::new(vec![]),
            advance_scope: Mutex::new(None),
        },
        target().app,
        Duration::from_secs(3),
        clock.clone(),
    );
    let pixels = runtime.capture(&target()).unwrap();
    clock.advance(Duration::from_secs(2));
    *runtime.backend.advance_scope.lock().unwrap() = Some(clock.clone());
    assert_eq!(
        runtime.observe_scoped_until(
            &target(),
            &ObserveQuery::default(),
            ObserveScope::Region(PixelRegion {
                x: 0.,
                y: 0.,
                width: 10.,
                height: 10.
            }),
            Some(pixels.reference),
            clock.now() + Duration::from_secs(10)
        ),
        Err(Error::StaleObservation)
    );
}

#[test]
fn region_references_are_pixel_bound_and_success_preserves_capture_and_other_window() {
    let runtime = setup();
    let target = target();
    let mut peer = target.clone();
    peer.native_id += 1;
    let pixels = runtime.capture(&target).unwrap();
    let other = runtime.capture(&peer).unwrap();
    let original = runtime.observe(&target).unwrap();
    let region = ObserveScope::Region(PixelRegion {
        x: 10.,
        y: 10.,
        width: 20.,
        height: 30.,
    });
    assert_eq!(
        runtime.observe_scoped_until(
            &target,
            &ObserveQuery::default(),
            region,
            Some(original.reference),
            until()
        ),
        Err(Error::InvalidObservation)
    );
    assert_eq!(
        runtime.observe_scoped_until(
            &peer,
            &ObserveQuery::default(),
            region,
            Some(pixels.reference),
            until()
        ),
        Err(Error::TargetChanged)
    );
    assert_eq!(
        runtime.observe_scoped_until(
            &target,
            &ObserveQuery::default(),
            ObserveScope::Region(PixelRegion {
                x: 90.,
                y: 0.,
                width: 11.,
                height: 1.
            }),
            Some(pixels.reference),
            until()
        ),
        Err(Error::InvalidPoint)
    );
    let scoped = runtime
        .observe_scoped_until(
            &target,
            &ObserveQuery::default(),
            region,
            Some(pixels.reference),
            until(),
        )
        .unwrap();
    assert!(!scoped.discovery.unwrap().complete);
    assert!(runtime
        .read_text_until(&target, original.reference, request(0), until())
        .is_err());
    assert!(runtime
        .read_text_until(&target, scoped.reference, request(0), until())
        .is_ok());
    assert!(runtime
        .click(&target, pixels.reference, PixelPoint { x: 1., y: 1. })
        .outcome
        .error
        .is_none());
    assert!(runtime
        .click(&peer, other.reference, PixelPoint { x: 1., y: 1. })
        .outcome
        .error
        .is_none());
}

#[test]
fn subtree_retires_source_after_native_read_and_unsupported_scope_does_not_consume() {
    let runtime = setup();
    let original = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime.observe_scoped_until(
            &target(),
            &ObserveQuery::default(),
            ObserveScope::Menu,
            None,
            until()
        ),
        Err(Error::UnsupportedCapability)
    );
    assert!(runtime
        .read_text_until(&target(), original.reference, request(0), until())
        .is_ok());
    let scoped = runtime
        .observe_scoped_until(
            &target(),
            &ObserveQuery::default(),
            ObserveScope::Subtree(ElementId(7)),
            Some(original.reference),
            until(),
        )
        .unwrap();
    assert!(runtime
        .read_text_until(&target(), original.reference, request(0), until())
        .is_err());
    assert!(runtime
        .read_text_until(&target(), scoped.reference, request(0), until())
        .is_ok());
}

#[test]
fn text_pages_keep_one_reference_and_failed_read_retires_only_semantics() {
    let runtime = setup();
    let pixels = runtime.capture(&target()).unwrap();
    let semantic = runtime.observe(&target()).unwrap();
    runtime
        .backend
        .unsupported_text
        .store(true, Ordering::SeqCst);
    assert_eq!(
        runtime.read_text_until(&target(), semantic.reference, request(0), until()),
        Err(Error::UnsupportedCapability)
    );
    runtime
        .backend
        .unsupported_text
        .store(false, Ordering::SeqCst);
    assert!(
        matches!(runtime.read_text_until(&target(),semantic.reference,request(2101),until()),Err(Error::NativeFailure{code,..}) if code=="invalid_text_range")
    );
    let mut offset = 0;
    let mut text = String::new();
    loop {
        let page = runtime
            .read_text_until(&target(), semantic.reference, request(offset), until())
            .unwrap();
        text.push_str(&page.text);
        let Some(next) = page.next_offset else { break };
        offset = next;
    }
    assert_eq!(text, format!("{}😀end", "content".repeat(300)));
    runtime.backend.fail_text.store(true, Ordering::SeqCst);
    assert_eq!(
        runtime.read_text_until(&target(), semantic.reference, request(0), until()),
        Err(Error::StaleObservation)
    );
    runtime.backend.fail_text.store(false, Ordering::SeqCst);
    assert!(runtime
        .read_text_until(&target(), semantic.reference, request(0), until())
        .is_err());
    assert!(runtime
        .click(&target(), pixels.reference, PixelPoint { x: 1., y: 1. })
        .outcome
        .error
        .is_none());
}
