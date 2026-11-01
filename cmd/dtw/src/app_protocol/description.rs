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
            "error":{"anyOf":[error,{"type":"null"}]},
            "delivery":object(json!({
                "route":{"enum":["semantic","background","foreground",null]},
                "reason":{"type":["string","null"]},
                "foreground_borrowed":{"type":"boolean"},
                "focus_return":{"enum":["pending","restored","application_restored","changed_externally","previous_app_closed","unchanged","unconfirmed",null]}
            }), &["route","reason","foreground_borrowed","focus_return"])
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
            "scope":{"enum":["same_process_standard_windows","same_process_application_surfaces","application_and_proven_associated_surfaces","application_instances"]},
            "status":{"enum":["available","empty_within_scope","partial"]},
            "complete_within_scope":{"type":"boolean"},
            "candidate_count":{"type":["integer","null"],"minimum":0},
            "supported_count":{"type":"integer","minimum":0},
            "excluded_count":{"type":["integer","null"],"minimum":0},
            "associated_count":{"type":"integer","minimum":0,"maximum":8192},
            "association_complete":{"const":false},
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
    let scroll_base = json!({"window_ref":reference,
        "delta_x":{"type":"integer","minimum":-100,"maximum":100,"default":0},
        "delta_y":{"type":"integer","minimum":-100,"maximum":100,"default":0}});
    let mut scroll_pixels = scroll_base.clone();
    scroll_pixels["observation_ref"] = reference.clone();
    scroll_pixels["x"] = json!({"type":"number","minimum":0});
    scroll_pixels["y"] = json!({"type":"number","minimum":0});
    let mut scroll_element = scroll_base.clone();
    scroll_element["observation_ref"] = reference.clone();
    scroll_element["element_ref"] = reference.clone();
    let scroll = json!({"oneOf":[
        object(scroll_base, &["window_ref"]),
        object(scroll_pixels, &["window_ref","observation_ref","x","y"]),
        object(scroll_element, &["window_ref","observation_ref","element_ref"])
    ]});
    let window = object(json!({"window_ref":reference}), &["window_ref"]);
    let query_base = json!({"window_ref":reference,
        "role":{"type":"string","minLength":1,"maxLength":64,"description":"Exact returned semantic role, for example button."},
        "text":{"type":"string","minLength":0,"maxLength":256,"description":"Case-insensitive substring of label, name or value. Combined with role using AND. Empty string omits the text filter; whitespace is literal."},
        "limit":{"type":"integer","minimum":1,"maximum":100,"default":40}
    });
    let mut menu_query = query_base.clone();
    menu_query["scope"] = json!({"const":"menu"});
    let mut subtree_query = query_base.clone();
    subtree_query["observation_ref"] = reference.clone();
    subtree_query["element_ref"] = reference.clone();
    let mut region_query = query_base.clone();
    region_query["observation_ref"] = reference.clone();
    region_query["region"] = object(
        json!({"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}),
        &["x", "y", "width", "height"],
    );
    let query = json!({"oneOf":[
        object(query_base, &["window_ref"]),
        object(menu_query, &["window_ref","scope"]),
        object(subtree_query, &["window_ref","observation_ref","element_ref"]),
        object(region_query, &["window_ref","observation_ref","region"]),
        object(json!({"cursor":reference}), &["cursor"])
    ]});
    let text_request = object(
        json!({"window_ref":reference,"observation_ref":reference,"element_ref":reference,
        "offset":{"type":"integer","minimum":0,"maximum":4294967295u64,"default":0},"limit":{"type":"integer","minimum":2,"maximum":16384,"default":4096}}),
        &["window_ref", "observation_ref", "element_ref"],
    );
    let text_page = object(
        json!({"text":{"type":"string"},"offset":{"type":"integer","minimum":0},"next_offset":{"type":["integer","null"],"minimum":0},"total_length":{"type":"integer","minimum":0},"unit":{"const":"utf16"}}),
        &["text", "offset", "next_offset", "total_length", "unit"],
    );
    let observation = object(
        json!({
            "window_ref":reference,"observation_ref":reference,"kind":{"const":"semantic"},
            "next":{"anyOf":[reference,{"type":"null"}]},
            "coverage":object(json!({"complete":{"type":"boolean"},"reason":{"type":["string","null"]},"visited":{"type":"integer","minimum":0,"maximum":8192}}), &["complete","reason","visited"]),
            "elements":{"type":"array","items":object(json!({
                "element_ref":reference,"label":{"type":"string","description":"Display summary; may include a semantic role prefix. Use name for the native accessible name."},"can_press":{"type":"boolean"},
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
    let selector_fields = json!({
        "scope":{"enum":["window","menu"],"default":"window","description":"Search the selected window or its app menu. Menu scope requires provider support; it never falls back to window scope."},
        "role":{"type":"string","minLength":1,"maxLength":64,"description":"Exact returned semantic role."},
        "text":{"type":"string","minLength":1,"maxLength":256,"description":"Case-insensitive substring of label, name or value; AND with role."}
    });
    let selector = json!({"anyOf":[
        object(selector_fields.clone(), &["role"]),
        object(selector_fields, &["text"])
    ]});
    let timeout = json!({"type":"integer","minimum":0,"maximum":30000,"default":2000,
        "description":"Polling window for reads; an admitted native read may finish after it. Zero disables read polling and cold-window focus preparation. Menu selector actions still prepare focus once after a unique match and resolve afresh; other selector actions may do so for content_unproven with nonzero timeout. All work retains the original script deadline; actions are never repeated."});
    let select = object(
        json!({"window_ref":reference,"selector":selector,"timeout_ms":timeout}),
        &["window_ref", "selector"],
    );
    let mut element_fields = observation["properties"]["elements"]["items"]["properties"].clone();
    element_fields
        .as_object_mut()
        .unwrap()
        .remove("element_ref");
    let element = object(
        element_fields,
        &[
            "label",
            "role",
            "name",
            "value",
            "enabled",
            "actions",
            "can_press",
        ],
    );
    let found = object(
        json!({"window_ref":reference,"observation_ref":reference,"element_ref":reference,"element":element}),
        &["window_ref", "observation_ref", "element_ref", "element"],
    );
    let press_refs = object(
        json!({"window_ref":reference,"observation_ref":reference,"element_ref":reference}),
        &["window_ref", "observation_ref", "element_ref"],
    );
    let press = json!({"oneOf":[press_refs,select]});
    let value = json!({"type":"string","description":"Replacement value, at most 16 KiB UTF-8 (48 KiB after JSON encoding), no NUL; empty clears the field."});
    let set_value = json!({"oneOf":[
        object(json!({"window_ref":reference,"observation_ref":reference,"element_ref":reference,"value":value}), &["window_ref","observation_ref","element_ref","value"]),
        object(json!({"window_ref":reference,"selector":selector,"timeout_ms":timeout,"value":value}), &["window_ref","selector","value"])
    ]});
    let wait_window = object(
        json!({"window_ref":reference,
        "state":{"enum":["dialog","unblocked","closed"]},"timeout_ms":timeout}),
        &["window_ref", "state"],
    );
    let click = object(
        json!({"window_ref":reference,"observation_ref":reference,
        "x":{"type":"number","minimum":0,"description":"Finite x coordinate in the explicit coordinate_space; defaults to image pixels."},
        "y":{"type":"number","minimum":0,"description":"Finite y coordinate in the same coordinate_space as x."},
        "coordinate_space":{"enum":["image_pixels","normalized_1000"],"default":"image_pixels","description":"normalized_1000 uses 0 <= x,y < 1000 and DTW converts through the bound capture dimensions. Never mix spaces between axes."},
        "button":{"enum":["left","right"],"default":"left"},"count":{"type":"integer","minimum":1,"maximum":2,"default":1}}),
        &["window_ref", "observation_ref", "x", "y"],
    );
    json!({
        "version":1,"language":"javascript",
        "entry":"exec({app: 'com.example.App', source}); source is an async function body with app and state",
        "facade":{"windows":"app.windows()","waitForWindow":"app.waitForWindow({window_ref,state:'dialog'|'unblocked'|'closed',timeout_ms?})","find":"app.find({window_ref,selector:{scope?:'window'|'menu',role?,text?},timeout_ms?})","observe":"app.observe({window_ref, role?, text?, limit?}) or add scope:'menu', observation_ref+element_ref (subtree), or observation_ref+region:{x,y,width,height} (capture pixels); continue with app.observe({cursor})","capture":"app.capture({window_ref})",
            "press":"app.press({window_ref,selector:{scope?:'window'|'menu',role?,text?},timeout_ms?}) or {window_ref,observation_ref,element_ref}","click":"app.click({window_ref,observation_ref,x,y,coordinate_space?,button?,count?})",
            "typeText":"app.typeText({window_ref,text})","keypress":"app.keypress({window_ref,keys})","scroll":"app.scroll({window_ref,delta_x?,delta_y?,observation_ref?,x?,y?,element_ref?})",
            "focus":"app.focus({window_ref})","setValue":"app.setValue({window_ref,selector:{scope?:'window'|'menu',role?,text?},value,timeout_ms?}) or {window_ref,observation_ref,element_ref,value}",
            "readText":"app.readText({window_ref,observation_ref,element_ref,offset?,limit?})"},
        "methods":{
            (Method::Windows.name()):{"parameters":null,"returns":inventory,"requires":[Method::Windows.capability().as_str()],
                "semantics":"No arguments. Aggregates proven windows from the app instances discovered before this exec. Each window_ref routes to its exact native instance. Unchanged targets keep refs and observations; proven removed targets retire theirs. A failed instance does not discard proven sibling windows: partial coverage reports unavailable_instances and bounded errors. When no instance can be inventoried the call throws; an unproven inventory is never success []."},
            (Method::Find.name()):{"parameters":select,"returns":found,"requires":["observe"],
                "semantics":"Resolve one fresh semantic element by selector in window scope by default, or selector.scope:'menu' for the application's menu. DTW handles pagination and briefly waits using reads only; find never borrows foreground focus. Uniqueness requires a completed traversal; ambiguous matches return bounded candidates, incomplete discovery stays unproven. Missing controls can be read again within timeout_ms; find returns the current enabled state without waiting for it to change. Actions are never repeated. The returned refs and capabilities follow observe's normal lifetime. Use observe for exploration, large trees or narrower scopes."},
            (Method::WaitForWindow.name()):{"parameters":wait_window,"returns":object(json!({"state":{"enum":["dialog","unblocked","closed"]},"window_ref":reference,"kind":{"enum":["window","dialog","sheet"]},"relation":{"enum":["same_window","child_modal","application_modal"]}}), &["state","window_ref"]),"requires":["windows"],
                "semantics":"Read-only waiting for one selected native window. dialog returns the fresh ref of the uniquely proven actionable related modal; unblocked waits for the selected window's modal gate to clear; closed requires native proof that this exact surface disappeared. Missing inventory, focus changes, empty titles and arbitrary new windows are not closure or relationship proof. Uses scoped native probes, never input or action replay. Timeout is an ordinary condition-not-met error; the enclosing script deadline still applies. Unsupported providers report unsupported_capability."},
            (Method::Observe.name()):{"parameters":query,"returns":observation,"requires":[Method::Observe.capability().as_str()],
                "semantics":"Starts a bounded native semantic query. Default scope is the window; scope:'menu' discovers its app menu, observation_ref+element_ref uses a retained subtree, and observation_ref+region uses pixel coordinates of a retained capture. Scopes are exclusive; provider.supported_observe_scopes advertises availability. A region tests native geometry: macOS clips it through proven AXScrollArea ancestor viewports and can skip disjoint scroll contents; Windows uses UIA bounding rectangles. This does not prove pixel visibility or absence of occlusion. Invalid or unsupported scope admission preserves refs. An admitted replacement retires only this window's previous semantic query; the capture and other windows survive. Return elements are one bounded page. Continue with only {cursor: page.next}; each cursor advances once and earlier page refs share the same observation_ref. Filters run during native traversal. Failed continuation retires the query. next=null does not imply complete: inspect coverage. Complete is a live traversal of the selected scope without known gaps, not an atomic snapshot or proof an element does not exist."},
            (Method::ReadText.name()):{"parameters":text_request,"returns":text_page,"requires":[Method::ReadText.capability().as_str()],
                "semantics":"Read native text from one retained semantic element; discovery label/name/value are bounded previews, never proof of full text. Offset, limit and total_length use UTF-16 units; pass next_offset unchanged until null. Pages never split surrogate pairs. The provider pins the initial content and rejects replacement or changed text as stale_observation. Success, unsupported_capability and invalid_text_range preserve observations and original expiry; other failures retire the semantic query. Unsupported text providers never substitute screenshots/OCR, titles or summaries for document text."},
            (Method::Capture.name()):{"parameters":window,"returns":capture,"requires":[Method::Capture.capability().as_str()],
                "semantics":"Capture this window. Replaces this window's previous pixel binding. On Windows, an explicit native occlusion failure during exec may prepare foreground once through ordinary input admission and capture afresh under the original deadline; the focus receipt is retained in this capture step's resolution.focus_preparation. That preparation retires this window's semantic queries too. Otherwise semantic queries survive; other windows always survive. A denied or unconfirmed preparation stops, and a still-occluded capture is not retried. No action is replayed. Atomic diagnostic capture remains read-only. Only metadata enters the VM; the last image is attached outside the VM."},
            (Method::Press.name()):{"parameters":press,"returns":receipt,"requires":[Method::Press.capability().as_str()],
                "semantics":"Use selector for a known control: DTW resolves a fresh unique ready target and presses once. selector.scope defaults to window. With scope:'menu', a unique match triggers one ordinary focus preparation and fresh menu traversal before dispatch, including timeout_ms:0; app-menu content and enabled state may change on focus. Read-only readiness polling may continue within the original timeout after preparation; focus is not repeated. find never focuses. Unsupported menu scope never falls back to window discovery. With nonzero timeout_ms, terminal content_unproven may trigger one service-policy-permitted focus preparation and one fresh traversal, even after the polling window; all use the original script deadline. For window scope, other partial reasons, no match and ambiguity never trigger focus. Preparation failure or unknown stops immediately; its separate receipt stays in the resolution ledger, and script cleanup releases input ownership and leaves the selected foreground in place. The selected press is never dispatched until uniqueness/readiness is proven and is never replayed. Or pass a fresh semantic observation and one of its can_press=true element refs from any page of that query. Selector and ref forms are exclusive. Core action admission consumes both observation kinds on this window; other windows survive. Use a fresh selector action or reobserve before another ref-based action on this window."},
            (Method::Click.name()):{"parameters":click,"returns":receipt,"requires":[Method::Click.capability().as_str()],
                "semantics":"Use a fresh pixel capture of this window. coordinate_space defaults to image_pixels; normalized_1000 converts x and y internally using that exact capture. Pixel coordinates must fit the image; normalized coordinates must be less than 1000 on both axes. button defaults to left; count is 1 or 2. Core action admission consumes both observation kinds on this window; other windows survive. Never uses display/global coordinates."},
            (Method::TypeText.name()):{"parameters":object(json!({"window_ref":reference,"text":{"type":"string","minLength":1,"description":"Unicode text, at most 16 KiB UTF-8 (48 KiB after JSON encoding); no NUL. Sent through keyboard input to the selected editing control."}}), &["window_ref","text"]),"returns":receipt,"requires":["typeText"],
                "semantics":"DTW verifies the selected window before Unicode keyboard input. On macOS, newline characters generate Return (CRLF once) and tabs generate Tab; application formatting and indentation can change the resulting text. On macOS, eligible input is delivered in the background; otherwise DTW may raise and retain the selected foreground before dispatch. Select the intended editable control first. No held keys escape the call. All observations on this window retire; read back independently."},
            (Method::Keypress.name()):{"parameters":object(json!({"window_ref":reference,"keys":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"string","minLength":1,"maxLength":32},"description":"One chord, for example ['META','A'], ['CTRL','S'], ['RETURN'], ['TAB'], ['SHIFT','TAB']; META is Command on macOS and Windows key on Windows. Use typeText for Unicode text."}}), &["window_ref","keys"]),"returns":receipt,"requires":["keypress"],
                "semantics":"Press and release one complete chord within DTW's input transaction after target focus verification. No persistent key-down API. All target-window observations retire. Independently inspect effects; never retry uncertain input."},
            (Method::Scroll.name()):{"parameters":scroll,"returns":receipt,"requires":["scroll"],
                "semantics":"Scroll in signed wheel steps: positive x right, positive y down. At least one delta must be nonzero; distance depends on OS/application, not a pixel guarantee. Default anchor is the window center. On macOS, supply observation_ref with x/y in capture pixels, or with element_ref from observe; never mix these forms. An element need not support press. DTW verifies the bound object and fixed point before the first wheel; movement caused by that scroll does not invalidate the remaining wheel sequence. Target-window observations are consumed, other windows are preserved. Unsupported anchored providers reject before consuming references. Routing/visibility conditions and uncertain-input handling remain unchanged."},
            (Method::Focus.name()):{"parameters":window,"returns":receipt,"requires":["focus"],
                "semantics":"Bring the exact selected app window to the foreground and verify focus under DTW's input coordination. This affects the user's foreground. On macOS, the window stays foreground after the script ends; DTW does not switch back to the previous app. The Agent may close or minimize the window at a task milestone, or the user may switch away. An external focus change is preserved. It does not grant an input lease to later scripts; each later input independently checks its target. All target-window observations retire."},
            (Method::SetValue.name()):{"parameters":set_value,"returns":receipt,"requires":["setValue"],
                "semantics":"Use selector to resolve a fresh unique ready field and set its value once, or pass current observation_ref and element_ref. Selector scope and menu focus preparation follow press. For window scope, the same one-time content_unproven preparation applies when timeout_ms is nonzero and service policy permits; preparation evidence is separate, unknown stops immediately, and the value write is never replayed. Set literal text only on an editable text control whose returned actions include setValue. Native setters may not emit the same events as typing; independently check application state. Consumes both observation kinds on this window. For ordinary user typing use typeText after selecting an editable control."}
        },
        "references":{
            "app":"Selected by the exec app identifier; DTW discovers its live instances and binds each window to its exact native owner internally. Scripts cannot select another app or pass app_ref, PID, native IDs or provider handles.",
            "window":"Opaque context-local refs returned by app.windows() or waitForWindow, each bound to one exact native app instance and window. An unchanged target keeps its ref across refreshes and sibling instance changes; context retirement or that target changing invalidates it.",
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
        "state":"The state root object cannot be rebound. Modify its JSON fields; state must be strict JSON. Returned plain-object undefined properties are omitted recursively; undefined array elements remain invalid. Both reject non-finite numbers, cycles, functions, accessors and exotic objects, with depth <=64 and nodes <=32768. Invalid JSON errors include a bounded field path. An absent return or undefined result root becomes null. State commits only after successful completion before cancellation/timeout; mutations never roll back desktop effects.",
        "errors":{
            "shape":error,"catch":"Ordinary Host errors expose e.code and optional e.details and may be caught. Timeout, cancellation and call limit remain failures even when caught. Unhandled Promise rejections fail the script without committing state. Caught memory-allocation exceptions may recover within the hard VM quota.",
            "inventory_unproven":"e.code=window_inventory_unproven; e.details contains coverage with status=unproven, complete_within_scope=false and diagnostics, without windows. Explicit supported-scope exclusions alone do not make inventory unproven."
        },
        "receipts":{
            "shape":receipt,
            "meaning":"dispatched records dispatch evidence, not proven UI success. Unknown may have effects. Inspect the receipt and independently read back; never automatically retry an unknown action.",
            "failure":"A Host error throws before returning to JavaScript; its action receipt remains in script.steps. Previously completed effects and receipts survive later exceptions, cancellation, timeout and VM failure.",
            "ledger":"script.steps retains number,line,operation,status,error,receipt and bounded selector/window-wait resolution plus discovery/inventory coverage independently of evictable value/image payloads. Bridge attempts consume call numbers, so the ledger may have gaps. No automatic replay. DTW keeps the original ledger; repeated accepted MCP transport IDs refer to the same execution, while new requests execute new work."
        },
        "result":"exec waits until completion and returns value.result plus console,console_truncated,calls,last_capture. last_capture.step identifies the actual capture call; timing:before_later_calls and later_calls explicitly identify an image predating later calls. Input never implicitly takes a post-action screenshot; call capture after input when visual verification is needed. script.summary replaces the step ledger with counts and a last_issue sample; script.failure,state_committed,shutdown_error,optional focus_return and all recovery flags remain. result({app}) retrieves the full original record with script.steps; optional execution_id selects a prior returned record. Summary dispatch/evidence counts do not prove business success. Optional delivery summaries retain reported routes and focus-return evidence; absent delivery is not inferred. An app.focus receipt may remain pending while script.focus_return records the later execution cleanup. Caught errors may coexist with state_committed=true; consult the full ledger for all issues, never replay. Payload eviction sets payload_expired and may clear value/image while retaining errors and receipts.",
        "images":"value.last_capture identifies the last successful capture's step and metadata. Its PNG is returned as MCP image content outside the VM; scripts cannot access or return image bytes.",
        "lifecycle":"Cancellation or script timeout closes the context, stops native resources and joins the VM before job completion. Cancellation cannot prove an already dispatched action did not occur. A later new exec rebuilds affected native references as needed (context_reset) while retaining committed JSON state. Only an actual application instance change clears state (state_reset). Retained jobs remain queryable within this MCP connection.",
        "example":{
            "purpose":"Choose one explicitly named window and enabled control, press once, independently reobserve, then capture. Replace the two exact names with the intended target's observed names; do not infer success from dispatch alone.",
            "source":"const inventory = await app.windows();\nconst candidates = inventory.windows.filter(w => w.title === 'Target window');\nif (candidates.length !== 1) throw new Error('Choose one target window');\nconst window_ref = candidates[0].window_ref;\nconst receipt = await app.press({window_ref, selector:{role:'button',text:'Target control'}});\nconst after = await app.observe({window_ref, limit:10});\nreturn {receipt, after};"
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
