//! Controlled pagination contract tests; native traversal is accepted separately.
use super::*;

#[path = "discovery_evidence_tests.rs"]
mod discovery_evidence_tests;

#[derive(Clone)]
struct PagedBackend {
    base: MockBackend,
    queries: Arc<Mutex<Vec<ObserveQuery>>>,
    advances: Arc<AtomicUsize>,
    fail_next: Arc<std::sync::atomic::AtomicBool>,
    complete_after_first: Arc<std::sync::atomic::AtomicBool>,
}
impl Backend for PagedBackend {
    type Binding = AtomicUsize;
    fn stamp(&self) -> ProviderStamp {
        self.base.stamp()
    }
    fn supports(&self, capability: Capability) -> bool {
        self.base.supports(capability)
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        self.base.windows(app, call)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.observe_page(target, &ObserveQuery::default(), call)
    }
    fn observe_page(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        _: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        let complete = {
            let mut queries = self.queries.lock().unwrap();
            queries.push(query.clone());
            queries.len() > 1 && self.complete_after_first.load(Ordering::SeqCst)
        };
        let mut read = self.base.read(target, false);
        read.elements[0].id = ElementId(1);
        Ok(BackendObservation {
            target: read.target,
            elements: read.elements,
            image: None,
            binding: AtomicUsize::new(0),
            discovery: Some(DiscoveryPage {
                has_more: !complete,
                complete,
                reason: None,
                visited: 1,
            }),
        })
    }
    fn continue_observation(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        _: &CallContext<'_>,
    ) -> Result<BackendObservationPage, Error> {
        self.advances.fetch_add(1, Ordering::SeqCst);
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(Error::BackendFailed(
                "controlled continuation failure".into(),
            ));
        }
        let page = binding.fetch_add(1, Ordering::SeqCst) + 1;
        let mut read = self.base.read(target, false);
        read.elements[0].id = ElementId(page as u64 + 1);
        Ok(BackendObservationPage {
            target: target.clone(),
            elements: read.elements,
            discovery: DiscoveryPage {
                has_more: page < 2,
                complete: false,
                reason: (page == 2).then(|| "node_unavailable".into()),
                visited: page as u32 + 1,
            },
        })
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        let read = self.base.capture(target, call)?;
        Ok(BackendObservation {
            target: read.target,
            elements: read.elements,
            image: read.image,
            binding: AtomicUsize::new(0),
            discovery: None,
        })
    }
    fn press(
        &self,
        target: &WindowTarget,
        _: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.base.press(target, &(), element, call)
    }
}
fn setup() -> (Arc<Coordinator<PagedBackend>>, PagedBackend) {
    let backend = PagedBackend {
        base: MockBackend::new(),
        queries: Arc::new(Mutex::new(vec![])),
        advances: Arc::new(AtomicUsize::new(0)),
        fail_next: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        complete_after_first: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    (
        Coordinator::new(backend.clone(), application(), "Paged".into()),
        backend,
    )
}
fn script_run(owner: &Arc<Coordinator<PagedBackend>>, id: &str, source: &str) -> Reply {
    run(
        owner,
        id,
        None,
        Command::Script {
            source: source.into(),
            timeout_ms: 5000,
            max_calls: 0,
        },
    )
}

#[test]
fn undefined_optional_filters_reach_core_as_omitted_filters_through_all_pages() {
    let (owner, backend) = setup();
    let reply = script_run(
        &owner,
        "undefined-filters",
        r#"
const window_ref=(await app.windows()).windows[0].window_ref;
async function collect(args) {
  const first=await app.observe({window_ref,limit:12,...args});
  let page=first, count=page.elements.length;
  while(page.next) {
    page=await app.observe({cursor:page.next});
    count+=page.elements.length;
  }
  return {count,sameQuery:page.observation_ref===first.observation_ref,next:page.next};
}
return [await collect({role:undefined,text:'Increment'}),await collect({text:'Increment'}),
        await collect({role:undefined,text:undefined}),await collect({})];
"#,
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(
        reply.data["value"]["result"],
        json!(vec![json!({"count":3,"sameQuery":true,"next":null}); 4])
    );
    let queries = backend.queries.lock().unwrap();
    assert_eq!(queries.len(), 4);
    assert_eq!(
        queries[0],
        ObserveQuery {
            role: None,
            text: Some("Increment".into()),
            limit: 12
        }
    );
    assert_eq!(queries[1], queries[0]);
    assert_eq!(
        queries[2],
        ObserveQuery {
            role: None,
            text: None,
            limit: 12
        }
    );
    assert_eq!(queries[3], queries[2]);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 8);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn scripts_normalize_empty_text_before_core_and_keep_the_query_paging_binding() {
    let (owner, backend) = setup();
    let reply = script_run(
        &owner,
        "empty-text",
        r#"
const window_ref=(await app.windows()).windows[0].window_ref;
async function collect(extra) {
  const first=await app.observe({window_ref,role:'button',limit:1,...extra});
  let last=first, count=first.elements.length;
  while(last.next) {
    last=await app.observe({cursor:last.next});
    count+=last.elements.length;
  }
  await app.press({window_ref,observation_ref:first.observation_ref,
                  element_ref:first.elements[0].element_ref});
  return {count,sameQuery:first.observation_ref===last.observation_ref,next:last.next};
}
return {empty:await collect({text:''}),absent:await collect({})};
"#,
    );
    assert!(!reply.is_error, "{}", reply.data);
    let expected = json!({"count":3,"sameQuery":true,"next":null});
    assert_eq!(
        reply.data["value"]["result"],
        json!({"empty":expected,"absent":expected})
    );
    assert_eq!(
        *backend.queries.lock().unwrap(),
        vec![
            ObserveQuery {
                role: Some("button".into()),
                text: None,
                limit: 1,
            };
            2
        ]
    );
    assert_eq!(backend.advances.load(Ordering::SeqCst), 4);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
}

#[test]
fn scripts_keep_earlier_page_and_other_window_refs_across_capture_and_inventory() {
    let (owner, backend) = setup();
    let reply = script_run(
        &owner,
        "paged",
        r#"
const ws = (await app.windows()).windows;
const w = ws[0].window_ref, peer = ws[1].window_ref;
const first = await app.observe({window_ref:w, role:'button', text:'Increment', limit:1});
const other = await app.observe({window_ref:peer});
await app.capture({window_ref:w});
const sameWindow = (await app.windows()).windows[0].window_ref === w;
const second = await app.observe({cursor:first.next});
let stale;
try { await app.observe({cursor:first.next}); } catch(e) { stale=e.code; }
const last = await app.observe({cursor:second.next});
await app.press({window_ref:w, observation_ref:first.observation_ref, element_ref:first.elements[0].element_ref});
await app.press({window_ref:peer, observation_ref:other.observation_ref, element_ref:other.elements[0].element_ref});
return {sameWindow, sameQuery:first.observation_ref===last.observation_ref,
        distinctElements:first.elements[0].element_ref!==last.elements[0].element_ref,
        stale, next:last.next, coverage:last.coverage};
"#,
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(
        reply.data["value"]["result"],
        json!({"sameWindow":true,"sameQuery":true,"distinctElements":true,"stale":"stale_cursor","next":null,"coverage":{"complete":false,"reason":"node_unavailable","visited":3}})
    );
    assert_eq!(backend.advances.load(Ordering::SeqCst), 2);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
    let state = owner.state.lock().unwrap();
    assert!(
        state.observations.iter().all(|snapshot| snapshot.retired
            && snapshot.elements.capacity() == 0
            && snapshot.cursor.is_none()),
        "Consumed queries must release the public element map as well as Core/native bindings"
    );
    assert_eq!(
        backend.queries.lock().unwrap()[0],
        ObserveQuery {
            role: Some("button".into()),
            text: Some("Increment".into()),
            limit: 1
        }
    );
}

#[test]
fn continuation_failure_retires_the_query_without_replaying_or_consuming_another_window() {
    let (owner, backend) = setup();
    backend.fail_next.store(true, Ordering::SeqCst);
    let reply = script_run(
        &owner,
        "failed-page",
        r#"
const ws=(await app.windows()).windows;
const w=ws[0].window_ref, peer=ws[1].window_ref;
const first=await app.observe({window_ref:w});
const other=await app.observe({window_ref:peer});
let failed, retry, old;
try { await app.observe({cursor:first.next}); } catch(e) { failed=e.code; }
try { await app.observe({cursor:first.next}); } catch(e) { retry=e.code; }
try { await app.press({window_ref:w,observation_ref:first.observation_ref,element_ref:first.elements[0].element_ref}); } catch(e) { old=e.code; }
await app.press({window_ref:peer,observation_ref:other.observation_ref,element_ref:other.elements[0].element_ref});
return {failed,retry,old};
"#,
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(
        reply.data["value"]["result"],
        json!({"failed":"backend_failed","retry":"stale_cursor","old":"invalid_observation"})
    );
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn cursors_are_context_local_and_cannot_be_combined_with_new_query_parameters() {
    let (first, backend) = setup();
    let window = windows(&first, "windows").remove(0);
    let page = run(&first, "first", Some(&window), Command::Observe);
    let cursor = page.data["value"]["next"].as_str().unwrap();
    let (second, other_backend) = setup();
    let foreign = run(
        &second,
        "foreign",
        None,
        Command::ContinueObservation {
            cursor: cursor.into(),
        },
    );
    assert_code(&foreign, "stale_cursor");
    for args in [
        json!({"cursor":cursor,"window_ref":window}),
        json!({"cursor":cursor,"text":"x"}),
        json!({"cursor":cursor,"limit":1}),
    ] {
        let reply = script_run(
            &first,
            &reference("bad"),
            &format!("return await app.observe({args});"),
        );
        assert!(reply.is_error, "{}", reply.data);
    }
    assert_eq!(backend.advances.load(Ordering::SeqCst), 0);
    assert_eq!(other_backend.advances.load(Ordering::SeqCst), 0);
    let valid = run(
        &first,
        "continue",
        None,
        Command::ContinueObservation {
            cursor: cursor.into(),
        },
    );
    assert!(!valid.is_error, "{}", valid.data);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
}
