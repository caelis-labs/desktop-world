//! Real script/ledger/projection path with the controlled paged provider.
use super::*;
use crate::app_protocol::{
    projection,
    server::{Application, Factory, Server, Started},
};

const APP: &str = "org.example.Controlled";
const READ_ALL: &str = r#"
const window_ref=(await app.windows()).windows[0].window_ref;
let page=await app.observe({window_ref});
while(page.next) page=await app.observe({cursor:page.next});
"#;

#[derive(Clone)]
struct PagedCatalog(PagedBackend);
impl Factory for PagedCatalog {
    type Provider = PagedBackend;
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        Ok(vec![Application {
            app_id: APP.into(),
            app: application(),
            name: "Controlled pagination".into(),
        }])
    }
    fn connect(&self) -> Result<Self::Provider, Error> {
        Ok(self.0.clone())
    }
    fn close_catalog(&self) -> Result<(), Error> {
        Ok(())
    }
}

fn call(server: &Arc<Server<PagedCatalog>>, id: &str, name: &str, arguments: Value) -> Reply {
    match server.begin(json!(id), name, arguments) {
        Started::Reply(reply) => reply,
        Started::Call(call) => server.run(&call),
        Started::InFlight(_) => panic!("unexpected concurrent call"),
    }
}

fn discovery(visited: u32, terminal: bool) -> Value {
    json!({"complete":false,"reason":if terminal {Some("node_unavailable")} else {None},
        "visited":visited,"has_more":!terminal})
}

fn assert_discovery(actual: &Value, visited: u32, terminal: bool) {
    let mut coverage = actual.clone();
    for key in ["window_ref", "observation_ref"] {
        let reference = coverage.as_object_mut().unwrap().remove(key).unwrap();
        assert!(reference.as_str().is_some_and(|s| !s.is_empty()), "{key}");
    }
    assert_eq!(coverage, discovery(visited, terminal));
}

