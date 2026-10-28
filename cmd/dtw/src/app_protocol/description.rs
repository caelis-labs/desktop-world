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
            "scope":{"enum":["same_process_standard_windows","same_process_layer_zero_surfaces","application_instances"]},
            "status":{"enum":["available","empty_within_scope","partial"]},
            "complete_within_scope":{"type":"boolean"},
            "candidate_count":{"type":["integer","null"],"minimum":0},
            "supported_count":{"type":"integer","minimum":0},
            "excluded_count":{"type":["integer","null"],"minimum":0},
            "excludes":{"type":"array","items":{"type":"string"}},
            "instance_count":{"type":"integer","minimum":1,"maximum":32},
            "available_instances":{"type":"integer","minimum":0,"maximum":32},
            "unavailable_instances":{"type":"integer","minimum":0,"maximum":32}
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
            "error_codes":{"type":"array","items":{"type":"integer"}},
            "errors":{"type":"array","maxItems":32,"items":object(json!({"code":{"type":"string"},"message":{"type":"string"},"details":{},"message_truncated":{"type":"boolean"}}), &["code","message"])}
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
    let query = json!({"oneOf":[
        object(json!({"window_ref":reference,
            "role":{"type":"string","minLength":1,"maxLength":64,"description":"Exact returned semantic role, for example button."},
            "text":{"type":"string","minLength":1,"maxLength":256,"description":"Case-insensitive substring of label, name or value. Combined with role using AND."},
            "limit":{"type":"integer","minimum":1,"maximum":100,"default":40}
        }), &["window_ref"]),
        object(json!({"cursor":reference}), &["cursor"])
    ]});
    let observation = object(
        json!({
            "window_ref":reference,"observation_ref":reference,"kind":{"const":"semantic"},
            "next":{"anyOf":[reference,{"type":"null"}]},
            "coverage":object(json!({"complete":{"type":"boolean"},"reason":{"type":["string","null"]},"visited":{"type":"integer","minimum":0,"maximum":8192}}), &["complete","reason","visited"]),
            "elements":{"type":"array","items":object(json!({
                "element_ref":reference,"label":{"type":"string"},"can_press":{"type":"boolean"},
                "role":{"type":"string"},"name":{"type":["string","null"],"description":"Bounded native name excerpt, or null when unavailable."},"value":{"type":["string","null"],"description":"Bounded native value excerpt, or null when unavailable; not a complete document read."},
                "enabled":{"type":["boolean","null"]},"actions":{"type":"array","items":{"type":"string"}}
            }), &["element_ref","label","can_press","role","name","value","enabled","actions"])}
        }),
        &[
            "window_ref",
            "observation_ref",
            "kind",
            "elements",
            "next",
            "coverage",
        ],
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
        "y":{"type":"number","minimum":0,"description":"Finite image-pixel coordinate; less than capture height."},
        "button":{"enum":["left","right"],"default":"left"},"count":{"type":"integer","minimum":1,"maximum":2,"default":1}}),
        &["window_ref", "observation_ref", "x", "y"],
    );
    json!({
        "version":1,"language":"javascript",
        "entry":"exec({app: 'com.example.App', source}); source is an async function body with app and state",
        "facade":{"windows":"app.windows()","observe":"app.observe({window_ref, role?, text?, limit?}) or app.observe({cursor})","capture":"app.capture({window_ref})",
            "press":"app.press({window_ref,observation_ref,element_ref})","click":"app.click({window_ref,observation_ref,x,y,button?,count?})",
            "typeText":"app.typeText({window_ref,text})","keypress":"app.keypress({window_ref,keys})","scroll":"app.scroll({window_ref,delta_x?,delta_y?})",
            "focus":"app.focus({window_ref})","setValue":"app.setValue({window_ref,observation_ref,element_ref,value})"},
        "methods":{
            (Method::Windows.name()):{"parameters":null,"returns":inventory,"requires":[Method::Windows.capability().as_str()],
                "semantics":"No arguments. Aggregates proven windows from the app instances discovered before this exec. Each window_ref routes to its exact native instance. Unchanged targets keep refs and observations; proven removed targets retire theirs. A failed instance does not discard proven sibling windows: partial coverage reports unavailable_instances and bounded errors. When no instance can be inventoried the call throws; an unproven inventory is never success []."},
            (Method::Observe.name()):{"parameters":query,"returns":observation,"requires":[Method::Observe.capability().as_str()],
                "semantics":"Starts a native filtered semantic query on one window, retiring only that window's previous semantic query. Return elements are one bounded page. Continue with only {cursor: page.next}; each cursor advances once, and earlier page element refs share the same observation_ref. Filters run during native traversal, which retains its frontier. A failed continuation retires the query. next=null does not imply complete: inspect coverage. complete describes a live traversal without known gaps, not an atomic UI snapshot or proof that an element does not exist."},
            (Method::Capture.name()):{"parameters":window,"returns":capture,"requires":[Method::Capture.capability().as_str()],
                "semantics":"Capture this window. Replaces only this window's previous pixel binding; semantic queries and other windows survive. Only metadata enters the VM; the last image is attached outside the VM."},
            (Method::Press.name()):{"parameters":press,"returns":receipt,"requires":[Method::Press.capability().as_str()],
                "semantics":"Use a fresh semantic observation and one of its can_press=true element refs from any page of that query. Core action admission consumes both observation kinds on this window; other windows survive. Reobserve before another action on this window."},
            (Method::Click.name()):{"parameters":click,"returns":receipt,"requires":[Method::Click.capability().as_str()],
                "semantics":"Use a fresh pixel capture of this window and coordinates within its image dimensions. button defaults to left; count is 1 or 2. Core action admission consumes both observation kinds on this window; other windows survive. Never uses display/global coordinates."},
            (Method::TypeText.name()):{"parameters":object(json!({"window_ref":reference,"text":{"type":"string","minLength":1,"description":"Literal Unicode text, at most 16 KiB UTF-8 (48 KiB after JSON encoding); no NUL. Inserts into the target window's focused control, not a clipboard paste."}}), &["window_ref","text"]),"returns":receipt,"requires":["typeText"],
                "semantics":"DTW coordinates input and verifies the selected window's focus before inserting literal text. Select the intended editable control first. No held keys escape the call. All observations on this window retire; read back independently."},
            (Method::Keypress.name()):{"parameters":object(json!({"window_ref":reference,"keys":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"string","minLength":1,"maxLength":32},"description":"One chord, for example ['META','A'], ['CTRL','S'], ['RETURN'], ['TAB'], ['SHIFT','TAB']; META is Command on macOS and Windows key on Windows. Use typeText for literal Unicode."}}), &["window_ref","keys"]),"returns":receipt,"requires":["keypress"],
                "semantics":"Press and release one complete chord within DTW's input transaction after target focus verification. No persistent key-down API. All target-window observations retire. Independently inspect effects; never retry uncertain input."},
            (Method::Scroll.name()):{"parameters":object(json!({"window_ref":reference,"delta_x":{"type":"integer","minimum":-100,"maximum":100,"default":0},"delta_y":{"type":"integer","minimum":-100,"maximum":100,"default":0}}), &["window_ref"]),"returns":receipt,"requires":["scroll"],
                "semantics":"Scroll the selected window in signed wheel steps: positive x right, positive y down. Distance depends on the OS and application, not a pixel guarantee. At least one delta must be nonzero. Native routing/visibility conditions apply; multiple nested scroll regions may need a coordinate click to select focus first. All target-window observations retire."},
            (Method::Focus.name()):{"parameters":window,"returns":receipt,"requires":["focus"],
                "semantics":"Bring the exact selected app window to the foreground and verify focus under DTW's input coordination. This affects the user's foreground. It does not grant an input lease to later scripts; each later input independently checks its target. All target-window observations retire."},
            (Method::SetValue.name()):{"parameters":object(json!({"window_ref":reference,"observation_ref":reference,"element_ref":reference,"value":{"type":"string","description":"Replacement value, at most 16 KiB UTF-8 (48 KiB after JSON encoding), no NUL; empty clears the field."}}), &["window_ref","observation_ref","element_ref","value"]),"returns":receipt,"requires":["setValue"],
                "semantics":"Set literal text only on an editable text control whose returned actions include setValue. Native setters may not emit the same events as typing; independently check application state. Consumes both observation kinds on this window. For ordinary user typing use typeText after selecting an editable control."}
        },
        "references":{
            "app":"Selected by the exec app identifier; DTW discovers its live instances and binds each window to its exact native owner internally. Scripts cannot select another app or pass app_ref, PID, native IDs or provider handles.",
            "window":"Opaque context-local refs returned by app.windows(), each bound to one exact native app instance and window. An unchanged target keeps its ref across refreshes and sibling instance changes; context retirement or that target changing invalidates it.",
            "observation":"One semantic query and one pixel binding per window, at most 15000 ms from initial acquisition; continuation does not extend life. New reads retire only the same window and kind, including failed replacements. Accepted actions consume both kinds on their target window. Script deadlines can shorten calls but never extend binding life.",
            "element":"Opaque ref belonging to one live semantic query; earlier pages remain usable until query expiry, replacement, failure or action consumption. Cannot be used with pixels or another window.",
            "cursor":"Opaque context-local single-use continuation. Pass it alone to observe; do not add window_ref or repeat filters. Empty pages with next can occur while filtering. Inspect coverage.complete and coverage.reason even when next is null."
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
            "source":"const inventory = await app.windows();\nconst candidates = inventory.windows.filter(w => w.title === 'Target window');\nif (candidates.length !== 1) throw new Error('Expected one chosen window');\nconst window_ref = candidates[0].window_ref;\nlet page = await app.observe({window_ref, role:'button', text:'Target control'});\nconst controls = [];\nwhile (true) {\n  controls.push(...page.elements.filter(e => e.name === 'Target control' && e.enabled === true && e.can_press));\n  if (!page.next) break;\n  page = await app.observe({cursor:page.next});\n}\nif (!page.coverage.complete || controls.length !== 1) throw new Error('Expected one chosen control in complete discovery');\nconst receipt = await app.press({window_ref, observation_ref:page.observation_ref, element_ref:controls[0].element_ref});\nconst after = await app.observe({window_ref, limit:10});\nconst capture = await app.capture({window_ref});\nreturn {receipt, after, capture};"
        }
    })
}

