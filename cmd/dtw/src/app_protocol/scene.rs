//! Agent-facing app scene. Native refs, layering and image bindings stay here.
use super::*;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::Cursor;

#[derive(Default)]
pub(super) struct SceneState {
    baseline: Option<Value>,
    observed: Option<Value>,
    window_ids: HashMap<String, String>,
    nodes: Vec<Value>,
    pub(super) active: Option<String>,
    // Last image returned to the script. Private refreshes never replace it.
    raster: Option<Raster>,
    pixels: bool,
    projected: Option<Value>,
    element_ids: HashMap<String, DisplayIdentity>,
    retained_ids: HashMap<String, String>,
    retained_nodes: HashMap<String, Value>,
    next_element: u64,
    viewport: Option<([f64; 4], String, [f64; 4])>,
    window_wait_pending: bool,
    pending_new_window: Option<std::collections::HashSet<String>>,
}
struct DisplayIdentity {
    id: String,
    retained_identity: Option<String>,
}
impl SceneState {
    pub(super) fn projection(&self) -> Option<Value> {
        self.projected.clone()
    }

    // Intermediate script reads must not consume changes the Agent has not seen.
    pub(super) fn publish(&mut self) {
        self.baseline = self.observed.clone();
    }
}
struct Layer {
    // Ownership evidence of the script-visible image survives transient windows.
    instance: AppInstance,
    window: String,
    observation: String,
    bounds: [f64; 4],
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
struct Raster {
    bounds: [f64; 4],
    width: u32,
    height: u32,
    layers: Vec<Layer>,
}

pub(super) fn validate(op: &str, args: &Value) -> Result<(), wire::WireError> {
    let allowed: &[&str] = match op {
        "scene.look" => &["full", "role", "text", "scope", "target"],
        "scene.capture" | "scene.after" => &["full"],
        "scene.readText" => &["target", "offset", "limit"],
        "scene.waitForNewWindow" => &["timeout_ms"],
        "scene.click" => &["target", "selector", "x", "y", "button", "count"],
        "scene.setValue" => &["target", "selector", "value"],
        "scene.move" => &["x", "y"],
        "scene.drag" => &["path"],
        "scene.typeText" => &["text"],
        "scene.keypress" => &["keys"],
        "scene.scroll" => &["delta_y", "delta_x", "x", "y"],
        "scene.quit" | "scene.focus" => &[],
        _ => return Err(error("invalid_arguments", "Unknown app operation")),
    };
    let object = args
        .as_object()
        .ok_or_else(|| error("invalid_arguments", "Expected one argument object"))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(error(
            "invalid_arguments",
            "Unknown app argument; see Skill API.md",
        ));
    }
    if op == "scene.setValue" {
        let one_target = usize::from(args.get("target").is_some())
            + usize::from(args.get("selector").is_some())
            == 1;
        if !one_target
            || args["value"]
                .as_str()
                .is_none_or(|value| value.len() > 16_384 || value.contains('\0'))
        {
            return Err(error("invalid_arguments", "setValue requires one target or selector and a string value (up to 16 KiB, no NUL)"));
        }
    }
    if op == "scene.waitForNewWindow"
        && args
            .get("timeout_ms")
            .is_some_and(|v| v.as_u64().is_none_or(|ms| !(1..=10_000).contains(&ms)))
    {
        return Err(error(
            "invalid_arguments",
            "timeout_ms must be an integer from 1 to 10000",
        ));
    }
    for key in ["full"] {
        if args.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(error(
                "invalid_arguments",
                "Observation flags must be Boolean",
            ));
        }
    }
    if op == "scene.look" {
        if args
            .get("target")
            .is_some_and(|target| target.as_str().is_none_or(str::is_empty))
            || (args.get("target").is_some() && args.get("scope").is_some())
        {
            return Err(error(
                "invalid_arguments",
                "look target must be an observed ID and cannot be combined with scope",
            ));
        }
        if args
            .get("scope")
            .is_some_and(|scope| !matches!(scope.as_str(), Some("window" | "menu")))
        {
            return Err(error(
                "invalid_arguments",
                "look scope must be window or menu",
            ));
        }
        for (key, maximum) in [("role", 64), ("text", 256)] {
            if args.get(key).is_some_and(|value| {
                value
                    .as_str()
                    .is_none_or(|text| text.is_empty() || text.chars().count() > maximum)
            }) {
                return Err(error(
                    "invalid_arguments",
                    "Observation role/text must be bounded nonempty strings",
                ));
            }
        }
    }
    if op == "scene.readText" {
        if args["target"]
            .as_str()
            .is_none_or(|target| target.is_empty())
        {
            return Err(error(
                "invalid_arguments",
                "readText requires an observed target ID",
            ));
        }
        for (key, minimum, maximum) in [("offset", 0, u32::MAX as u64), ("limit", 2, 16384)] {
            if args.get(key).is_some_and(|value| {
                value
                    .as_u64()
                    .is_none_or(|number| number < minimum || number > maximum)
            }) {
                return Err(error(
                    "invalid_arguments",
                    "Invalid readText offset or limit",
                ));
            }
        }
    }
    Ok(())
}
// Sparse filtered pages can spend native work without producing rows. Both
// discovery and object renewal use this bound under their original deadline.
fn semantic_page_limit(query: &serde_json::Map<String, Value>) -> usize {
    if query.contains_key("role") || query.contains_key("text") {
        32
    } else {
        4
    }
}

fn error(code: &str, message: &str) -> wire::WireError {
    wire::WireError {
        code: code.into(),
        message: message.into(),
        details: None,
    }
}
fn failed(mut error: wire::WireError) -> Work {
    if matches!(error.code.as_str(), "invalid_request" | "invalid_args") {
        error.code = "invalid_arguments".into();
    }
    Work {
        completed: Completed {
            value: None,
            image: None,
            receipt: None,
            error: Some(error),
            payload_bytes: 0,
            payload_expired: false,
        },
        publication: Publication::None,
        state_update: None,
    }
}
fn data(value: Value) -> Work {
    Work {
        completed: Completed {
            value: Some(value),
            image: None,
            receipt: None,
            error: None,
            payload_bytes: 0,
            payload_expired: false,
        },
        publication: Publication::None,
        state_update: None,
    }
}
// Only semantic availability may degrade to fresh pixels. Caller termination,
// permissions, process identity and malformed provider replies remain failures.
fn semantic_fallback(code: &str) -> bool {
    matches!(
        code,
        "ax_unavailable"
            | "children_unavailable"
            | "semantic_not_ready"
            | "semantic_window_unavailable"
            | "semantic_menu_unavailable"
            | "window_unproven"
            | "stale_observation"
            | "stale_cursor"
            | "unsupported_capability"
            | "target_changed"
            | "stale_window"
            // UIA/AT-SPI read failures can use the generic provider code.
            // A pixel read independently validates target and provider health.
            | "backend_failed"
    )
}

fn terminal_read_error(code: &str) -> bool {
    matches!(
        code,
        "cancelled"
            | "timeout"
            | "timed_out"
            | "closed"
            | "deadline_exceeded"
            | "native_stopping"
            | "permission_required"
            | "permission_denied"
            | "app_changed"
            | "wrong_app"
            | "wrong_provider"
            | "invalid_observation"
            | "backend_failed"
    )
}

fn transient_capture_error(code: &str) -> bool {
    matches!(
        code,
        "scene_changed"
            | "capture_geometry_changed"
            | "capture_failed"
            | "target_changed"
            | "stale_window"
            | "window_unavailable"
            | "surface_unavailable"
    )
}

fn bounds(row: &Value) -> Option<[f64; 4]> {
    let a = row["scene"]["bounds"].as_array()?;
    if a.len() != 4 {
        return None;
    }
    let b = [
        a[0].as_f64()?,
        a[1].as_f64()?,
        a[2].as_f64()?,
        a[3].as_f64()?,
    ];
    (b.iter().all(|v| v.is_finite()) && b[2] > 0.0 && b[3] > 0.0).then_some(b)
}
fn project_text(entry: &mut Value, field: &str, value: &Value) -> bool {
    let Some(text) = value.as_str() else {
        entry[field] = value.clone();
        return false;
    };
    entry[field] = json!(text.chars().take(240).collect::<String>());
    let length = text.chars().count();
    if length <= 240 {
        return false;
    }
    entry[format!("{field}_truncated")] = json!(true);
    entry[format!("{field}_length")] = json!(length);
    // Only omitted content needs a revision in the view. It signals change,
    // never full readback or object identity; ordinary fields stay compact.
    entry[format!("{field}_revision")] = json!(format!("{:x}", Sha256::digest(text.as_bytes())));
    true
}

fn useful_element(row: &Value) -> bool {
    element_priority(row) < 3
}
fn element_priority(row: &Value) -> u8 {
    if row["actions"].as_array().is_some_and(|v| !v.is_empty()) {
        0
    } else if row["value"].as_str().is_some_and(|s| !s.is_empty()) {
        1
    } else if row["name"].as_str().is_some_and(|s| !s.is_empty())
        && !matches!(
            row["role"].as_str(),
            Some("group" | "window" | "toolbar" | "scroll_area")
        )
    {
        2
    } else {
        3
    }
}