#[test]
fn terminal_incomplete_discovery_survives_js_discard_and_exception_in_public_results() {
    for throws in [false, true] {
        let (_, backend) = setup();
        let server = Server::new(PagedCatalog(backend.clone()));
        let tail = if throws {
            "if(!page.coverage.complete) throw new Error('Incomplete');"
        } else {
            "return 'discarded';"
        };
        let input = json!({"app":APP,"source":format!("{READ_ALL}{tail}")});
        let original = call(&server, "original", "exec", input.clone());
        assert_eq!(original.is_error, throws, "{}", original.data);
        assert!(original.data["execution_id"].is_string());
        let steps = original.data["script"]["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 4);
        assert!(steps[0].get("discovery").is_none());
        for (index, step) in steps[1..].iter().enumerate() {
            assert_discovery(&step["discovery"], index as u32 + 1, index == 2);
            for key in ["window_ref", "observation_ref"] {
                assert_eq!(step["discovery"][key], steps[1]["discovery"][key]);
            }
            assert_eq!(step["status"], "completed");
            assert!(step["error"].is_null());
        }
        let compact = projection::execution(original.clone());
        let summary = &compact.data["script"]["summary"];
        assert_eq!(summary["steps"], 4);
        assert_eq!(summary["failed"], 0);
        assert_eq!(summary["pending"], 0);
        assert_eq!(summary["incomplete_reads"], 1);
        assert_eq!(summary["last_issue"]["number"], 4);
        assert_eq!(summary["last_issue"]["discovery"], steps[3]["discovery"]);
        assert!(summary.get("recovered_incomplete_reads").is_none());
        assert_eq!(summary["last_issue"]["operation"], "observe");
        assert_eq!(summary["last_issue"]["status"], "completed");
        assert!(summary["last_issue"]["error"].is_null());
        assert!(summary["last_issue"].get("inventory").is_none());
        for key in [
            "execution_id",
            "status",
            "error",
            "value",
            "payload_expired",
        ] {
            assert_eq!(compact.data[key], original.data[key], "{key}");
        }
        for key in ["failure", "state_committed", "shutdown_error"] {
            assert_eq!(compact.data["script"][key], original.data["script"][key]);
        }
        assert_eq!(compact.is_error, throws);
        assert_eq!(original.data["script"]["state_committed"], !throws);
        assert!(compact.data["script"].get("steps").is_none());
        let retained = call(
            &server,
            "receipt",
            "result",
            json!({
                "app":APP,"execution_id":original.data["execution_id"]
            }),
        );
        assert_eq!(retained.data, original.data);
        assert_eq!(call(&server, "original", "exec", input).data, original.data);
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(backend.advances.load(Ordering::SeqCst), 2);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn unfinished_pagination_and_failed_continuation_are_not_terminal_incomplete_queries() {
    for fail_next in [false, true] {
        let (_, backend) = setup();
        backend.fail_next.store(fail_next, Ordering::SeqCst);
        let server = Server::new(PagedCatalog(backend.clone()));
        let source = format!(
            "const w=(await app.windows()).windows[0].window_ref;\
             const p=await app.observe({{window_ref:w}});\
             {} return 'discarded';",
            if fail_next {
                "try { await app.observe({cursor:p.next}); } catch(e) {}"
            } else {
                ""
            }
        );
        let original = call(
            &server,
            "original",
            "exec",
            json!({"app":APP,"source":source}),
        );
        assert!(!original.is_error, "{}", original.data);
        assert_discovery(&original.data["script"]["steps"][1]["discovery"], 1, false);
        let compact = projection::execution(original.clone());
        let summary = &compact.data["script"]["summary"];
        assert_eq!(summary["incomplete_reads"], 0);
        assert!(summary.get("recovered_incomplete_reads").is_none());
        assert_eq!(summary["failed"], u32::from(fail_next));
        if fail_next {
            assert_eq!(summary["last_issue"]["error"]["code"], "backend_failed");
            assert!(summary["last_issue"].get("discovery").is_none());
            assert!(original.data["script"]["steps"][2]
                .get("discovery")
                .is_none());
        } else {
            assert!(summary.get("last_issue").is_none());
        }
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(
            backend.advances.load(Ordering::SeqCst),
            usize::from(fail_next)
        );
    }
}

#[test]
fn terminal_discovery_metadata_survives_real_result_payload_eviction() {
    let (mut owner, backend) = setup();
    Arc::get_mut(&mut owner).unwrap().max_payload_bytes = 0;
    let original = script_run(&owner, "evicted", &format!("{READ_ALL}return 'discarded';"));
    assert!(!original.is_error, "{}", original.data);
    assert_eq!(original.data["payload_expired"], true);
    assert!(original.data["value"].is_null());
    assert_discovery(&original.data["script"]["steps"][3]["discovery"], 3, true);
    assert_eq!(result(&owner, "evicted").data, original.data);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 2);
}

#[test]
fn fresh_complete_query_records_recovery_with_exact_public_refs_within_one_execution() {
    let (_, backend) = setup();
    backend.complete_after_first.store(true, Ordering::SeqCst);
    let server = Server::new(PagedCatalog(backend.clone()));
    let source = format!(
        "{READ_ALL}\
        const fresh=await app.observe({{window_ref}});\
        return {{window_ref,old:page.observation_ref,fresh:fresh.observation_ref}};"
    );
    let original = call(
        &server,
        "original",
        "exec",
        json!({"app":APP,"source":source}),
    );
    assert!(!original.is_error, "{}", original.data);
    let refs = &original.data["value"]["result"];
    assert_ne!(refs["old"], refs["fresh"]);
    let steps = original.data["script"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 5);
    for index in [1, 2, 3, 4] {
        assert_eq!(steps[index]["discovery"]["window_ref"], refs["window_ref"]);
        assert_eq!(
            &steps[index]["discovery"]["observation_ref"],
            if index == 4 {
                &refs["fresh"]
            } else {
                &refs["old"]
            }
        );
    }
    assert_eq!(steps[4]["discovery"]["complete"], true);
    assert_eq!(steps[4]["discovery"]["has_more"], false);
    let compact = projection::execution(original.clone());
    let summary = &compact.data["script"]["summary"];
    assert_eq!(summary["incomplete_reads"], 1);
    assert_eq!(summary["recovered_incomplete_reads"], 1);
    assert_eq!(summary["last_issue"]["discovery"], steps[3]["discovery"]);
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["pending"], 0);
    assert_eq!(
        call(
            &server,
            "result",
            "result",
            json!({
                "app":APP,"execution_id":original.data["execution_id"]
            })
        )
        .data,
        original.data
    );

    let later = call(
        &server,
        "later",
        "exec",
        json!({"app":APP,"source":
        "const w=(await app.windows()).windows[0].window_ref;return await app.observe({window_ref:w});"}),
    );
    assert!(!later.is_error, "{}", later.data);
    assert_ne!(later.data["execution_id"], original.data["execution_id"]);
    assert_eq!(
        later.data["script"]["steps"][1]["discovery"]["window_ref"],
        refs["window_ref"]
    );
    let later_compact = projection::execution(later);
    assert_eq!(
        later_compact.data["script"]["summary"]["incomplete_reads"],
        0
    );
    assert!(later_compact.data["script"]["summary"]
        .get("recovered_incomplete_reads")
        .is_none());
    assert_eq!(backend.queries.lock().unwrap().len(), 3);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 2);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}
