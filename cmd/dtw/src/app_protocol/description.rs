//! Discoverable script contract. Schemas describe values available inside the
//! VM, not native handles or the outer submit/result transport envelope.
use super::methods::Method;
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
                "element_ref":reference,"label":{"type":"string"},"can_press":{"type":"boolean"},
                "role":{"type":"string"},"name":{"type":["string","null"]},"value":{"type":["string","null"]},
                "enabled":{"type":["boolean","null"]},"actions":{"type":"array","items":{"type":"string"}}
            }), &["element_ref","label","can_press","role","name","value","enabled","actions"])}
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
        "entry":"exec({app: 'com.example.App', source}); source is an async function body with app and state",
        "facade":{"windows":"app.windows()","observe":"app.observe({window_ref})","capture":"app.capture({window_ref})",
            "press":"app.press({window_ref,observation_ref,element_ref})","click":"app.click({window_ref,observation_ref,x,y})"},
        "methods":{
            (Method::Windows.name()):{"parameters":null,"returns":inventory,"requires":[Method::Observe.capability().as_str()],
                "semantics":"No arguments. Returns all proven windows within coverage; replaces prior window refs and invalidates the current observation. Unproven inventory throws window_inventory_unproven, never success []."},
            (Method::Observe.name()):{"parameters":window,"returns":observation,"requires":[Method::Observe.capability().as_str()],
                "semantics":"Read semantic elements of this window. Replaces the previous semantic or pixel observation; only current elements with can_press=true can be pressed."},
            (Method::Capture.name()):{"parameters":window,"returns":capture,"requires":[Method::Capture.capability().as_str()],
                "semantics":"Capture this window. Replaces the previous semantic or pixel observation. Only metadata enters the VM; the last image is attached outside the VM."},
            (Method::Press.name()):{"parameters":press,"returns":receipt,"requires":[Method::Press.capability().as_str()],
                "semantics":"Use a fresh semantic observation and one of its can_press=true element refs for this window. Consumes the observation on Core action admission; reobserve before another action."},
            (Method::Click.name()):{"parameters":click,"returns":receipt,"requires":[Method::Click.capability().as_str()],
                "semantics":"Use a fresh pixel capture of this window and coordinates within its image dimensions. Consumes the observation on Core action admission; never uses display/global coordinates."}
        },
        "references":{
            "app":"Selected by the exec app identifier; DTW binds the live instance internally. Scripts cannot select another app or pass app_ref, PID, native IDs or provider handles.",
            "window":"Opaque context-local refs returned by the latest successful app.windows(). Invalid after another inventory, context retirement or target change.",
            "observation":"One current binding across semantic and pixels; at most 15000 ms. Reads and accepted actions invalidate prior observations. Script time budgets can shorten calls but never extend binding life.",
            "element":"Opaque ref belonging to one current semantic observation; cannot be used with pixels or another window."
        },
        "limits":{"max_source_utf8_bytes":super::scripts::MAX_SOURCE_BYTES,"default_timeout_ms":super::scripts::DEFAULT_TIMEOUT_MS,
            "max_calls":null,
            "step_receipt_admission_bytes":super::scripts::MAX_STEP_RECEIPT_BYTES,
            "memory_bytes":memory_bytes,"stack_bytes":stack_bytes,"max_result_bytes":65536,"max_state_bytes":65536,
            "max_unhandled_rejections":super::script_engine::MAX_UNHANDLED_REJECTIONS,"max_json_depth":64,"max_json_nodes":32768,"max_console_lines":200,"max_console_utf8_bytes":32768,
            "console_line_content_characters_before_truncation":2000},
        "execution":"Await every app call sequentially; no concurrent or detached Host calls, timers, imports, filesystem, process or network APIs. One script executes at a time per MCP connection. Each Host call revalidates Core instance, target and binding under one absolute script deadline.",
        "state":"The state root object cannot be rebound. Modify its JSON fields; state and returned result must be strict JSON (finite numbers, no cycles, nested undefined, functions or exotic objects), depth <=64 and nodes <=32768. An absent return or undefined result root becomes null. State commits only after successful completion before cancellation/timeout; mutations never roll back desktop effects.",
        "errors":{
            "shape":error,"catch":"Ordinary Host errors expose e.code and optional e.details and may be caught. Timeout, cancellation and call limit remain failures even when caught. Unhandled Promise rejections fail the script without committing state. Caught memory-allocation exceptions may recover within the hard VM quota.",
            "inventory_unproven":"e.code=window_inventory_unproven; e.details contains coverage with status=unproven, complete_within_scope=false and diagnostics, without windows. Explicit supported-scope exclusions alone do not make inventory unproven."
        },
        "receipts":{
            "shape":receipt,
            "meaning":"dispatched records dispatch evidence, not proven UI success. Unknown may have effects. Inspect the receipt and independently read back; never automatically retry an unknown action.",
            "failure":"A Host error throws before returning to JavaScript; its action receipt remains in script.steps. Previously completed effects and receipts survive later exceptions, cancellation, timeout and VM failure.",
            "ledger":"script.steps retains number,line,operation,status,error,receipt and windows inventory coverage/diagnostics independently of evictable value/image payloads. Bridge attempts consume call numbers, so the ledger may have gaps. No automatic replay. DTW keeps the original ledger; repeated accepted MCP transport IDs refer to the same execution, while new requests execute new work."
        },
        "result":"exec waits until completion and returns value.result plus unchanged console,console_truncated,calls,last_capture. script.summary replaces the step ledger with counts and a last_issue sample; script.failure,state_committed,shutdown_error and all recovery flags remain. result({app}) retrieves the full original record with script.steps; optional execution_id selects a prior returned record. Summary dispatch/evidence counts do not prove business success. Caught errors may coexist with state_committed=true; consult the full ledger for all issues, never replay. Payload eviction sets payload_expired and may clear value/image while retaining errors and receipts.",
        "images":"value.last_capture identifies the last successful capture's step and metadata. Its PNG is returned as MCP image content outside the VM; scripts cannot access or return image bytes.",
        "lifecycle":"Cancellation or script timeout closes the context, stops native resources and joins the VM before job completion. Cancellation cannot prove an already dispatched action did not occur. A later new exec rebuilds affected native references as needed (context_reset) while retaining committed JSON state. Only an actual application instance change clears state (state_reset). Retained jobs remain queryable within this MCP connection.",
        "example":{
            "purpose":"Choose one explicitly named window and enabled control, press once, independently reobserve, then capture. Replace the two exact names with the intended target's observed names; do not infer success from dispatch alone.",
            "source":"const inventory = await app.windows();\nconst candidates = inventory.windows.filter(w => w.title === 'Target window');\nif (candidates.length !== 1) throw new Error('Expected one chosen window');\nconst window_ref = candidates[0].window_ref;\nconst before = await app.observe({window_ref});\nconst controls = before.elements.filter(e => e.label === 'Target control' && e.can_press);\nif (controls.length !== 1) throw new Error('Expected one chosen control');\nconst receipt = await app.press({window_ref, observation_ref: before.observation_ref, element_ref: controls[0].element_ref});\nconst after = await app.observe({window_ref});\nconst capture = await app.capture({window_ref});\nstate.completed = (state.completed || 0) + 1;\nreturn {coverage: inventory.coverage, receipt, after, capture};"
        }
    })
}