/// Small default catalog; expensive schemas are opt-in through the api tool.
pub(super) fn index() -> Value {
    let full = super::scripts::description();
    json!({"language":"javascript", "entry":"exec({app, source}); await app methods sequentially; state persists per app",
        "methods":full["facade"], "details":"Call api({methods:[name,...]}) for argument/return schemas and semantics. api({}) lists method signatures.",
        "references":"Keep window_ref in state. Observations expire after 15s; actions retire observations on that window. Read/act/verify in one script; never replay uncertain actions.",
        "limits":{"default_timeout_ms":super::scripts::DEFAULT_TIMEOUT_MS,"max_source_utf8_bytes":super::scripts::MAX_SOURCE_BYTES,"max_calls":null}})
}

pub(super) fn requested(args: &Value) -> Result<Value, super::wire::WireError> {
    let invalid = |message: &str| super::wire::WireError {
        code: "invalid_arguments".into(),
        message: message.into(),
        details: None,
    };
    let fields = args
        .as_object()
        .ok_or_else(|| invalid("api arguments must be an object"))?;
    if fields.keys().any(|name| name != "methods") {
        return Err(invalid("api accepts only methods"));
    }
    let Some(names) = fields.get("methods") else {
        return Ok(index());
    };
    let names = names
        .as_array()
        .filter(|names| !names.is_empty() && names.len() <= Method::ALL.len())
        .ok_or_else(|| invalid("methods must be a nonempty array of known method names"))?;
    let full = super::scripts::description();
    let mut selected = serde_json::Map::new();
    for name in names {
        let name = name
            .as_str()
            .filter(|name| Method::parse(name).is_some())
            .ok_or_else(|| invalid("Unknown script method"))?;
        selected.insert(name.into(), full["methods"][name].clone());
    }
    Ok(
        json!({"methods":selected,"references":full["references"],"errors":full["errors"]["catch"],"execution":full["execution"],"images":full["images"]}),
    )
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