impl<B: Backend + 'static> Coordinator<B> {
    fn scene_target(&self, target: &Value) -> Option<(Value, bool)> {
        let state = self.state.lock().unwrap();
        state
            .scene
            .nodes
            .iter()
            .find(|node| node["id"] == *target)
            .cloned()
            .map(|node| (node, false))
            .or_else(|| {
                target
                    .as_str()
                    .and_then(|id| state.scene.retained_nodes.get(id))
                    .cloned()
                    .map(|node| (node, true))
            })
    }
    pub(super) fn scene_atomic(&self, id: &str, op: &str, args: Value, deadline: Instant) -> Work {
        let submitted = match wire::script_submit(&self.app_ref, id, op, &args) {
            Ok(v) => v,
            Err(e) => return failed(e),
        };
        let mut work = match &submitted.command {
            Command::PressSelected {
                selector,
                timeout_ms,
            } if selector.scope == wire::SelectorScope::Window => {
                self.select_scene_element(&submitted, selector, *timeout_ms, deadline)
            }
            _ => self.execute_atomic(&submitted, deadline),
        };
        let publication = std::mem::replace(&mut work.publication, Publication::None);
        apply_publication(&mut self.state.lock().unwrap(), publication);
        work
    }
    fn scene_wait_after_application_input(&self, work: &Work) {
        if work
            .completed
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome.dispatch == DispatchState::Dispatched)
        {
            let mut state = self.state.lock().unwrap();
            state.scene.window_wait_pending = true;
            state.scene.pending_new_window = Some(state.windows.keys().cloned().collect());
        }
    }
    pub(super) fn scene_call(&self, id: &str, op: &str, args: &Value, deadline: Instant) -> Work {
        if let Err(e) = validate(op, args) {
            return failed(e);
        }
        if matches!(op, "scene.look" | "scene.capture" | "scene.after") {
            return self.scene_look(
                id,
                args,
                op == "scene.capture",
                op == "scene.after",
                0,
                deadline,
            );
        }
        if op == "scene.waitForNewWindow" {
            return self.scene_wait_for_new_window(id, args, deadline);
        }
        if op == "scene.readText" {
            return self.scene_read_text(id, args, deadline);
        }
        if op == "scene.quit" {
            let mut state = self.state.lock().unwrap();
            state.scene.raster = None;
            let active = state.scene.active.clone();
            drop(state);
            let work = self.scene_atomic(
                id,
                "quit",
                active.map(|w| json!({"window_ref":w})).unwrap_or(json!({})),
                deadline,
            );
            self.scene_wait_after_application_input(&work);
            return work;
        }
        if matches!(op, "scene.move" | "scene.drag") {
            return self.scene_pointer(id, op, args, deadline);
        }
        // Known sequential steps can open a new dialog/window in the same
        // script. Resolve implicit input routing from a fresh app inventory;
        // explicit semantic IDs and pixels remain bound to what was observed.
        let implicit = matches!(op, "scene.typeText" | "scene.keypress" | "scene.focus")
            || (op == "scene.scroll" && args.get("x").is_none())
            || (matches!(op, "scene.click" | "scene.setValue") && args.get("selector").is_some());
        if implicit && self.runtime.instances().is_empty() {
            // Reuse the existing launch/readiness path only for stopped apps.
            // Running apps need inventory, not a redundant semantic look before
            // a known selector, key chord, text insertion or default scroll.
            let prepared = self.scene_look(id, &json!({}), false, false, 0, deadline);
            if prepared.completed.error.is_some() || self.runtime.instances().is_empty() {
                return prepared;
            }
        }
        if implicit {
            // Focus is app-instance-local. A background sibling's focus hint
            // must never redirect input from the instance selected by the view.
            let selected_instance = {
                let state = self.state.lock().unwrap();
                state
                    .scene
                    .active
                    .as_ref()
                    .and_then(|w| state.windows.get(w))
                    .map(|target| target.app.clone())
            };
            let wait = self.state.lock().unwrap().scene.window_wait_pending;
            let until = (Instant::now()
                + if wait {
                    Duration::from_secs(2)
                } else {
                    Duration::ZERO
                })
            .min(deadline);
            let rows = loop {
                let work = self.scene_atomic(id, "windows", json!({}), deadline);
                if work.completed.error.is_some() {
                    return work;
                }
                let value = work.completed.value.unwrap();
                let rows = ordered(value["windows"].as_array().cloned().unwrap_or_default());
                if rows.iter().any(|row| row["visible"] == true) || Instant::now() >= until {
                    break rows;
                }
                std::thread::sleep(
                    Duration::from_millis(20).min(until.saturating_duration_since(Instant::now())),
                );
            };
            self.state.lock().unwrap().scene.window_wait_pending = false;
            let mut state = self.state.lock().unwrap();
            let instance = selected_instance.or_else(|| {
                rows.iter()
                    .find(|row| row["visible"] == true)
                    .and_then(|row| state.windows.get(row["window_ref"].as_str()?))
                    .map(|target| target.app.clone())
            });
            let candidates: Vec<_> = rows
                .iter()
                .filter(|row| {
                    row["visible"] == true
                        && row["window_ref"]
                            .as_str()
                            .and_then(|w| state.windows.get(w))
                            .is_some_and(|target| Some(&target.app) == instance.as_ref())
                })
                .collect();
            let keyboard = matches!(op, "scene.typeText" | "scene.keypress");
            // Background AppKit dialogs can own input while AXFocusedWindow
            // still names their parent document. Modal proof wins over that
            // stale hint; ordinary overlays preserve the native focus route.
            let selected = candidates
                .iter()
                .find(|row| {
                    row["scene"]["keyboard_modal"] == true
                        && row["scene"]["semantic_candidate"] != false
                })
                .or_else(|| {
                    keyboard
                        .then(|| {
                            candidates
                                .iter()
                                .find(|row| row["scene"]["keyboard_focused"] == true)
                        })
                        .flatten()
                })
                .or_else(|| {
                    candidates
                        .iter()
                        .find(|row| row["scene"]["semantic_candidate"] != false)
                });
            if selected.is_none() && rows.iter().any(|row| row["visible"] == true) {
                return failed(error(
                    "target_changed",
                    "The selected application instance lost its window; inspect the returned view",
                ));
            }
            state.scene.active = selected
                .and_then(|row| row["window_ref"].as_str())
                .map(str::to_owned);
        }
        let active = self.state.lock().unwrap().scene.active.clone();
        let Some(window) = active else {
            if op == "scene.focus" && !self.runtime.instances().is_empty() {
                return Work::action(self.runtime.app_focus_until(deadline));
            }
            if op == "scene.keypress" && !self.runtime.instances().is_empty() {
                let mut options = args.clone();
                options["window_ref"] = json!("internal-app-shortcut");
                let parsed = match wire::script_submit(&self.app_ref, id, "keypress", &options) {
                    Ok(v) => v,
                    Err(e) => return failed(e),
                };
                if let Command::Input(WindowInput::Keypress { keys }) = parsed.command {
                    let work = Work::action(self.runtime.app_keypress_until(&keys, deadline));
                    self.scene_wait_after_application_input(&work);
                    return work;
                }
            }
            return failed(error(
                "look_required",
                "No current editing surface; inspect the app with look or capture",
            ));
        };
        let mut options = args.clone();
        let mut method = op.trim_start_matches("scene.").to_owned();
        if matches!(op, "scene.click" | "scene.setValue") {
            let setter = op == "scene.setValue";
            let modes = usize::from(args.get("target").is_some())
                + usize::from(args.get("selector").is_some())
                + usize::from(args.get("x").is_some() || args.get("y").is_some());
            if modes != 1 {
                return failed(error(
                    "invalid_arguments",
                    "Provide exactly one observed target or selector (click also accepts x/y)",
                ));
            }
            if let Some(target) = args.get("target") {
                let Some((node, _)) = self.scene_target(target) else {
                    return failed(error(
                        "target_changed",
                        "Target handle is unavailable; inspect the returned view",
                    ));
                };
                if args.get("button").is_some() || args.get("count").is_some() {
                    return failed(error(
                        "invalid_arguments",
                        "Semantic targets do not accept mouse button/count",
                    ));
                }
                let node = match self.scene_refresh_target(id, node, deadline) {
                    Ok(node) => node,
                    Err(error) => return failed(error),
                };
                method = if setter { "setValue" } else { "press" }.into();
                options = json!({"window_ref":node["window_ref"],"observation_ref":node["observation_ref"],"element_ref":node["element_ref"]});
                if setter {
                    options["value"] = args["value"].clone();
                }
            } else if args.get("selector").is_some() {
                if args.get("button").is_some() || args.get("count").is_some() {
                    return failed(error(
                        "invalid_arguments",
                        "Semantic selectors do not accept mouse button/count",
                    ));
                }
                method = if setter { "setValue" } else { "press" }.into();
                options["window_ref"] = json!(window);
            } else {
                let frame =
                    match self.scene_validate_frame(id, std::slice::from_ref(args), deadline) {
                        Ok(frame) => frame,
                        Err(e) => return failed(e),
                    };
                options = match frame.point(args) {
                    Ok(v) => v,
                    Err(e) => return failed(e),
                };
                for key in ["button", "count"] {
                    if let Some(v) = args.get(key) {
                        options[key] = v.clone();
                    }
                }
            }
        } else if op == "scene.scroll" && (args.get("x").is_some() || args.get("y").is_some()) {
            let frame = match self.scene_validate_frame(id, std::slice::from_ref(args), deadline) {
                Ok(frame) => frame,
                Err(e) => return failed(e),
            };
            options = match frame.point(args) {
                Ok(v) => v,
                Err(e) => return failed(e),
            };
            options["delta_y"] = args["delta_y"].clone();
            if let Some(v) = args.get("delta_x") {
                options["delta_x"] = v.clone();
            }
        } else if op == "scene.scroll" {
            // The default is the active window's center, through the same
            // retained pixel route as an explicit anchor. No AX-window shortcut.
            options["window_ref"] = json!(window);
            if let Err(e) = wire::script_submit(&self.app_ref, id, "scroll", &options) {
                return failed(e);
            }
            let captured = self.scene_atomic(id, "capture", json!({"window_ref":window}), deadline);
            if captured.completed.error.is_some() {
                return captured;
            }
            let captured = captured.completed.value.unwrap();
            options["observation_ref"] = captured["observation_ref"].clone();
            options["x"] = json!(captured["image"]["width"].as_f64().unwrap_or(0.0) / 2.0);
            options["y"] = json!(captured["image"]["height"].as_f64().unwrap_or(0.0) / 2.0);
            self.state.lock().unwrap().scene.pixels = true;
        } else {
            options["window_ref"] = json!(window)
        }
        // Selector admission owns menu focus preparation after capability and
        // unique-target checks, and retains its receipt in the resolution ledger.
        let input_window = options["window_ref"].as_str().map(str::to_owned);
        let follows_scene = matches!(method.as_str(), "press" | "setValue")
            && options.get("selector").is_some()
            && options["selector"]["scope"] != "menu";
        let work = self.scene_atomic(id, &method, options, deadline);
        // Keep the scene mapping for the next requested action. Its native
        // binding is consumed; preflight renews it without replaying this input.
        let mut state = self.state.lock().unwrap();
        if matches!(method.as_str(), "press" | "click" | "keypress")
            && work
                .completed
                .receipt
                .as_ref()
                .is_some_and(|r| r.outcome.dispatch != DispatchState::NotDispatched)
        {
            state.scene.pending_new_window = Some(state.windows.keys().cloned().collect());
        }
        if !follows_scene
            && work
                .completed
                .receipt
                .as_ref()
                .is_some_and(|r| r.outcome.dispatch == DispatchState::Dispatched)
        {
            state.scene.active = input_window;
        }
        if method == "press"
            && work
                .completed
                .receipt
                .as_ref()
                .is_some_and(|r| r.outcome.dispatch == DispatchState::NotDispatched)
            && work.completed.error.as_ref().is_some_and(|e| {
                matches!(
                    e.code.as_str(),
                    "element_not_pressable"
                        | "selector_unproven"
                        | "semantic_window_unavailable"
                        | "semantic_not_ready"
                        | "children_unavailable"
                        | "ax_unavailable"
                        | "modal_state_unproven"
                        | "unsupported_modal"
                        | "unsupported"
                )
            })
        {
            state.scene.pixels = true;
        }
        work
    }
    fn scene_wait_for_new_window(&self, id: &str, args: &Value, deadline: Instant) -> Work {
        let Some(before) = self.state.lock().unwrap().scene.pending_new_window.take() else {
            return failed(error(
                "input_required",
                "waitForNewWindow requires a preceding dispatched input",
            ));
        };
        let until = (Instant::now()
            + Duration::from_millis(args["timeout_ms"].as_u64().unwrap_or(2_000)))
        .min(deadline);
        let previous = self.state.lock().unwrap().scene.observed.clone();
        let mut last_projection = self.state.lock().unwrap().scene.projection();
        let timeout = |last: Option<Work>, projection: Option<Value>| {
            if until == deadline {
                return failed(error("timed_out", "Execution deadline exceeded"));
            }
            let mut result = failed(error(
                "wait_timeout",
                "No new content window appeared before the deadline; input was not repeated",
            ));
            if let Some(last) = last {
                result.completed.value = last.completed.value;
                result.completed.image = last.completed.image;
            } else {
                result.completed.value = previous.clone();
            }
            // The last attempted refresh can clear projection before its local
            // deadline expires. Preserve display evidence, never stale input
            // credentials: nodes and raster remain retired by that refresh.
            let mut state = self.state.lock().unwrap();
            state.scene.observed = result.completed.value.clone();
            state.scene.projected = projection;
            result
        };
        let mut last = None;
        loop {
            if Instant::now() >= until {
                return timeout(last, last_projection);
            }
            // A condition budget stops new probes; it is not the transport
            // deadline of an in-flight native read. Match Runtime's window-wait
            // contract, preserving the caller budget and original read errors.
            let work = self.scene_look(id, &json!({}), false, true, 0, deadline);
            if work.completed.error.is_some() {
                return work;
            }
            let matched = self
                .state
                .lock()
                .unwrap()
                .scene
                .active
                .as_ref()
                .is_some_and(|window| !before.contains(window));
            if matched {
                return work;
            }
            last_projection = self.state.lock().unwrap().scene.projection();
            last = Some(work);
            std::thread::sleep(
                Duration::from_millis(20).min(until.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn scene_pointer(&self, id: &str, op: &str, args: &Value, deadline: Instant) -> Work {
        let prepared = (|| {
            let supplied = if op == "scene.move" {
                vec![args.clone()]
            } else {
                args["path"]
                    .as_array()
                    .filter(|p| (2..=64).contains(&p.len()))
                    .cloned()
                    .ok_or_else(|| {
                        error(
                            "invalid_arguments",
                            "drag requires path with 2..64 image points",
                        )
                    })?
            };
            let raster = self.scene_validate_frame(id, &supplied, deadline)?;
            let mut samples = Vec::new();
            for (i, point) in supplied.iter().enumerate() {
                if point
                    .as_object()
                    .is_none_or(|p| p.len() != 2 || !p.contains_key("x") || !p.contains_key("y"))
                {
                    return Err(error(
                        "invalid_arguments",
                        "Each path point has exactly x and y",
                    ));
                }
                let (x, y) = (
                    point["x"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| error("invalid_arguments", "Invalid x"))?,
                    point["y"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| error("invalid_arguments", "Invalid y"))?,
                );
                if i > 0 {
                    let previous = &supplied[i - 1];
                    let (px, py) = (
                        previous["x"].as_f64().unwrap(),
                        previous["y"].as_f64().unwrap(),
                    );
                    let steps = ((x - px).hypot(y - py) / 8.0).ceil().max(1.0) as usize;
                    if samples.len().saturating_add(steps) > 256 {
                        return Err(error(
                            "invalid_arguments",
                            "Drag path exceeds the 256-sample bound",
                        ));
                    }
                    for step in 1..steps {
                        let f = step as f64 / steps as f64;
                        samples.push(json!({"x":px+(x-px)*f,"y":py+(y-py)*f}));
                    }
                }
                samples.push(point.clone());
            }
            let state = self.state.lock().unwrap();
            let mapped = samples
                .iter()
                .map(|p| raster.point(p))
                .collect::<Result<Vec<_>, _>>()?;
            let first = &mapped[0];
            if mapped.iter().any(|p| {
                p["window_ref"] != first["window_ref"]
                    || p["observation_ref"] != first["observation_ref"]
            }) {
                return Err(error(
                    "cross_window_drag",
                    "Drag path must remain in one observed window",
                ));
            }
            let target = state
                .windows
                .get(first["window_ref"].as_str().unwrap())
                .cloned()
                .ok_or_else(|| error("target_changed", "Window changed"))?;
            let observation = state
                .observations
                .iter()
                .find(|o| o.public == first["observation_ref"].as_str().unwrap())
                .map(|o| o.core)
                .ok_or_else(|| error("stale_observation", "Capture expired"))?;
            let points: Vec<_> = mapped
                .iter()
                .map(|p| PixelPoint {
                    x: p["x"].as_f64().unwrap(),
                    y: p["y"].as_f64().unwrap(),
                })
                .collect();
            if op == "scene.drag" && points.iter().all(|p| *p == points[0]) {
                return Err(error("invalid_arguments", "A drag must contain movement"));
            }
            let gesture = if op == "scene.move" {
                PointerGesture::Move(points[0])
            } else {
                PointerGesture::Drag(points)
            };
            Ok((target, observation, gesture))
        })();
        let (target, observation, gesture) = match prepared {
            Ok(v) => v,
            Err(e) => return failed(e),
        };
        let work =
            Work::action(
                self.runtime
                    .pointer_until(&target, observation, &gesture, deadline),
            );
        work
    }
    fn scene_read_text(&self, id: &str, args: &Value, deadline: Instant) -> Work {
        let Some((mut node, cached)) = self.scene_target(&args["target"]) else {
            return failed(error(
                "target_changed",
                "Text source handle is unavailable; observe it again",
            ));
        };
        // Start a fresh content observation after input consumed its binding,
        // or when returning to an older object handle. Continuations never
        // renew: they must keep the first page's exact native snapshot.
        let live = {
            let state = self.state.lock().unwrap();
            state.observations.iter().any(|observation| {
                observation.public == node["observation_ref"]
                    && !observation.retired
                    && self.runtime.retains_observation(observation.core)
            })
        };
        if args["offset"].as_u64().unwrap_or(0) == 0 && (cached || !live) {
            node = match self.scene_refresh_target(id, node, deadline) {
                Ok(node) => node,
                Err(error) => return failed(error),
            };
            let mut state = self.state.lock().unwrap();
            if let Some(current) = state
                .scene
                .nodes
                .iter_mut()
                .find(|current| current["id"] == node["id"])
            {
                *current = node.clone();
            }
            if let Some(source) = state
                .scene
                .retained_nodes
                .get_mut(node["id"].as_str().unwrap())
            {
                source["observation_ref"] = node["observation_ref"].clone();
                source["element_ref"] = node["element_ref"].clone();
            }
        }
        // Retain this exact observation across pages. Renewing here could join
        // different text revisions or revive an ID with a reused native object.
        let mut request = json!({"window_ref":node["window_ref"],
            "observation_ref":node["observation_ref"],"element_ref":node["element_ref"]});
        for key in ["offset", "limit"] {
            if let Some(value) = args.get(key) {
                request[key] = value.clone();
            }
        }
        let mut work = self.scene_atomic(id, "readText", request, deadline);
        if let Some(error) = work.completed.error.as_mut() {
            if error.code == "unsupported_capability" {
                error.message = format!(
                    "{}; readText requires native text content (target {}, role {}). Read control values from look().elements[].value",
                    error.message, node["id"].as_str().unwrap_or("unknown"),
                    node["expected"]["role"].as_str().unwrap_or("unknown")
                );
            }
        }
        if let Some(error) = &work.completed.error {
            if !matches!(
                error.code.as_str(),
                "invalid_text_range" | "unsupported_capability"
            ) {
                let mut state = self.state.lock().unwrap();
                state.scene.nodes.clear();
                state.scene.retained_nodes.clear();
                state.scene.raster = None;
                state.scene.observed = None;
                state.scene.projected = None;
            }
        }
        work
    }
    fn scene_refresh_target(
        &self,
        id: &str,
        node: Value,
        deadline: Instant,
    ) -> Result<Value, wire::WireError> {
        let Some(identity) = node["rebind_identity"].as_str() else {
            // Comparison identity alone cannot renew an object (UIA IDs can be
            // reused). Use the exact retained binding once, never a replacement.
            let state = self.state.lock().unwrap();
            return if state.observations.iter().any(|o| {
                o.public == node["observation_ref"]
                    && !o.retired
                    && self.runtime.retains_observation(o.core)
            }) {
                Ok(node)
            } else {
                Err(error(
                    "target_changed",
                    "Target cannot be rebound; inspect the returned view",
                ))
            };
        };
        // A public scene ID represents an object; Core/native observations
        // remain single-use. Renew before dispatch, never by retrying input.
        let mut query = node["query"].as_object().cloned().unwrap_or_default();
        query.insert("window_ref".into(), node["window_ref"].clone());
        query.insert("limit".into(), json!(80));
        let max_pages = semantic_page_limit(&query);
        let mut work = self.scene_atomic(id, "observe", Value::Object(query), deadline);
        let mut found = None;
        for page_index in 0..max_pages {
            if let Some(error) = work.completed.error {
                let mut state = self.state.lock().unwrap();
                state.scene.raster = None;
                state
                    .scene
                    .nodes
                    .retain(|n| n["window_ref"] != node["window_ref"]);
                state
                    .scene
                    .retained_nodes
                    .retain(|_, n| n["window_ref"] != node["window_ref"]);
                return Err(error);
            }
            let page = work.completed.value.unwrap();
            for element in page["elements"].as_array().into_iter().flatten() {
                if element["rebind_identity"] != identity {
                    continue;
                }
                if found.is_some()
                    || ["role", "name", "actions", "enabled"]
                        .iter()
                        .any(|key| element[*key] != node["expected"][*key])
                {
                    return Err(error(
                        "target_changed",
                        "Target identity or meaning changed; inspect the returned view",
                    ));
                }
                let mut next = node.clone();
                next["observation_ref"] = page["observation_ref"].clone();
                next["element_ref"] = element["element_ref"].clone();
                found = Some(next);
            }
            // Opaque object identity does not require selector uniqueness or a
            // complete tree. Do not let an unrelated later branch retire this
            // already-proven binding. Reject duplicates in the returned page.
            if found.is_some() || page["next"].is_null() || page_index + 1 == max_pages {
                break;
            }
            work = self.scene_atomic(id, "observe", json!({"cursor":page["next"]}), deadline);
        }
        found.ok_or_else(|| {
            error(
                "target_changed",
                "Target is no longer available; inspect the returned view",
            )
        })
    }
    fn scene_validate_frame(
        &self,
        id: &str,
        points: &[Value],
        deadline: Instant,
    ) -> Result<Raster, wire::WireError> {
        // Take the old mapping out before any read: a failed refresh cannot
        // leave old coordinates or consumed observations available for input.
        let previous = self
            .state
            .lock()
            .unwrap()
            .scene
            .raster
            .take()
            .ok_or_else(|| {
                error(
                    "capture_required",
                    "Call app.capture() before coordinate input",
                )
            })?;
        // Coordinates retain their application instance and canvas, not the
        // identity of a transient layer that happened to receive them before.
        let (instances, anchor) = {
            let state = self.state.lock().unwrap();
            let instances = points
                .iter()
                .map(|point| {
                    let receiver = previous.point(point)?;
                    previous
                        .layers
                        .iter()
                        .find(|layer| receiver["window_ref"] == layer.window)
                        .map(|layer| layer.instance.clone())
                        .ok_or_else(|| {
                            error(
                                "stale_scene",
                                "The original application instance is unavailable",
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let anchor = state
                .scene
                .viewport
                .as_ref()
                .map(|v| (v.1.clone(), v.2))
                .or_else(|| {
                    previous
                        .layers
                        .last()
                        .map(|layer| (layer.window.clone(), layer.bounds))
                })
                .ok_or_else(|| error("stale_scene", "The canvas anchor is unavailable"))?;
            (instances, anchor)
        };
        let mut refreshed = false;
        loop {
            let work = self.scene_atomic(id, "windows", json!({}), deadline);
            if let Some(error) = work.completed.error {
                return Err(error);
            }
            let value = work.completed.value.unwrap();
            let rows = ordered(
                value["windows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|r| r["visible"] == true)
                    .cloned()
                    .collect(),
            );
            // The canvas anchor must retain its geometry. Other app surfaces may
            // move, appear or disappear while the canvas mapping remains stable.
            if !rows
                .iter()
                .any(|row| row["window_ref"] == anchor.0 && bounds(row) == Some(anchor.1))
            {
                return Err(error(
                    "stale_scene",
                    "Canvas anchor geometry changed; inspect the returned view before new input",
                ));
            }
            let (fresh, _) = match self.scene_capture(id, &rows, deadline) {
                Ok(capture) => capture,
                Err(e)
                    if !refreshed
                        && transient_capture_error(e["code"].as_str().unwrap_or(""))
                        && Instant::now() + Duration::from_millis(80) < deadline =>
                {
                    // A popup can join the scene between inventory and capture.
                    // Refresh reads once before admission; no input has occurred.
                    refreshed = true;
                    std::thread::sleep(Duration::from_millis(80));
                    continue;
                }
                Err(e) => {
                    return Err(wire::WireError {
                        code: e["code"].as_str().unwrap_or("capture_failed").into(),
                        message: e["message"]
                            .as_str()
                            .unwrap_or("Image refresh failed")
                            .into(),
                        details: e.get("details").cloned(),
                    })
                }
            };
            if previous.bounds != fresh.bounds
                || previous.width != fresh.width
                || previous.height != fresh.height
            {
                return Err(error(
                    "stale_scene",
                    "Image canvas mapping changed; inspect the returned view before new input",
                ));
            }
            {
                let state = self.state.lock().unwrap();
                for (point, instance) in points.iter().zip(&instances) {
                    let receiver = fresh.point(point)?;
                    if !state
                        .windows
                        .get(receiver["window_ref"].as_str().unwrap())
                        .is_some_and(|target| &target.app == instance)
                    {
                        return Err(error(
                            "stale_scene",
                            "The current pixel receiver belongs to another application instance",
                        ));
                    }
                }
            }
            // A coordinate selects the current top app pixel, not a semantic object.
            // Keep the original canvas and per-point instance authorization baseline.
            // Fresh native observations authorize only this dispatch.
            let mut state = self.state.lock().unwrap();
            state.scene.raster = Some(previous);
            state.scene.pixels = true;
            return Ok(fresh);
        }
    }
    fn scene_look(
        &self,
        id: &str,
        args: &Value,
        explicit_pixels: bool,
        after: bool,
        attempt: u8,
        deadline: Instant,
    ) -> Work {
        let mut work = self.scene_look_inner(id, args, explicit_pixels, after, attempt, deadline);
        // An app view follows its current surfaces. A menu may disappear while
        // inventory/AX/pixels are being read after a successful selection. Renew
        // this read once; never renew an explicitly addressed object or replay
        // its input. Process/provider/permission failures remain terminal.
        if attempt == 0
            && args.get("target").is_none()
            && work
                .completed
                .error
                .as_ref()
                .is_some_and(|e| matches!(e.code.as_str(), "target_changed" | "stale_window"))
            && Instant::now() + Duration::from_millis(80) < deadline
        {
            std::thread::sleep(Duration::from_millis(80));
            work = self.scene_look_inner(id, args, explicit_pixels, after, 1, deadline);
        }
        if work.completed.error.is_some()
            || work.completed.value.as_ref().is_some_and(|value| {
                value["semantics"]["reason"]
                    .as_str()
                    .is_some_and(semantic_fallback)
            })
        {
            self.state.lock().unwrap().scene.retained_nodes.clear();
        }
        work
    }
    fn scene_look_inner(
        &self,
        id: &str,
        args: &Value,
        explicit_pixels: bool,
        after: bool,
        attempt: u8,
        deadline: Instant,
    ) -> Work {
        let subtree = if let Some(target) = args.get("target") {
            let Some((node, cached)) = self.scene_target(target) else {
                return failed(error(
                    "target_changed",
                    "Subtree handle is unavailable; observe it again",
                ));
            };
            // Historical object handles require new exact-object proof, not
            // a selector or a replacement with the same label.
            if cached {
                match self.scene_refresh_target(id, node, deadline) {
                    Ok(node) => Some(node),
                    Err(error) => return failed(error),
                }
            } else {
                Some(node)
            }
        } else {
            None
        };
        // A failed refresh cannot leave old semantic/pixel input authority.
        // A successful semantic read can retain the script's image mapping,
        // independently of automatic image presentation. Coordinate input still
        // refreshes pixels and proves geometry/instance before every dispatch.
        let (wait_for_window, previous_raster) = {
            let mut state = self.state.lock().unwrap();
            state.scene.nodes.clear();
            let raster = state.scene.raster.take();
            state.scene.observed = None;
            state.scene.projected = None;
            (std::mem::take(&mut state.scene.window_wait_pending), raster)
        };
        let mut startup = None;
        if self.runtime.instances().is_empty() && after {
            let current = json!({"running":false,"windows":[],"elements":[],"semantics":{"complete":true,"reason":"app_stopped"},"coverage":{"complete_within_scope":true}});
            let mut state = self.state.lock().unwrap();
            state.scene = SceneState::default();
            state.scene.observed = Some(current.clone());
            let mut view = current.clone();
            view["reset"] = json!(true);
            state.scene.projected = Some(view);
            return data(current);
        }
        if self.runtime.instances().is_empty() {
            let work = self.scene_atomic(id, "launch", json!({}), deadline);
            if work.completed.error.is_some() {
                return work;
            }
            startup = work.completed.receipt.as_ref().map(wire::receipt);
            if self.runtime.instances().is_empty() {
                return work;
            }
            // A launch establishes a new process incarnation even when this
            // Coordinator survived the previous normal quit.
            self.state.lock().unwrap().scene = SceneState::default();
        }
        let until = (Instant::now()
            + if startup.is_some() || after || wait_for_window {
                Duration::from_secs(2)
            } else {
                Duration::ZERO
            })
        .min(deadline);
        let inventory = loop {
            let work = self.scene_atomic(id, "windows", json!({}), deadline);
            if work.completed.error.is_some() {
                // A dialog button can terminate its app successfully. Only a
                // separate native incarnation-exit proof can convert that
                // final read into a stopped view; other identity errors remain.
                if after
                    && attempt == 0
                    && work
                        .completed
                        .error
                        .as_ref()
                        .is_some_and(|e| e.code == "app_changed")
                    && self
                        .runtime
                        .retire_exited_instances_until(deadline)
                        .is_ok_and(|count| count > 0)
                {
                    return self.scene_look_inner(id, args, explicit_pixels, after, 1, deadline);
                }
                return work;
            }
            let value = work.completed.value.unwrap();
            if !value["windows"].as_array().is_none_or(|w| w.is_empty()) || Instant::now() >= until
            {
                break value;
            }
            std::thread::sleep(
                Duration::from_millis(40).min(until.saturating_duration_since(Instant::now())),
            );
        };
        let mut rows = inventory["windows"].as_array().cloned().unwrap_or_default();
        rows = ordered(rows);
        let visible: Vec<_> = rows
            .iter()
            .filter(|r| r["visible"] == true)
            .cloned()
            .collect();
        // A native decoration can be a real composited surface without being
        // the active semantic window. Do not substitute keyboard focus here:
        // menus/dropdowns may be interactive above their focused owner.
        let semantic_windows: Vec<_> = visible
            .iter()
            .filter(|row| row["scene"]["keyboard_modal"] == true)
            .chain(
                visible
                    .iter()
                    .filter(|row| row["scene"]["keyboard_modal"] != true),
            )
            .chain(rows.iter().filter(|row| row["visible"] != true))
            .filter(|row| row["scene"]["semantic_candidate"] != false)
            .take(32)
            .cloned()
            .collect();
        let mut active = None;
        let window_ids = {
            let mut state = self.state.lock().unwrap();
            if state.scene.window_ids.len() > 4096 {
                state.scene = SceneState::default();
            }
            for row in &rows {
                let reference = row["window_ref"].as_str().unwrap().to_owned();
                if !state.scene.window_ids.contains_key(&reference) {
                    let id = format!("w{}", state.scene.window_ids.len() + 1);
                    state.scene.window_ids.insert(reference, id);
                }
            }
            state.scene.window_ids.clone()
        };
        let mut query: serde_json::Map<String, Value> = ["role", "text", "scope"]
            .into_iter()
            .filter_map(|key| args.get(key).map(|value| (key.to_owned(), value.clone())))
            .collect();
        if query.get("scope").and_then(Value::as_str) == Some("window") {
            query.remove("scope");
        }
        // Retain the app-menu route for exact-object renewal of descendants.
        // The native read itself uses the retained subtree, never title matching.
        if subtree
            .as_ref()
            .is_some_and(|node| node["query"]["scope"] == "menu")
        {
            query.insert("scope".into(), json!("menu"));
        }
        let mut nodes = Vec::new();
        let mut elements = Vec::new();
        let mut semantic_bytes = 0usize;
        let mut semantic = json!({"complete":false,"reason":"no_window"});
        let mut observation = None;
        for row in semantic_windows {
            if subtree
                .as_ref()
                .is_some_and(|node| node["window_ref"] != row["window_ref"])
            {
                continue;
            }
            let mut request = query.clone();
            if let Some(node) = &subtree {
                request.remove("scope");
                request.insert("observation_ref".into(), node["observation_ref"].clone());
                request.insert("element_ref".into(), node["element_ref"].clone());
            }
            request.insert("window_ref".into(), row["window_ref"].clone());
            request.insert("limit".into(), json!(80));
            let work = self.scene_atomic(id, "observe", Value::Object(request), deadline);
            // Optional window metadata can miss a newly materialized system
            // decoration. Only exact read-time proof permits choosing another
            // content window; arbitrary AX errors keep the original target.
            if work
                .completed
                .error
                .as_ref()
                .is_some_and(|e| e.code == "non_content_surface")
            {
                continue;
            }
            active = Some(row);
            observation = Some(work);
            break;
        }
        if let Some(row) = &active {
            let work = observation.unwrap();
            if let Some(e) = work.completed.error {
                if subtree.is_some() || !semantic_fallback(&e.code) {
                    return failed(e);
                }
                semantic = json!({"complete":false,"reason":e.code});
            } else if let Some(mut page) = work.completed.value {
                let mut candidates = Vec::new();
                let max_pages = semantic_page_limit(&query);
                for page_index in 0..max_pages {
                    if let Some(rows) = page["elements"].as_array() {
                        for row in rows {
                            if !query.is_empty() || useful_element(row) {
                                candidates.push((row.clone(), page["observation_ref"].clone()));
                            }
                        }
                    }
                    semantic = json!({"complete":page["coverage"]["complete"]==true && page["next"].is_null(),
                        "reason":page["coverage"]["reason"],"has_more":!page["next"].is_null()});
                    if page["next"].is_null()
                        || candidates.len() >= 80
                        || page_index + 1 == max_pages
                    {
                        break;
                    }
                    let more =
                        self.scene_atomic(id, "observe", json!({"cursor":page["next"]}), deadline);
                    if let Some(e) = more.completed.error {
                        if !semantic_fallback(&e.code) {
                            return failed(e);
                        }
                        // A failed continuation retires its observation. None
                        // of the earlier pages may publish usable element IDs.
                        candidates.clear();
                        semantic = json!({"complete":false,"reason":e.code});
                        break;
                    }
                    page = more.completed.value.unwrap();
                }
                candidates.sort_by_key(|(row, _)| element_priority(row));
                let mut identities = HashMap::<String, usize>::new();
                let mut rebinds = HashMap::<String, usize>::new();
                for (element, _) in &candidates {
                    if let Some(identity) = element["identity"].as_str() {
                        *identities.entry(identity.into()).or_default() += 1;
                    }
                    if let Some(credential) = element["rebind_identity"].as_str() {
                        *rebinds.entry(credential.into()).or_default() += 1;
                    }
                }

                let mut state = self.state.lock().unwrap();
                if state.scene.retained_ids.len() > 4096 {
                    state.scene.retained_ids.clear();
                }
                for (element, observation) in candidates {
                    if elements.len() >= 80 {
                        semantic["complete"] = json!(false);
                        semantic["reason"] = json!("output_budget");
                        semantic["has_more"] = json!(true);
                        break;
                    }
                    let retained = element["rebind_identity"]
                        .as_str()
                        .filter(|credential| rebinds.get(*credential) == Some(&1));
                    let retained_key = retained.map(|credential| {
                        format!(
                            "{}\0{}\0{}",
                            row["window_ref"].as_str().unwrap(),
                            credential,
                            element["role"]
                        )
                    });
                    let key = element["identity"]
                        .as_str()
                        .filter(|identity| identities.get(*identity) == Some(&1))
                        .map(|identity| {
                            format!(
                                "{}\0{}\0{}",
                                row["window_ref"].as_str().unwrap(),
                                identity,
                                element["role"]
                            )
                        });
                    let existing = retained_key
                        .as_ref()
                        .and_then(|key| state.scene.retained_ids.get(key))
                        .cloned()
                        .or_else(|| {
                            key.as_ref()
                                .and_then(|key| state.scene.element_ids.get(key))
                                .filter(|old| {
                                    // A changed exact-object credential overrides a reused
                                    // weaker identity, even on consecutive observations.
                                    old.retained_identity
                                        .as_deref()
                                        .zip(retained)
                                        .is_none_or(|(old, new)| old == new)
                                })
                                .map(|old| old.id.clone())
                        });
                    let target = existing.unwrap_or_else(|| {
                        state.scene.next_element += 1;
                        let target = format!(
                            "{}.e{}",
                            window_ids[row["window_ref"].as_str().unwrap()],
                            state.scene.next_element
                        );
                        target
                    });
                    if let Some(key) = key {
                        let retained_identity = retained.map(str::to_owned).or_else(|| {
                            state
                                .scene
                                .element_ids
                                .get(&key)
                                .filter(|old| old.id == target)
                                .and_then(|old| old.retained_identity.clone())
                        });
                        state.scene.element_ids.insert(
                            key,
                            DisplayIdentity {
                                id: target.clone(),
                                retained_identity,
                            },
                        );
                    }
                    if let Some(key) = retained_key {
                        state.scene.retained_ids.insert(key, target.clone());
                    }
                    let mut entry = json!({"id":target,"role":element["role"],"enabled":element["enabled"],"actions":element["actions"]});
                    let name_truncated = project_text(&mut entry, "name", &element["name"]);
                    let value_truncated = project_text(&mut entry, "value", &element["value"]);
                    if name_truncated || value_truncated {
                        semantic["text_truncated"] = json!(true);
                        if semantic["complete"] == true {
                            semantic["complete"] = json!(false);
                            semantic["reason"] = json!("text_truncated");
                        }
                    }
                    let entry_bytes = entry.to_string().len();
                    if semantic_bytes.saturating_add(entry_bytes) > 24 * 1024 {
                        semantic["complete"] = json!(false);
                        semantic["reason"] = json!("output_budget");
                        semantic["has_more"] = json!(true);
                        break;
                    }
                    nodes.push(json!({"id":target,"window_ref":row["window_ref"],"observation_ref":observation,"element_ref":element["element_ref"],
                        "rebind_identity":element["rebind_identity"].as_str().filter(|id| rebinds.get(*id) == Some(&1)), "expected":element,"query":query}));
                    semantic_bytes += entry_bytes;
                    elements.push(entry);
                }
            }
        }
        if query.is_empty()
            && semantic["complete"] == false
            && (semantic["has_more"] == true
                || (!elements.is_empty()
                    && matches!(
                        semantic["reason"].as_str(),
                        Some("node_unavailable" | "ownership_unproven" | "children_unavailable")
                    )))
        {
            semantic["hint"] = json!("Inspect a narrower view with look({role:'button'}) or look({text:'...'}); use returned target IDs.");
        }
        if subtree.is_some() && active.is_none() {
            return failed(error(
                "target_changed",
                "The subtree's original window is no longer available",
            ));
        }
        let semantic_empty =
            elements.is_empty() && (query.is_empty() || semantic["complete"] != true);
        if !query.is_empty() || subtree.is_some() {
            let mut public_query = query;
            if let Some(target) = args.get("target") {
                public_query.insert("target".into(), target.clone());
            }
            semantic["query"] = Value::Object(public_query);
        }
        let pixel_mode = explicit_pixels
            || (after && self.state.lock().unwrap().scene.pixels)
            || (subtree.is_none() && !visible.is_empty() && semantic_empty);
        let windows:Vec<_>=rows.iter().take(32).map(|r|json!({"id":window_ids[r["window_ref"].as_str().unwrap()],"title":r["title"].as_str().map(|s|s.chars().take(80).collect::<String>()),"title_truncated":r["title"].as_str().is_some_and(|s|s.chars().count()>80),"visible":r["visible"],"bounds":r["scene"]["bounds"]})).collect();
        let mut coverage = inventory["coverage"].clone();
        // Keep optional native read gaps visible without exposing the full
        // diagnostic ledger or replacing independently proven window coverage.
        if let Some(codes) = inventory["diagnostics"]["error_codes"]
            .as_array()
            .filter(|v| !v.is_empty())
        {
            coverage["read_error_codes"] = json!(codes.iter().take(8).collect::<Vec<_>>());
            if codes.len() > 8 {
                coverage["read_errors_truncated"] = json!(true);
            }
        }
        coverage["windows_returned"] = json!(windows.len());
        coverage["windows_truncated"] = json!(rows.len() > windows.len());
        let current = json!({"running":true,"windows":windows,"elements":elements,"semantics":semantic,"coverage":coverage});
        let mut image = None;
        let mut image_info = None;
        if pixel_mode && !visible.is_empty() {
            match self.scene_capture(id, &visible, deadline) {
                Ok((raster, png)) => {
                    image_info = Some(
                        json!({"width":raster.width,"height":raster.height,"bounds":raster.bounds,"coordinate_space":"image_pixels"}),
                    );
                    let mut state = self.state.lock().unwrap();
                    if state
                        .scene
                        .viewport
                        .as_ref()
                        .is_some_and(|v| v.0 != raster.bounds)
                    {
                        image_info.as_mut().unwrap()["coordinate_reset"] = json!(true);
                    }
                    let anchor = visible.last().unwrap();
                    if let Some(rect) = bounds(anchor) {
                        state.scene.viewport = Some((
                            raster.bounds,
                            anchor["window_ref"].as_str().unwrap().into(),
                            rect,
                        ));
                    }
                    drop(state);
                    image = Some(Arc::new(png));
                    let mut state = self.state.lock().unwrap();
                    state.scene.raster = Some(raster);
                    state.scene.pixels = true;
                }
                Err(e) => {
                    if terminal_read_error(e["code"].as_str().unwrap_or("")) {
                        return failed(wire::WireError {
                            code: e["code"].as_str().unwrap().into(),
                            message: e["message"].as_str().unwrap_or("Image read failed").into(),
                            details: e.get("details").cloned(),
                        });
                    }
                    if attempt == 0
                        && transient_capture_error(e["code"].as_str().unwrap_or(""))
                        && Instant::now() + Duration::from_millis(80) < deadline
                    {
                        self.state.lock().unwrap().scene.raster = None;
                        std::thread::sleep(Duration::from_millis(80));
                        let mut retried =
                            self.scene_look(id, args, explicit_pixels, after, 1, deadline);
                        if let (Some(startup), Some(value)) =
                            (startup, retried.completed.value.as_mut())
                        {
                            value["startup"] = startup;
                        }
                        return retried;
                    }
                    self.state.lock().unwrap().scene.raster = None;
                    image_info = Some(json!({"error":e}));
                }
            }
        } else {
            let mut state = self.state.lock().unwrap();
            if !visible.is_empty() {
                state.scene.raster = previous_raster;
            }
            state.scene.pixels = false;
        }
        let mut state = self.state.lock().unwrap();
        let reset = state.scene.baseline.is_none()
            || args["full"] == true
            || state.scene.baseline.as_ref().is_some_and(|previous| {
                previous["semantics"]["query"] != current["semantics"]["query"]
            });
        let mut view = if reset {
            let mut v = current.clone();
            v["reset"] = json!(true);
            v
        } else {
            delta(state.scene.baseline.as_ref().unwrap(), &current)
        };
        if let Some(info) = image_info {
            view["image"] = info
        }
        if let Some(startup) = startup {
            view["startup"] = startup
        }
        let mut snapshot = current.clone();
        if let Some(info) = view.get("image") {
            snapshot["image"] = info.clone();
        }
        state.scene.observed = Some(current);
        state.scene.projected = Some(view);
        state.scene.active = active.and_then(|r| r["window_ref"].as_str().map(str::to_owned));
        // This bounded label cache carries no input authority. Exact native
        // object credentials can retain their display IDs across filters;
        // omitted nodes themselves are still retired below. Weak identities
        // require continuous observation and cannot revive a removed target.
        state.scene.element_ids.retain(|_, cached| {
            nodes
                .iter()
                .any(|row| row["id"].as_str() == Some(cached.id.as_str()))
        });
        // Cache only native-renewable handles, with bounded metadata and no
        // value payloads. Inputs always pass scene_refresh_target again; text
        // pages retain their original read-only content observation.
        if state.scene.retained_nodes.len() >= 256 {
            state.scene.retained_nodes.clear();
        }
        for node in &nodes {
            if node["rebind_identity"].as_str().is_none() {
                continue;
            }
            let mut retained = node.clone();
            retained["expected"] = json!({"role":node["expected"]["role"],"name":node["expected"]["name"],"actions":node["expected"]["actions"],"enabled":node["expected"]["enabled"]});
            if retained.to_string().len() <= 4096 {
                state
                    .scene
                    .retained_nodes
                    .insert(node["id"].as_str().unwrap().into(), retained);
            }
        }
        state.scene.nodes = nodes;
        drop(state);
        let mut work = data(snapshot);
        work.completed.image = image;
        work
    }
    fn scene_capture(
        &self,
        id: &str,
        rows: &[Value],
        deadline: Instant,
    ) -> Result<(Raster, String), Value> {
        if rows.len() > 16 {
            return Err(
                json!({"code":"scene_limit","message":"More than 16 visible surfaces; no partial image binding created"}),
            );
        }
        let mut layers = Vec::new();
        for row in rows {
            if rows.len() > 1 && bounds(row).is_none() {
                return Err(
                    json!({"code":"scene_geometry_unavailable","message":"Provider cannot compose multiple surfaces"}),
                );
            }
            let work = self.scene_atomic(
                id,
                "capture",
                json!({"window_ref":row["window_ref"]}),
                deadline,
            );
            if let Some(e) = work.completed.error {
                return Err(error_json(&e));
            }
            let v = work
                .completed
                .value
                .ok_or(json!({"code":"invalid_image"}))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(
                    work.completed
                        .image
                        .ok_or(json!({"code":"invalid_image"}))?
                        .as_str(),
                )
                .map_err(|_| json!({"code":"invalid_image"}))?;
            let (width, height, rgba) =
                decode(&bytes).map_err(|e| json!({"code":"invalid_image","message":e}))?;
            let rect = bounds(row).unwrap_or([0.0, 0.0, f64::from(width), f64::from(height)]);
            let total: usize = layers.iter().map(|l: &Layer| l.rgba.len()).sum();
            if total.saturating_add(rgba.len()) > 64 * 1024 * 1024 {
                return Err(
                    json!({"code":"scene_limit","message":"Scene exceeds 64 MiB decoded budget"}),
                );
            }
            let instance = self.state.lock().unwrap().windows
                .get(row["window_ref"].as_str().unwrap())
                .map(|target| target.app.clone())
                .ok_or(json!({"code":"invalid_image_binding","message":"Captured surface ownership is unavailable"}))?;
            layers.push(Layer {
                instance,
                window: row["window_ref"].as_str().unwrap().into(),
                observation: v["observation_ref"]
                    .as_str()
                    .ok_or(json!({"code":"invalid_image_binding"}))?
                    .into(),
                bounds: rect,
                width,
                height,
                rgba,
            });
        }
        // Re-read layer membership and geometry after all captures. Do not bind
        // a canvas assembled across a modal, z-order or geometry transition.
        let check = self.scene_atomic(id, "windows", json!({}), deadline);
        if let Some(e) = check.completed.error {
            return Err(error_json(&e));
        }
        let after = check.completed.value.unwrap();
        let after_rows = ordered(
            after["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| r["visible"] == true)
                .cloned()
                .collect(),
        );
        let before_rows = ordered(rows.to_vec());
        for row in &before_rows {
            if !after_rows.iter().any(|r| {
                r["window_ref"] == row["window_ref"]
                    && bounds(r) == bounds(row)
                    && r["scene"]["rank"] == row["scene"]["rank"]
            }) {
                return Err(
                    json!({"code":"scene_changed","message":"App surfaces changed during capture; look again before input"}),
                );
            }
        }
        let count = after["windows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["visible"] == true)
            .count();
        if count != rows.len() {
            return Err(
                json!({"code":"scene_changed","message":"Visible surface membership changed"}),
            );
        }
        let viewport = self
            .state
            .lock()
            .unwrap()
            .scene
            .viewport
            .as_ref()
            .and_then(|v| {
                rows.iter()
                    .any(|r| r["window_ref"] == v.1 && bounds(r) == Some(v.2))
                    .then_some(v.0)
            });
        compose_in(layers, viewport)
            .map_err(|message| json!({"code":"image_encoding","message":message}))
    }
}
// Only app-relative order is a scene fact. Other apps and short-lived SCK
// clients can shift global WindowServer ranks without changing this app.
pub(super) fn ordered(mut rows: Vec<Value>) -> Vec<Value> {
    rows.sort_by_key(|r| r["scene"]["rank"].as_u64().unwrap_or(u64::MAX));
    for (rank, row) in rows.iter_mut().enumerate() {
        if row["scene"].is_object() {
            row["scene"]["rank"] = json!(rank);
        }
    }
    rows
}
impl Raster {
    fn point(&self, args: &Value) -> Result<Value, wire::WireError> {
        let frame = self;
        let (Some(x), Some(y)) = (args["x"].as_f64(), args["y"].as_f64()) else {
            return Err(error(
                "invalid_arguments",
                "Image coordinates require finite x and y",
            ));
        };
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= f64::from(frame.width)
            || y >= f64::from(frame.height)
        {
            return Err(error(
                "invalid_arguments",
                "Point is outside the returned image",
            ));
        }
        let sx = frame.bounds[0] + x / f64::from(frame.width) * frame.bounds[2];
        let sy = frame.bounds[1] + y / f64::from(frame.height) * frame.bounds[3];
        for layer in &frame.layers {
            let px = (sx - layer.bounds[0]) / layer.bounds[2] * f64::from(layer.width);
            let py = (sy - layer.bounds[1]) / layer.bounds[3] * f64::from(layer.height);
            if px >= 0.0 && py >= 0.0 && px < f64::from(layer.width) && py < f64::from(layer.height)
            {
                let index = (py as usize * layer.width as usize + px as usize) * 4;
                if layer.rgba[index + 3] > 0 {
                    return Ok(
                        json!({"window_ref":layer.window,"observation_ref":layer.observation,"x":px,"y":py}),
                    );
                }
            }
        }
        Err(error(
            "outside_app",
            "The point contains no authorized app surface",
        ))
    }
}
fn delta(previous: &Value, current: &Value) -> Value {
    let mut out = json!({"changed":previous!=current});
    for key in ["windows", "elements"] {
        let old = previous[key].as_array().unwrap();
        let new = current[key].as_array().unwrap();
        let added: Vec<_> = new
            .iter()
            .filter(|row| !old.iter().any(|o| o["id"] == row["id"]))
            .cloned()
            .collect();
        let changed: Vec<_> = new
            .iter()
            .filter(|row| old.iter().any(|o| o["id"] == row["id"] && o != *row))
            .cloned()
            .collect();
        let removed: Vec<_> = old
            .iter()
            .filter(|row| !new.iter().any(|n| n["id"] == row["id"]))
            .map(|r| r["id"].clone())
            .collect();
        let before_order: Vec<_> = old.iter().map(|r| r["id"].clone()).collect();
        let order: Vec<_> = new.iter().map(|r| r["id"].clone()).collect();
        if !added.is_empty() || !changed.is_empty() || !removed.is_empty() || before_order != order
        {
            out[key] = json!({"added":added,"changed":changed,"removed":removed});
            if before_order != order {
                out[key]["order"] = json!(order);
            }
        }
    }
    for key in ["running", "semantics", "coverage"] {
        if previous[key] != current[key] {
            out[key] = current[key].clone()
        }
    }
    out
}
fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("Invalid image size")?;
    if size > 16 * 1024 * 1024 {
        return Err("Surface exceeds 16 MiB decoded budget".into());
    }
    let mut bytes = vec![0; size];
    let info = reader.next_frame(&mut bytes).map_err(|e| e.to_string())?;
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for pixel in bytes[..info.buffer_size()].chunks_exact(info.color_type.samples()) {
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(pixel),
            png::ColorType::Rgb => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            png::ColorType::Grayscale => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255])
            }
            png::ColorType::GrayscaleAlpha => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]])
            }
            _ => return Err("Unsupported image color type".into()),
        }
    }
    Ok((info.width, info.height, rgba))
}
#[cfg(test)]
fn compose(layers: Vec<Layer>) -> Result<(Raster, String), String> {
    compose_in(layers, None)
}
fn compose_in(layers: Vec<Layer>, viewport: Option<[f64; 4]>) -> Result<(Raster, String), String> {
    let x = layers
        .iter()
        .map(|l| l.bounds[0])
        .fold(f64::INFINITY, f64::min);
    let y = layers
        .iter()
        .map(|l| l.bounds[1])
        .fold(f64::INFINITY, f64::min);
    let right = layers
        .iter()
        .map(|l| l.bounds[0] + l.bounds[2])
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = layers
        .iter()
        .map(|l| l.bounds[1] + l.bounds[3])
        .fold(f64::NEG_INFINITY, f64::max);
    let bounds = [x, y, right - x, bottom - y];
    if !bounds.iter().all(|v| v.is_finite()) || bounds[2] <= 0.0 || bounds[3] <= 0.0 {
        return Err("Invalid scene geometry".into());
    }
    let bounds = if let Some(old) = viewport {
        let left = bounds[0].min(old[0]);
        let top = bounds[1].min(old[1]);
        [
            left,
            top,
            (bounds[0] + bounds[2]).max(old[0] + old[2]) - left,
            (bounds[1] + bounds[3]).max(old[1] + old[3]) - top,
        ]
    } else {
        bounds
    };
    let scale = (1600.0 / bounds[2].max(bounds[3])).min(2.0);
    let width = (bounds[2] * scale).round().max(1.0) as u32;
    let height = (bounds[3] * scale).round().max(1.0) as u32;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for layer in layers.iter().rev() {
        for py in 0..height {
            let sy = bounds[1] + (f64::from(py) + 0.5) / f64::from(height) * bounds[3];
            if sy < layer.bounds[1] || sy >= layer.bounds[1] + layer.bounds[3] {
                continue;
            }
            for px in 0..width {
                let sx = bounds[0] + (f64::from(px) + 0.5) / f64::from(width) * bounds[2];
                if sx < layer.bounds[0] || sx >= layer.bounds[0] + layer.bounds[2] {
                    continue;
                }
                let lx =
                    ((sx - layer.bounds[0]) / layer.bounds[2] * f64::from(layer.width)) as usize;
                let ly =
                    ((sy - layer.bounds[1]) / layer.bounds[3] * f64::from(layer.height)) as usize;
                let source = &layer.rgba[(ly * layer.width as usize + lx) * 4..][..4];
                let dest = &mut rgba[(py as usize * width as usize + px as usize) * 4..][..4];
                let a = u32::from(source[3]);
                let da = u32::from(dest[3]);
                let alpha = a * 255 + da * (255 - a);
                if alpha > 0 {
                    for c in 0..3 {
                        dest[c] = ((u32::from(source[c]) * a * 255
                            + u32::from(dest[c]) * da * (255 - a))
                            / alpha) as u8;
                    }
                    dest[3] = (alpha / 255) as u8;
                }
            }
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok((
        Raster {
            bounds,
            width,
            height,
            layers,
        },
        base64::engine::general_purpose::STANDARD.encode(png),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layer(window: &str, rect: [f64; 4], color: [u8; 4]) -> Layer {
        Layer {
            instance: AppInstance {
                identity: AppIdentity("org.example.Scene".into()),
                pid: 42,
                generation: 1,
            },
            window: window.into(),
            observation: format!("obs_{window}"),
            bounds: rect,
            width: 2,
            height: 2,
            rgba: color.repeat(4),
        }
    }
    #[test]
    fn app_relative_order_ignores_unrelated_global_rank_shifts() {
        let first = vec![
            json!({"window_ref":"a","scene":{"bounds":[0,0,10,10],"rank":2}}),
            json!({"window_ref":"b","scene":{"bounds":[5,5,10,10],"rank":4}}),
        ];
        let mut shifted = first.clone();
        shifted[0]["scene"]["rank"] = json!(7);
        shifted[1]["scene"]["rank"] = json!(15);
        assert_eq!(ordered(first.clone()), ordered(shifted));
        let mut swapped = first.clone();
        swapped[1]["scene"]["rank"] = json!(0);
        assert_ne!(ordered(first), ordered(swapped));
    }
    #[test]
    fn compositor_preserves_layering_alpha_and_negative_screen_origin() {
        let (raster, png) = compose(vec![
            layer("top", [0.0, 0.0, 2.0, 2.0], [255, 0, 0, 128]),
            layer("base", [-2.0, -2.0, 4.0, 4.0], [0, 0, 255, 255]),
        ])
        .unwrap();
        assert_eq!(raster.bounds, [-2.0, -2.0, 4.0, 4.0]);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(png)
            .unwrap();
        let (w, h, rgba) = decode(&bytes).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(&rgba[0..4], &[0, 0, 255, 255]);
        let center = (6 * w as usize + 6) * 4;
        assert_eq!(&rgba[center..center + 4], &[128, 0, 127, 255]);
    }
    #[test]
    fn scene_coordinates_hit_top_alpha_and_reject_empty_canvas_space() {
        let (raster, _) = compose(vec![
            layer("top", [0.0, 0.0, 2.0, 2.0], [255, 0, 0, 255]),
            layer("base", [-2.0, -2.0, 2.0, 2.0], [0, 0, 255, 255]),
        ])
        .unwrap();
        let point = raster.point(&json!({"x":6,"y":6})).unwrap();
        assert_eq!(point["window_ref"], "top");
        assert_eq!(point["x"], 1.0);
        assert_eq!(
            raster.point(&json!({"x":1,"y":6})).unwrap_err().code,
            "outside_app"
        );
        assert_eq!(
            raster.point(&json!({"x":8,"y":0})).unwrap_err().code,
            "invalid_arguments"
        );
    }
    #[test]
    fn closing_an_overlay_preserves_the_canvas_and_existing_base_pixel_mapping() {
        let (before, _) = compose(vec![
            layer("popup", [1.0, -4.0, 3.0, 2.0], [255, 0, 0, 255]),
            layer("base", [-2.0, -2.0, 4.0, 4.0], [0, 0, 255, 255]),
        ])
        .unwrap();
        let (after, _) = compose_in(
            vec![layer("base", [-2.0, -2.0, 4.0, 4.0], [0, 0, 255, 255])],
            Some(before.bounds),
        )
        .unwrap();
        assert_eq!(
            (before.bounds, before.width, before.height),
            (after.bounds, after.width, after.height)
        );
        assert_eq!(
            before.point(&json!({"x":2,"y":8})).unwrap(),
            after.point(&json!({"x":2,"y":8})).unwrap()
        );
        assert_eq!(
            after.point(&json!({"x":10,"y":1})).unwrap_err().code,
            "outside_app"
        );
    }
    #[test]
    fn closing_left_or_top_overlay_keeps_pixels_aligned_with_retained_viewport_points() {
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
        ];
        let base = || {
            let mut base = layer("base", [0.0, 0.0, 2.0, 2.0], [0; 4]);
            base.rgba = colors.concat();
            base
        };
        for popup in [
            [-2.0, 0.0, 2.0, 2.0],
            [0.0, -2.0, 2.0, 2.0],
            [-2.0, -2.0, 2.0, 2.0],
        ] {
            let (before, _) = compose(vec![layer("popup", popup, [255; 4]), base()]).unwrap();
            let (after, png) = compose_in(vec![base()], Some(before.bounds)).unwrap();
            assert_eq!(
                (after.bounds, after.width, after.height),
                (before.bounds, before.width, before.height)
            );
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(png)
                .unwrap();
            let (width, height, rgba) = decode(&bytes).unwrap();
            assert_eq!((width, height), (after.width, after.height));
            for y in 0..height {
                for x in 0..width {
                    let point = json!({"x":f64::from(x) + 0.5,"y":f64::from(y) + 0.5});
                    let sx = before.bounds[0]
                        + (f64::from(x) + 0.5) / f64::from(width) * before.bounds[2];
                    let sy = before.bounds[1]
                        + (f64::from(y) + 0.5) / f64::from(height) * before.bounds[3];
                    let pixel = &rgba[((y * width + x) * 4) as usize..][..4];
                    if (0.0..2.0).contains(&sx) && (0.0..2.0).contains(&sy) {
                        assert_eq!(
                            pixel,
                            &colors[sy as usize * 2 + sx as usize],
                            "popup={popup:?}, pixel=({x},{y})"
                        );
                        let mapped = after.point(&point).unwrap();
                        assert_eq!(mapped, before.point(&point).unwrap());
                        assert_eq!(mapped["window_ref"], "base");
                        assert_eq!(mapped["x"], sx);
                        assert_eq!(mapped["y"], sy);
                    } else {
                        assert_eq!(
                            pixel,
                            &[0, 0, 0, 0],
                            "vacated popup={popup:?}, pixel=({x},{y})"
                        );
                        assert_eq!(after.point(&point).unwrap_err().code, "outside_app");
                    }
                }
            }
        }
    }

    #[test]
    fn delta_reports_disappearance_and_reordering_without_repeating_unchanged_controls() {
        let old = json!({"windows":[{"id":"w1"},{"id":"w2"}],"elements":[{"id":"e1","name":"Save"}],"semantics":{"complete":true},"coverage":{}});
        let mut new = old.clone();
        new["windows"] = json!([{"id":"w2"}]);
        let diff = delta(&old, &new);
        assert_eq!(diff["windows"]["removed"], json!(["w1"]));
        assert!(diff.get("elements").is_none());
    }
}