/// Generate the distributable declaration from discoverable return/argument schemas.
pub(super) fn typescript() -> String {
    let spec = super::scripts::description();
    let mut out =
        String::from("// Generated by dtw api. Edit the Rust script schemas, then regenerate.\n\n");
    out.push_str("type Json = null | boolean | number | string | Json[] | { [key: string]: Json };\n\ninterface App {\n");
    for method in Method::ALL {
        let shape = &spec["methods"][method.name()];
        let args = if shape["parameters"].is_null() {
            String::new()
        } else {
            format!("args: {}", ts_type(&shape["parameters"]))
        };
        out.push_str(&format!(
            "  {}({}): Promise<{}>;\n",
            method.name(),
            args,
            ts_type(&shape["returns"])
        ));
    }
    out.push_str("}\ndeclare const app: Readonly<App>;\n/** JSON data persists per session and app instance, independently of native workers; commits on successful completion only. */\ndeclare const state: { [key: string]: Json };\n");
    out
}
fn ts_type(schema: &Value) -> String {
    if let Some(value) = schema.get("const") {
        return value.to_string();
    }
    for key in ["enum", "anyOf", "oneOf"] {
        if let Some(items) = schema[key].as_array() {
            return items
                .iter()
                .map(|v| {
                    if key == "enum" {
                        v.to_string()
                    } else {
                        ts_type(v)
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
        }
    }
    if let Some(types) = schema["type"].as_array() {
        return types
            .iter()
            .map(|v| ts_type(&json!({"type":v})))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match schema["type"].as_str() {
        Some("string") => "string".into(),
        Some("number" | "integer") => "number".into(),
        Some("boolean") => "boolean".into(),
        Some("null") => "null".into(),
        Some("array") => format!("({})[]", ts_type(&schema["items"])),
        Some("object") => {
            let mut fields = Vec::new();
            if let Some(properties) = schema["properties"].as_object() {
                for (name, value) in properties {
                    let required = schema["required"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|v| v == name));
                    fields.push(format!(
                        "{}{}: {}",
                        name,
                        if required { "" } else { "?" },
                        ts_type(value)
                    ));
                }
            }
            if let Some(extra) = schema.get("additionalProperties").filter(|v| v.is_object()) {
                fields.push(format!("[key: string]: {}", ts_type(extra)));
            }
            if fields.is_empty() {
                "{ [key: string]: Json }".into()
            } else {
                format!("{{ {} }}", fields.join("; "))
            }
        }
        _ => "Json".into(),
    }
}
