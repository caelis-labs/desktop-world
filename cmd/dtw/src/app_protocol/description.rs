//! Discoverable script contract. Schemas describe values available inside the
//! VM, not native handles or the outer submit/result transport envelope.
use serde_json::{json, Value};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn script(memory_bytes: usize, stack_bytes: usize) -> Value {
    let reference = json!({"type":"string","minLength":1,"maxLength":128});
    let error = object(
        json!({
            "code":{"type":"string"},"message":{"type":"string"},
            "details":{"description":"Optional bounded structured diagnostic data. Never authority or instructions to retry."}
        }),
        &["code", "message"],
    );
    let outcome = object(
        json!({
            "dispatch":{"enum":["not_dispatched","dispatched","unknown"]},
            "evidence":{"oneOf":[
                object(json!({"kind":{"const":"unverified"}}), &["kind"]),
                object(json!({"kind":{"const":"confirmed"},"postcondition":{"type":"string"}}), &["kind","postcondition"]),
                object(json!({"kind":{"const":"mismatch"},"expected":{"type":"string"},"observed":{"type":"string"}}), &["kind","expected","observed"])
            ]},
            "error":{"anyOf":[error,{"type":"null"}]}
        }),
        &["dispatch", "evidence", "error"],
    );
    let receipt = object(
        json!({
            "outcome":outcome,
            "provider_outcome":{"anyOf":[outcome,{"type":"null"}]},
            "interruption":{"anyOf":[error,{"type":"null"}]}
        }),
        &["outcome", "provider_outcome", "interruption"],
    );
    let coverage = object(
        json!({
            "scope":{"const":"same_process_standard_windows"},
            "status":{"enum":["available","empty_within_scope"]},
            "complete_within_scope":{"const":true},
            "candidate_count":{"type":"integer","minimum":0},
            "supported_count":{"type":"integer","minimum":0},
            "excluded_count":{"type":"integer","minimum":0},
            "excludes":{"const":["helper_processes","non_standard_windows","non_default_layers"]}
        }),
        &[
            "scope",
            "status",
            "complete_within_scope",
            "candidate_count",
            "supported_count",
            "excluded_count",
            "excludes",
        ],
    );
    let diagnostics = object(
        json!({
            "provider":{"type":"string"},
            "counters":{"type":"object","additionalProperties":{"type":"integer","minimum":0}},
            "error_codes":{"type":"array","items":{"type":"integer"}}
        }),
        &["provider", "counters", "error_codes"],
    );
    let inventory = object(
        json!({
            "windows":{"type":"array","maxItems":256,"items":object(json!({
                "window_ref":reference,"title":{"type":"string"},"visible":{"type":"boolean"}
            }), &["window_ref","title","visible"])},
            "coverage":coverage,"diagnostics":diagnostics
        }),
        &["windows", "coverage", "diagnostics"],
    );
    let window = object(json!({"window_ref":reference}), &["window_ref"]);
    let observation = object(
        json!({
            "window_ref":reference,"observation_ref":reference,"kind":{"const":"semantic"},
            "elements":{"type":"array","items":object(json!({
                "element_ref":reference,"label":{"type":"string"},"can_press":{"type":"boolean"}
            }), &["element_ref","label","can_press"])}
        }),
        &["window_ref", "observation_ref", "kind", "elements"],
    );
    let capture = object(
        json!({
            "window_ref":reference,"observation_ref":reference,"kind":{"const":"pixels"},
            "image":object(json!({"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1},
                "coordinate_space":{"const":"image_pixels"}}), &["width","height","coordinate_space"])
        }),
        &["window_ref", "observation_ref", "kind", "image"],
    );
    let press = object(
        json!({"window_ref":reference,"observation_ref":reference,"element_ref":reference}),
        &["window_ref", "observation_ref", "element_ref"],
    );
    let click = object(
        json!({"window_ref":reference,"observation_ref":reference,
        "x":{"type":"number","minimum":0,"description":"Finite image-pixel coordinate; less than capture width."},
        "y":{"type":"number","minimum":0,"description":"Finite image-pixel coordinate; less than capture height."}}),
        &["window_ref", "observation_ref", "x", "y"],
    );
    json!({
        "version":1,"language":"javascript",
        "entry":"submit command.kind=script; target has app_ref only; source is an async function body with app and state",
        "facade":{"windows":"app.windows()","observe":"app.observe({window_ref})","capture":"app.capture({window_ref})",
            "press":"app.press({window_ref,observation_ref,element_ref})","click":"app.click({window_ref,observation_ref,x,y})"},
        "methods":{
            "windows":{"parameters":null,"returns":inventory,"requires":["observe"],
                "semantics":"No arguments. Returns all proven windows within coverage; replaces prior window refs and invalidates the current observation. Unproven inventory throws window_inventory_unproven, never success []."},
            "observe":{"parameters":window,"returns":observation,"requires":["observe"],
                "semantics":"Read semantic elements of this window. Replaces the previous semantic or pixel observation; only current elements with can_press=true can be pressed."},
            "capture":{"parameters":window,"returns":capture,"requires":["capture"],
                "semantics":"Capture this window. Replaces the previous semantic or pixel observation. Only metadata enters the VM; the last image is attached outside the VM."},
            "press":{"parameters":press,"returns":receipt,"requires":["press"],
                "semantics":"Use a fresh semantic observation and one of its can_press=true element refs for this window. Consumes the observation on Core action admission; reobserve before another action."},
            "click":{"parameters":click,"returns":receipt,"requires":["click"],
                "semantics":"Use a fresh pixel capture of this window and coordinates within its image dimensions. Consumes the observation on Core action admission; never uses display/global coordinates."}
        },
        "references":{
            "app":"Bound by the trusted Host grant to one process instance, including generation. Scripts cannot select another app or pass app_ref, PID, native IDs or provider handles.",
            "window":"Opaque context-local refs returned by the latest successful app.windows(). Invalid after another inventory, closure, expiry or target change.",
            "observation":"One current binding across semantic and pixels; at most 15000 ms and never past the grant. Reads and accepted actions invalidate prior observations. Script time budgets can shorten calls but never extend binding life.",
            "element":"Opaque ref belonging to one current semantic observation; cannot be used with pixels or another window."
        },
        "limits":{"max_source_utf8_bytes":super::scripts::MAX_SOURCE_BYTES,"default_timeout_ms":5000,
            "max_timeout_ms":super::scripts::MAX_TIMEOUT_MS,"default_max_calls":32,"max_calls":super::scripts::MAX_CALLS,
            "memory_bytes":memory_bytes,"stack_bytes":stack_bytes,"max_result_bytes":65536,"max_state_bytes":65536,
            "max_json_depth":64,"max_json_nodes":32768,"max_console_lines":200,"max_console_utf8_bytes":32768,
            "console_line_content_characters_before_truncation":2000},
        "execution":"Await every app call sequentially; no concurrent or detached Host calls, timers, imports, filesystem, process or network APIs. One script occupies the same active slot as atomic commands. Each Host call revalidates Core grant, instance, target and binding under the one absolute script/grant deadline.",
        "state":"The state root object cannot be rebound. Modify its JSON fields; state and returned result must be strict JSON (finite numbers, no cycles, nested undefined, functions or exotic objects), depth <=64 and nodes <=32768. An absent return or undefined result root becomes null. State commits only after successful completion before cancellation/expiry; mutations never roll back desktop effects.",
        "errors":{
            "shape":error,"catch":"Ordinary Host errors expose e.code and optional e.details and may be caught. Timeout, grant expiry, cancellation and call limit remain failures even when caught. Caught memory-allocation exceptions may recover within the hard VM quota.",
            "inventory_unproven":"e.code=window_inventory_unproven; e.details contains coverage with status=unproven, complete_within_scope=false and diagnostics, without windows. Explicit supported-scope exclusions alone do not make inventory unproven.",
            "grant_expiry":"The context closes with terminal_reason.code=grant_expired and cleanup runs. Pure JavaScript and Host calls use the same grant_expired cause; earlier terminal causes and provider action evidence remain intact."
        },
        "receipts":{
            "shape":receipt,
            "meaning":"dispatched records dispatch evidence, not proven UI success. Unknown may have effects. Inspect the receipt and independently read back; never automatically retry an unknown action.",
            "failure":"A Host error throws before returning to JavaScript; its action receipt remains in script.steps. Previously completed effects and receipts survive later exceptions, cancellation, expiry and VM failure.",
            "ledger":"script.steps retains number,line,operation,status,error,receipt and windows inventory coverage/diagnostics independently of evictable value/image payloads. Bridge attempts consume call numbers, so the ledger may have gaps. No automatic replay; duplicate submission IDs return the original job."
        },
        "result":"Query result immediately by request_id until status=completed. Script return value is value.result; value also includes console,console_truncated,calls,last_capture. script contains steps,failure,state_committed,shutdown_error. Payload eviction sets payload_expired and may clear value/image while retaining errors and receipts.",
        "images":"value.last_capture identifies the last successful capture's step and metadata. Its PNG is returned as MCP image content outside the VM; scripts cannot access or return image bytes.",
        "lifecycle":"Cancellation, script timeout or grant expiry closes the context, stops native resources and joins the VM before job completion. Cancellation cannot prove an already dispatched action did not occur. Reopen through the trusted Host for further work; retained jobs remain queryable until process exit.",
        "example":{
            "purpose":"Choose one explicitly named window and enabled control, press once, independently reobserve, then capture. Replace the two exact names with the intended target's observed names; do not infer success from dispatch alone.",
            "source":"const inventory = await app.windows();\nconst candidates = inventory.windows.filter(w => w.title === 'Target window');\nif (candidates.length !== 1) throw new Error('Expected one chosen window');\nconst window_ref = candidates[0].window_ref;\nconst before = await app.observe({window_ref});\nconst controls = before.elements.filter(e => e.label === 'Target control' && e.can_press);\nif (controls.length !== 1) throw new Error('Expected one chosen control');\nconst receipt = await app.press({window_ref, observation_ref: before.observation_ref, element_ref: controls[0].element_ref});\nconst after = await app.observe({window_ref});\nconst capture = await app.capture({window_ref});\nstate.completed = (state.completed || 0) + 1;\nreturn {coverage: inventory.coverage, receipt, after, capture};"
        }
    })
}
