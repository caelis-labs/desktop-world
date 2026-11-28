//! Agent-facing app scene. Native refs, layering and image bindings stay here.
use super::operations::{capture_region, MAX_IMAGE_DIMENSION};
use super::*;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::Cursor;

const ACTION_READ_DELAY: Duration = Duration::from_millis(1_200);
const FOREGROUND_CLICK_READ_DELAY: Duration = Duration::from_millis(220);
const MAX_DISPLAY_IDENTITIES: usize = 32_768;

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
    capture_options: CaptureOptions,
    projected: Option<Value>,
    element_ids: HashMap<String, DisplayIdentity>,
    retained_ids: HashMap<String, String>,
    retained_nodes: HashMap<String, Value>,
    next_element: u64,
    viewport: Option<([f64; 4], String, [f64; 4])>,
    window_wait_pending: bool,
    pending_new_window: Option<NewWindowBaseline>,
    read_after: Option<Instant>,
}
#[derive(Clone, Copy)]
struct CaptureOptions {
    region: Option<[f64; 4]>,
    max_dimension: u32,
    semantics: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            region: None,
            max_dimension: MAX_IMAGE_DIMENSION,
            semantics: true,
        }
    }
}

#[derive(Clone)]
pub(super) struct NewWindowBaseline {
    instance: AppInstance,
    windows: Vec<WindowTarget>,
}

impl NewWindowBaseline {
    fn matches(&self, target: &WindowTarget) -> bool {
        self.instance == target.app && !self.windows.contains(target)
    }
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
    bounds: [f64; 4],
    pixels: LayerPixels,
}
struct LayerPixels {
    observation: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
struct Raster {
    bounds: [f64; 4],
    area: [f64; 4],
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    foreground_png: Option<Arc<String>>,
}

pub(super) fn validate(op: &str, args: &Value) -> Result<Value, wire::WireError> {
    let name = op.strip_prefix("scene.").unwrap_or(op);
    super::operations::validate(if name == "after" { "capture" } else { name }, args)
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
            | "windows_window_offscreen"
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

fn covers_point(row: &Value, point: Option<[f64; 2]>) -> bool {
    match (bounds(row), point) {
        (Some(bounds), Some(point)) => contains_point(bounds, point),
        _ => true,
    }
}

fn contains_point([left, top, width, height]: [f64; 4], [x, y]: [f64; 2]) -> bool {
    x >= left && x < left + width && y >= top && y < top + height
}

fn intersects_bounds(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

fn contains_bounds(outer: [f64; 4], inner: [f64; 4]) -> bool {
    inner[0] >= outer[0]
        && inner[1] >= outer[1]
        && inner[0] + inner[2] <= outer[0] + outer[2]
        && inner[1] + inner[3] <= outer[1] + outer[3]
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
        self.scene_atomic_with_baseline(id, op, args, deadline, None)
    }

    fn scene_atomic_with_baseline(
        &self,
        id: &str,
        op: &str,
        args: Value,
        deadline: Instant,
        before: Option<NewWindowBaseline>,
    ) -> Work {
        let submitted = match wire::script_submit(&self.app_ref, id, op, &args) {
            Ok(v) => v,
            Err(e) => return failed(e),
        };
        let mut work = match &submitted.command {
            Command::PressSelected {
                selector,
                timeout_ms,
            }
            | Command::SetValueSelected {
                selector,
                timeout_ms,
                ..
            }
            | Command::Wait {
                selector,
                timeout_ms,
                ..
            } => self.select_scene_element(&submitted, selector, *timeout_ms, deadline),
            _ if is_action(&submitted.command) && submitted.command != Command::Launch => {
                self.scene_execute_input(&submitted, deadline, before)
            }
            _ => self.execute_atomic(&submitted, deadline),
        };
        let publication = std::mem::replace(&mut work.publication, Publication::None);
        apply_publication(&mut self.state.lock().unwrap(), publication);
        work
    }
    pub(super) fn scene_receiver(
        &self,
        rows: &[Value],
        instance: Option<&AppInstance>,
        keyboard: bool,
    ) -> Option<String> {
        let state = self.state.lock().unwrap();
        let target = |row: &Value| state.windows.get(row["window_ref"].as_str()?);
        let instance = instance.or_else(|| {
            rows.iter()
                .filter(|row| row["visible"] == true)
                .find_map(|row| target(row).map(|target| &target.app))
        });
        let candidates = || {
            rows.iter().filter(|row| {
                row["visible"] == true
                    && target(row).is_some_and(|target| Some(&target.app) == instance)
            })
        };
        // A modal has priority over its owner's potentially stale focus hint.
        candidates()
            .find(|row| {
                row["scene"]["keyboard_modal"] == true
                    && row["scene"]["semantic_candidate"] != false
            })
            .or_else(|| {
                keyboard
                    .then(|| candidates().find(|row| row["scene"]["keyboard_focused"] == true))
                    .flatten()
            })
            .or_else(|| candidates().find(|row| row["scene"]["semantic_candidate"] != false))
            .and_then(|row| row["window_ref"].as_str().map(str::to_owned))
    }

    fn scene_input_instance(&self, window: Option<&str>) -> Option<AppInstance> {
        if let Some(window) = window {
            return self
                .state
                .lock()
                .unwrap()
                .windows
                .get(window)
                .map(|target| target.app.clone());
        }
        let mut instances = self.runtime.instances();
        (instances.len() == 1).then(|| instances.pop().unwrap())
    }

    fn scene_input_baseline(
        &self,
        instance: Option<AppInstance>,
        deadline: Instant,
    ) -> Option<NewWindowBaseline> {
        let instance = instance?;
        let inventory = self.runtime.windows_until(deadline).ok()?;
        // Runtime omits an instance's rows if its inventory could not prove
        // every candidate within the provider's declared scope.
        if !inventory
            .instances
            .iter()
            .any(|entry| entry.app == instance && entry.report.is_ok())
        {
            return None;
        }
        Some(NewWindowBaseline {
            windows: inventory
                .windows
                .into_iter()
                .map(|row| row.target)
                .filter(|target| target.app == instance)
                .collect(),
            instance,
        })
    }

    pub(super) fn scene_baseline_from_rows(
        &self,
        instance: &AppInstance,
        rows: &[Value],
    ) -> NewWindowBaseline {
        // A freshly selected receiver proves this member's inventory passed
        // Runtime validation, even when another member's inventory failed.
        let state = self.state.lock().unwrap();
        NewWindowBaseline {
            instance: instance.clone(),
            windows: rows
                .iter()
                .filter_map(|row| state.windows.get(row["window_ref"].as_str()?))
                .filter(|target| &target.app == instance)
                .cloned()
                .collect(),
        }
    }

    fn scene_record_input(&self, work: &Work, before: Option<NewWindowBaseline>) {
        // An unknown receipt can still accompany a real UI change. Keep its
        // original outcome and use the baseline only for a read-only wait.
        if work
            .completed
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.outcome.dispatch != DispatchState::NotDispatched)
        {
            self.state.lock().unwrap().scene.pending_new_window = before;
        }
    }

    pub(super) fn scene_execute_input(
        &self,
        submitted: &Submit,
        deadline: Instant,
        before: Option<NewWindowBaseline>,
    ) -> Work {
        let window = match &submitted.command {
            Command::Quit { window_ref } => window_ref.as_deref(),
            _ => submitted.target.window_ref.as_deref(),
        };
        let before = before
            .or_else(|| self.scene_input_baseline(self.scene_input_instance(window), deadline));
        let work = self.execute_atomic(submitted, deadline);
        self.scene_record_input(&work, before);
        work
    }
    pub(super) fn scene_call(&self, id: &str, op: &str, args: &Value, deadline: Instant) -> Work {
        let canonical = match validate(op, args) {
            Ok(args) => args,
            Err(error) => return failed(error),
        };
        let args = &canonical;
        if op == "scene.click"
            && args["modifiers"]
                .as_array()
                .is_some_and(|modifiers| !modifiers.is_empty())
        {
            return Work::action(ActionReceipt {
                outcome: ActionOutcome::rejected(Error::UnsupportedCapability),
                provider_outcome: None,
                interruption: None,
            });
        }
        if matches!(op, "scene.look" | "scene.capture" | "scene.after")
            || (op == "scene.typeText" && args.get("target").is_none())
        {
            // An earlier click or shortcut can transfer editing focus
            // asynchronously. Let that transition run before implicit typing.
            if let Err(error) = self.scene_wait_for_update(id, deadline) {
                return Work::read_error(error);
            }
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
        if op == "scene.wait" {
            return self.scene_wait(id, args, deadline);
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
            if work
                .completed
                .receipt
                .as_ref()
                .is_some_and(|receipt| receipt.outcome.dispatch == DispatchState::Dispatched)
            {
                self.state.lock().unwrap().scene.window_wait_pending = true;
            }
            return work;
        }
        let menu_capability = match op {
            "scene.invokeMenu" => Some(Capability::InvokeMenu),
            "scene.showMenu" => Some(Capability::ShowMenu),
            "scene.selectOption" => Some(Capability::SelectOption),
            _ => None,
        };
        if menu_capability.is_some_and(|capability| !self.supported.contains(&capability)) {
            return Work::action(ActionReceipt {
                outcome: ActionOutcome::rejected(Error::UnsupportedCapability),
                provider_outcome: None,
                interruption: None,
            });
        }
        if matches!(op, "scene.move" | "scene.drag") {
            return self.scene_pointer(id, op, args, deadline);
        }
        // Known sequential steps can open a new dialog/window in the same
        // script. Resolve implicit input routing from a fresh app inventory;
        // explicit semantic IDs and pixels remain bound to what was observed.
        let implicit = matches!(
            op,
            "scene.keypress" | "scene.focus" | "scene.invokeMenu" | "scene.waitFor"
        ) || (op == "scene.typeText" && args.get("target").is_none())
            || (op == "scene.scroll" && args.get("x").is_none() && args.get("target").is_none())
            || (matches!(op, "scene.click" | "scene.setValue") && args.get("selector").is_some());
        if op == "scene.waitFor" && self.runtime.instances().is_empty() {
            return failed(error(
                "selector_unproven",
                "No running application receiver is available to observe",
            ));
        }
        if implicit && self.runtime.instances().is_empty() {
            // Reuse the existing launch/readiness path only for stopped apps.
            // Running apps need inventory, not a redundant semantic look before
            // a known selector, key chord, text insertion or default scroll.
            let prepared = self.scene_look(id, &json!({}), false, false, 0, deadline);
            if prepared.completed.error.is_some() || self.runtime.instances().is_empty() {
                return prepared;
            }
        }
        let mut before = None;
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
                    .or_else(|| {
                        state
                            .scene
                            .pending_new_window
                            .as_ref()
                            .map(|before| before.instance.clone())
                    })
            };
            let wait = self.state.lock().unwrap().scene.window_wait_pending;
            let until = (Instant::now()
                + if wait {
                    Duration::from_secs(2)
                } else {
                    Duration::ZERO
                })
            .min(deadline);
            let rows =
                loop {
                    let mut work = self.scene_atomic(id, "windows", json!({}), deadline);
                    // A transient popup can disappear during this read-only
                    // inventory, before the requested payload has been admitted.
                    // Use the same single complete-read renewal as look(); never
                    // retry an input, a changed app incarnation, or a focus refusal.
                    if work.completed.error.as_ref().is_some_and(|e| {
                        matches!(e.code.as_str(), "target_changed" | "stale_window")
                    }) && !self.runtime.instances().is_empty()
                        && Instant::now() + Duration::from_millis(80) < deadline
                    {
                        std::thread::sleep(Duration::from_millis(80));
                        work = self.scene_atomic(id, "windows", json!({}), deadline);
                    }
                    if work.completed.error.is_some() {
                        return work;
                    }
                    let value = work.completed.value.unwrap();
                    let rows = ordered(value["windows"].as_array().cloned().unwrap_or_default());
                    if rows.iter().any(|row| row["visible"] == true) || Instant::now() >= until {
                        break rows;
                    }
                    std::thread::sleep(
                        Duration::from_millis(20)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                };
            self.state.lock().unwrap().scene.window_wait_pending = false;
            let keyboard = matches!(op, "scene.typeText" | "scene.keypress" | "scene.invokeMenu");
            let selected = self.scene_receiver(&rows, selected_instance.as_ref(), keyboard);
            if selected.is_none() && rows.iter().any(|row| row["visible"] == true) {
                return failed(error(
                    "target_changed",
                    "The selected application instance lost its window; inspect the returned view",
                ));
            }
            if keyboard || op == "scene.focus" {
                before = self
                    .scene_input_instance(selected.as_deref())
                    .map(|instance| self.scene_baseline_from_rows(&instance, &rows));
            }
            self.state.lock().unwrap().scene.active = selected;
        }
        let window = self.state.lock().unwrap().scene.active.clone();
        let coordinate_input =
            matches!(op, "scene.click" | "scene.scroll") && args.get("x").is_some();
        if window.is_none() && !coordinate_input {
            if op == "scene.focus" && !self.runtime.instances().is_empty() {
                let work = Work::action(self.runtime.app_focus_until(deadline));
                self.scene_record_input(&work, before);
                return work;
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
                    self.scene_record_input(&work, before);
                    if work.completed.receipt.as_ref().is_some_and(|receipt| {
                        receipt.outcome.dispatch != DispatchState::NotDispatched
                    }) {
                        self.state.lock().unwrap().scene.window_wait_pending = true;
                    }
                    return work;
                }
            }
            if op == "scene.waitFor" {
                return failed(error(
                    "selector_unproven",
                    "No running application receiver is available to observe",
                ));
            }
            return failed(error(
                "look_required",
                "No current editing surface; inspect the app with look or capture",
            ));
        }
        if op == "scene.waitFor" {
            let state = args["state"].as_str().unwrap_or("attached");
            let wire_state = match state {
                "attached" => "present",
                "detached" => "absent",
                state => state,
            };
            let mut options =
                json!({"window_ref":window,"selector":args["selector"],"state":wire_state});
            if let Some(value) = args.get("value") {
                options["value"] = value.clone();
            }
            if let Some(timeout) = args.get("timeout") {
                options["timeout_ms"] = timeout.clone();
            }
            let mut work = self.scene_atomic(id, "wait", options, deadline);
            if let Some(value) = &mut work.completed.value {
                let mut result = json!({"state":state});
                if let Some(element) = value.get("element") {
                    result["element"] = json!({"role":element["role"],"name":element["name"],
                        "value":element["value"],"enabled":element["enabled"],"actions":element["actions"]});
                }
                *value = result;
            }
            return work;
        }
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
            if (args.get("target").is_some() || args.get("selector").is_some())
                && ["button", "count", "modifiers"]
                    .iter()
                    .any(|key| args.get(*key).is_some())
            {
                return failed(error(
                    "invalid_arguments",
                    "Mouse options require image coordinates",
                ));
            }
            if let Some(target) = args.get("target") {
                let Some((node, _)) = self.scene_target(target) else {
                    return failed(error(
                        "target_changed",
                        "Target handle is unavailable; inspect the returned view",
                    ));
                };
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
                method = if setter { "setValue" } else { "press" }.into();
                options["window_ref"] = json!(window);
            } else {
                options = match self.scene_resolve_points(
                    id,
                    std::slice::from_ref(args),
                    args["unit"] == "fraction",
                    deadline,
                ) {
                    Ok(mut mapped) => mapped.remove(0),
                    Err(e) => return failed(e),
                };
                for key in ["button", "count", "modifiers"] {
                    if let Some(v) = args.get(key) {
                        options[key] = v.clone();
                    }
                }
            }
        } else if matches!(
            op,
            "scene.showMenu" | "scene.selectOption" | "scene.typeText" | "scene.scroll"
        ) && args.get("target").is_some()
        {
            let Some((node, _)) = self.scene_target(&args["target"]) else {
                return failed(error(
                    "target_changed",
                    "Target is unavailable; observe the control again",
                ));
            };
            let node = match self.scene_refresh_target(id, node, deadline) {
                Ok(node) => node,
                Err(error) => return failed(error),
            };
            let options = options.as_object_mut().expect("validated argument object");
            options.remove("target");
            for key in ["window_ref", "observation_ref", "element_ref"] {
                options.insert(key.into(), node[key].clone());
            }
        } else if op == "scene.scroll" && (args.get("x").is_some() || args.get("y").is_some()) {
            options = match self.scene_resolve_points(
                id,
                std::slice::from_ref(args),
                args["unit"] == "fraction",
                deadline,
            ) {
                Ok(mut mapped) => mapped.remove(0),
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
        let work = self.scene_atomic_with_baseline(id, &method, options, deadline, before);
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
            let delay = if coordinate_input && self.runtime.uses_foreground_capture() {
                FOREGROUND_CLICK_READ_DELAY
            } else {
                ACTION_READ_DELAY
            };
            state.scene.read_after = Some(Instant::now() + delay);
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
    fn scene_wait_for_update(&self, id: &str, deadline: Instant) -> Result<(), Error> {
        let until = self.state.lock().unwrap().scene.read_after;
        if let Some(until) = until {
            while Instant::now() < until {
                self.check_execution_wait(id, deadline)?;
                std::thread::sleep(
                    Duration::from_millis(20).min(
                        until
                            .min(deadline)
                            .saturating_duration_since(Instant::now()),
                    ),
                );
            }
            self.check_execution_wait(id, deadline)?;
            self.state.lock().unwrap().scene.read_after = None;
        }
        Ok(())
    }
    fn scene_wait(&self, id: &str, args: &Value, deadline: Instant) -> Work {
        let delay = args["ms"].as_u64();
        let budget = delay
            .or_else(|| args["timeout_ms"].as_u64())
            .unwrap_or(2_000);
        let until = (Instant::now() + Duration::from_millis(budget)).min(deadline);
        let mut query = json!({});
        for key in ["role", "text"] {
            if let Some(value) = args.get(key) {
                query[key] = value.clone();
            }
        }
        let filtered = query != json!({});
        if let Some(title) = args["title"].as_str() {
            query["_wait_title"] = json!(title.to_lowercase());
        }
        if delay.is_none() {
            query["_wait_view"] = json!(true);
        }
        let stable = args["window"] == "stable";
        let earliest_sample = if stable && !filtered && args.get("title").is_none() {
            self.state.lock().unwrap().scene.read_after
        } else {
            None
        };
        let mut previous = None;
        let mut last: Option<Work> = None;
        loop {
            if let Err(reason) = self.check_execution_wait(id, deadline) {
                return Work::read_error(reason);
            }
            if Instant::now() >= until {
                if delay.is_some() {
                    return data(Value::Null);
                }
                let mut work = failed(error(
                    "wait_timeout",
                    "The requested window or element was not observed in time",
                ));
                work.completed.value = last.and_then(|work| work.completed.value);
                return work;
            }
            if delay.is_none() && earliest_sample.is_none_or(|at| Instant::now() >= at) {
                let work = self.scene_look(id, &query, false, false, 0, deadline);
                if work.completed.error.is_some() {
                    return work;
                }
                let Some(snapshot) = &work.completed.value else {
                    return failed(error(
                        "observation_unavailable",
                        "Wait could not read the app",
                    ));
                };
                let matched = snapshot["windows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .next()
                    .filter(|row| {
                        row["visible"] == true
                            && row["bounds"].is_array()
                            && (!filtered
                                || snapshot["elements"].as_array().into_iter().flatten().any(
                                    |element| {
                                        element["id"]
                                            .as_str()
                                            .and_then(|id| id.split_once('.'))
                                            .is_some_and(|(window, _)| row["id"] == window)
                                    },
                                ))
                    });
                if let Some(row) = matched {
                    if !stable || previous.as_ref() == Some(row) {
                        self.state.lock().unwrap().scene.read_after = None;
                        return work;
                    }
                }
                previous = matched.cloned();
                if snapshot.get("running").is_some() {
                    last = Some(work);
                }
            }
            let next = if stable {
                150
            } else if delay.is_some() {
                20
            } else {
                80
            };
            let wake = (Instant::now() + Duration::from_millis(next)).min(until);
            while Instant::now() < wake {
                if let Err(reason) = self.check_execution_wait(id, deadline) {
                    return Work::read_error(reason);
                }
                std::thread::sleep(
                    Duration::from_millis(20).min(wake.saturating_duration_since(Instant::now())),
                );
            }
        }
    }

    fn scene_wait_for_new_window(&self, id: &str, args: &Value, deadline: Instant) -> Work {
        let Some(before) = self.state.lock().unwrap().scene.pending_new_window.clone() else {
            return failed(error(
                "input_required",
                "waitForNewWindow requires an unused baseline from a preceding input that may have reached the app",
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
            if let Err(reason) = self.check_execution_wait(id, deadline) {
                return Work::read_error(reason);
            }
            if Instant::now() >= until {
                return timeout(last, last_projection);
            }
            // Keep one usable snapshot for a timeout. Subsequent probes need
            // only inventory until a candidate is ready for a scene read.
            if last.is_some() {
                let has_candidate = if self.runtime.instances().contains(&before.instance) {
                    let inventory = self.scene_atomic(id, "windows", json!({}), deadline);
                    if inventory.completed.error.is_some() {
                        return inventory;
                    }
                    let rows = inventory.completed.value.unwrap();
                    let state = self.state.lock().unwrap();
                    rows["windows"].as_array().into_iter().flatten().any(|row| {
                        row["visible"] == true
                            && row["scene"]["semantic_candidate"] != false
                            && row["window_ref"]
                                .as_str()
                                .and_then(|reference| state.windows.get(reference))
                                .is_some_and(|target| before.matches(target))
                    })
                } else {
                    false
                };
                if !has_candidate || Instant::now() >= until {
                    std::thread::sleep(
                        Duration::from_millis(20)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                    continue;
                }
            }
            // A condition budget stops new probes. Any in-flight native read
            // keeps the enclosing deadline and its original error semantics.
            let work = self.scene_look(
                id,
                &json!({"_wait_new_window":true}),
                false,
                true,
                0,
                deadline,
            );
            if work.completed.error.is_some() {
                return work;
            }
            let mut state = self.state.lock().unwrap();
            if state
                .scene
                .active
                .as_deref()
                .and_then(|window| state.windows.get(window))
                .is_some_and(|target| before.matches(target))
            {
                state.scene.pending_new_window = None;
                state.scene.read_after = None;
                return work;
            }
            last_projection = state.scene.projection();
            drop(state);
            last = Some(work);
            std::thread::sleep(
                Duration::from_millis(20).min(until.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn scene_pointer(&self, id: &str, op: &str, args: &Value, deadline: Instant) -> Work {
        let prepared = (|| {
            let supplied = if op == "scene.move" {
                vec![json!({"x":args["x"],"y":args["y"]})]
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
            let fraction = args["unit"] == "fraction";
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
            let mapped = self.scene_resolve_points(id, &samples, fraction, deadline)?;
            let state = self.state.lock().unwrap();
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
        let before = self.scene_input_baseline(Some(target.app.clone()), deadline);
        let work =
            Work::action(
                self.runtime
                    .pointer_until(&target, observation, &gesture, deadline),
            );
        self.scene_record_input(&work, before);
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
    fn scene_resolve_points(
        &self,
        id: &str,
        points: &[Value],
        fraction: bool,
        deadline: Instant,
    ) -> Result<Vec<Value>, wire::WireError> {
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
        if self.runtime.uses_foreground_capture() {
            let mapped =
                self.scene_bind_foreground_points(&previous, points, fraction, deadline)?;
            let mut state = self.state.lock().unwrap();
            state.scene.raster = Some(previous);
            state.scene.pixels = true;
            return Ok(mapped);
        }
        let original_receivers = points
            .iter()
            .map(|point| previous.hit(point, fraction).map(|(layer, _, _)| layer))
            .collect::<Result<Vec<_>, _>>()?;
        let single_point = if let [point] = points {
            Some(previous.screen_point(point, fraction)?)
        } else {
            None
        };
        let anchor = {
            let state = self.state.lock().unwrap();
            state
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
            if rows.is_empty() {
                return Err(error(
                    "stale_scene",
                    "No previously observed app pixels are visible; inspect the returned view",
                ));
            }
            if original_receivers.iter().any(|before| {
                rows.iter().any(|row| {
                    row["window_ref"] == before.window
                        && bounds(row).is_some_and(|rect| rect != before.bounds)
                })
            }) {
                return Err(error(
                    "stale_scene",
                    "Geometry changed at the requested point; inspect the returned view before new input",
                ));
            }
            let mut fresh = match self.scene_capture(id, &rows, single_point, false, deadline) {
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
            let stable_canvas = anchor.as_ref().is_some_and(|(window, rect)| {
                rows.iter()
                    .any(|row| row["window_ref"] == *window && bounds(row) == Some(*rect))
                    && rows.iter().all(|row| {
                        bounds(row).is_some_and(|rect| contains_bounds(previous.bounds, rect))
                    })
            });
            // Native refresh uses the full scene. Input coordinates keep the
            // crop and dimensions of the last published image.
            fresh.area = previous.area;
            fresh.width = previous.width;
            fresh.height = previous.height;
            for (point, before) in points.iter().zip(original_receivers) {
                let (current, _, _) = fresh.hit(point, fraction)?;
                if current.instance != before.instance {
                    return Err(error(
                        "stale_scene",
                        "The current pixel receiver belongs to another application instance",
                    ));
                }
                // Other parts of the app can change without moving this point.
                // Keep the existing same-instance overlay transitions on a
                // stable canvas. A changed canvas needs the same receiver and
                // its original geometry at each requested point.
                let receiver_changed = if current.window == before.window {
                    current.bounds != before.bounds
                } else {
                    !stable_canvas
                };
                if receiver_changed {
                    return Err(error(
                        "stale_scene",
                        "Geometry changed at the requested point; inspect the returned view before new input",
                    ));
                }
            }
            // A coordinate selects the current top app pixel, not a semantic object.
            // Keep the original canvas and per-point instance authorization baseline.
            // Fresh native observations authorize only this dispatch.
            let mut state = self.state.lock().unwrap();
            state.scene.raster = Some(previous);
            state.scene.pixels = true;
            return points
                .iter()
                .map(|point| fresh.point(point, fraction))
                .collect();
        }
    }
    fn scene_bind_foreground_points(
        &self,
        raster: &Raster,
        points: &[Value],
        fraction: bool,
        deadline: Instant,
    ) -> Result<Vec<Value>, wire::WireError> {
        let mut mapped = points
            .iter()
            .map(|point| raster.point(point, fraction))
            .collect::<Result<Vec<_>, _>>()?;
        let anchor = &raster.layers[0];
        let target = self
            .state
            .lock()
            .unwrap()
            .windows
            .get(&anchor.window)
            .cloned()
            .ok_or_else(|| error("target_changed", "Captured window is no longer available"))?;
        let bounds = anchor.bounds.map(|coordinate| coordinate as i32);
        self.state.lock().unwrap().observations.retain(|snapshot| {
            snapshot.window_ref != anchor.window || snapshot.kind != ObservationKind::Pixels
        });
        let observation = self.runtime.bind_foreground_pixels_until(&target, bounds, deadline)
            .map_err(|reason| match reason {
                Error::StaleObservation => error(
                    "stale_scene",
                    "Geometry changed at the requested point; inspect the returned view before new input",
                ),
                _ => wire::core_error(&reason),
            })?;
        let work = self.observation_with_size(
            &anchor.window,
            observation,
            None,
            Some((bounds[2] as u32, bounds[3] as u32)),
        );
        if let Some(error) = work.completed.error {
            return Err(error);
        }
        let reference = &work.completed.value.unwrap()["observation_ref"];
        apply_publication(&mut self.state.lock().unwrap(), work.publication);
        for point in &mut mapped {
            point["observation_ref"] = reference.clone();
        }
        Ok(mapped)
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
        let (wait_for_window, previous_raster, previous_nodes) = {
            let mut state = self.state.lock().unwrap();
            let nodes = std::mem::take(&mut state.scene.nodes);
            let raster = state.scene.raster.take();
            state.scene.observed = None;
            state.scene.projected = None;
            (
                std::mem::take(&mut state.scene.window_wait_pending),
                raster,
                nodes,
            )
        };
        let mut startup = None;
        if self.runtime.instances().is_empty() && after {
            let current = json!({"running":false,"windows":[],"elements":[],"semantics":{"complete":true,"reason":"app_stopped"},"coverage":{"complete_within_scope":true}});
            let mut state = self.state.lock().unwrap();
            let pending_new_window = state.scene.pending_new_window.take();
            state.scene = SceneState {
                pending_new_window,
                ..SceneState::default()
            };
            state.scene.observed = Some(current.clone());
            let mut view = current.clone();
            view["kind"] = json!("full");
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
        let pending = if args["_wait_new_window"] == true {
            self.state.lock().unwrap().scene.pending_new_window.clone()
        } else {
            None
        };
        let until = (Instant::now()
            + if pending.is_none() && (startup.is_some() || after || wait_for_window) {
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
        let wait_instance = pending.as_ref().map(|before| &before.instance);
        if let Some(instance) = wait_instance {
            let state = self.state.lock().unwrap();
            rows.retain(|row| {
                row["window_ref"]
                    .as_str()
                    .and_then(|reference| state.windows.get(reference))
                    .is_some_and(|target| &target.app == instance)
            });
        }
        rows = ordered(rows);
        let wait_view = args["_wait_view"] == true;
        let wait_window = if let Some(title) = args["_wait_title"].as_str() {
            let mut matches = rows.iter().filter(|row| {
                row["visible"] == true
                    && row["scene"]["semantic_candidate"] != false
                    && row["title"]
                        .as_str()
                        .is_some_and(|name| name.to_lowercase().contains(title))
            });
            let Some(row) = matches.next() else {
                // The destination does not exist yet. Avoid reading the old
                // window's tree on every poll.
                return data(json!({"windows":[]}));
            };
            if matches.next().is_some() {
                return failed(error(
                    "ambiguous_window",
                    "Several visible windows match the wait title; provide a more specific title",
                ));
            }
            row["window_ref"].as_str().map(str::to_owned)
        } else {
            None
        };
        let visible: Vec<_> = rows
            .iter()
            .filter(|r| r["visible"] == true)
            .cloned()
            .collect();
        // A native decoration can be a real composited surface without being
        // the active semantic window. Do not substitute keyboard focus here:
        // menus/dropdowns may be interactive above their focused owner.
        let mut semantic_windows: Vec<_> = visible
            .iter()
            .filter(|row| row["scene"]["keyboard_modal"] == true)
            .chain(
                visible
                    .iter()
                    .filter(|row| row["scene"]["keyboard_modal"] != true),
            )
            .chain(rows.iter().filter(|row| row["visible"] != true))
            .filter(|row| row["scene"]["semantic_candidate"] != false)
            .filter(|row| {
                (!wait_view || row["visible"] == true)
                    && wait_window
                        .as_deref()
                        .is_none_or(|window| row["window_ref"] == window)
            })
            .cloned()
            .collect();
        if let Some(before) = &pending {
            let state = self.state.lock().unwrap();
            semantic_windows.retain(|row| row["visible"] == true);
            semantic_windows.sort_by_key(|row| {
                !row["window_ref"]
                    .as_str()
                    .and_then(|reference| state.windows.get(reference))
                    .is_some_and(|target| before.matches(target))
            });
        }
        semantic_windows.truncate(32);
        let state = self.state.lock().unwrap();
        let visual_followup = after && state.scene.pixels;
        let capture_options = if explicit_pixels {
            CaptureOptions {
                region: capture_region(&args["region"]),
                max_dimension: args["max_dimension"]
                    .as_u64()
                    .map_or(MAX_IMAGE_DIMENSION, |value| value as u32),
                semantics: args["semantics"] == true,
            }
        } else if visual_followup {
            state.scene.capture_options
        } else {
            CaptureOptions::default()
        };
        drop(state);
        let mut active = None;
        let window_ids = {
            let mut state = self.state.lock().unwrap();
            if state.scene.window_ids.len() > 4096 {
                let pending_new_window = state.scene.pending_new_window.take();
                state.scene = SceneState {
                    pending_new_window,
                    ..SceneState::default()
                };
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
        let mut semantic = json!({"complete":false,"reason":if capture_options.semantics {
            "no_window"
        } else {
            "skipped"
        }});
        let mut observation = None;
        for row in semantic_windows
            .into_iter()
            .filter(|_| capture_options.semantics)
        {
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
                let window_ref = row["window_ref"].as_str().unwrap();
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
                        format!("{}\0{}\0{}", window_ref, credential, element["role"])
                    });
                    let key = element["identity"]
                        .as_str()
                        .filter(|identity| identities.get(*identity) == Some(&1))
                        .map(|identity| {
                            format!("{}\0{}\0{}", window_ref, identity, element["role"])
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
                        let target =
                            format!("{}.e{}", window_ids[window_ref], state.scene.next_element);
                        target
                    });
                    if let Some(key) = key.filter(|key| {
                        state.scene.element_ids.len() < MAX_DISPLAY_IDENTITIES
                            || state.scene.element_ids.contains_key(key)
                    }) {
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
                    if element["focused"] == true {
                        entry["focused"] = json!(true);
                    }
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
        if wait_view || pending.is_some() {
            let primary = active
                .as_ref()
                .and_then(|row| row["window_ref"].as_str())
                .or(wait_window.as_deref());
            if let Some(position) =
                primary.and_then(|window| rows.iter().position(|row| row["window_ref"] == window))
            {
                rows[..=position].rotate_right(1);
            }
        }
        // The Windows capture path can prepare foreground focus on occlusion.
        // New-window conditions use inventory and semantics only.
        let pixel_mode = pending.is_none()
            && (explicit_pixels
                || visual_followup
                || (subtree.is_none() && !visible.is_empty() && semantic_empty));
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
            let capture = self
                .scene_capture(id, &visible, None, explicit_pixels, deadline)
                .and_then(|mut raster| {
                    let (previous_options, canvas_changed) = {
                        let state = self.state.lock().unwrap();
                        (
                            state.scene.capture_options,
                            state
                                .scene
                                .viewport
                                .as_ref()
                                .is_some_and(|v| v.0 != raster.bounds),
                        )
                    };
                    let mut options = capture_options;
                    if !explicit_pixels && canvas_changed {
                        options.region = None;
                    }
                    raster.set_image(options.region, options.max_dimension);
                    let native_size = &raster.layers[0].pixels;
                    let png = raster.foreground_png.take().filter(|_| {
                        raster.area == raster.bounds
                            && (raster.width, raster.height)
                                == (native_size.width, native_size.height)
                    });
                    let png = match png {
                        Some(png) => png,
                        None => Arc::new(raster.encode()?),
                    };
                    let changed = canvas_changed
                        || previous_options.region != options.region
                        || previous_options.max_dimension != options.max_dimension;
                    Ok((raster, png, options, changed))
                });
            match capture {
                Ok((raster, png, options, changed)) => {
                    let mut info = json!({"width":raster.width,"height":raster.height,
                        "bounds":raster.area,"coordinate_space":"image_pixels"});
                    if let Some([x, y, width, height]) = options.region {
                        info["region"] = json!({"x":x,"y":y,"width":width,"height":height});
                        info["canvas_bounds"] = json!(raster.bounds);
                    }
                    if changed {
                        info["coordinate_reset"] = json!(true);
                    }
                    image_info = Some(info);
                    let mut state = self.state.lock().unwrap();
                    if self.runtime.uses_foreground_capture() {
                        let anchor = &raster.layers[0];
                        state.scene.viewport =
                            Some((raster.bounds, anchor.window.clone(), anchor.bounds));
                    } else if let Some(anchor) = visible.last() {
                        if let Some(rect) = bounds(anchor) {
                            state.scene.viewport = Some((
                                raster.bounds,
                                anchor["window_ref"].as_str().unwrap().into(),
                                rect,
                            ));
                        }
                    }
                    image = Some(png);
                    state.scene.raster = Some(raster);
                    state.scene.pixels = true;
                    state.scene.capture_options = options;
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
        } else if pending.is_none() {
            let mut state = self.state.lock().unwrap();
            if !visible.is_empty() {
                state.scene.raster = previous_raster;
            } else if explicit_pixels {
                image_info = Some(json!({"error":{"code":"no_visible_window",
                    "message":"The app has no visible window to capture"}}));
            }
            state.scene.pixels = false;
        }
        let mut state = self.state.lock().unwrap();
        let reset = state.scene.baseline.is_none()
            || args["full"] == true
            || state.scene.baseline.as_ref().is_some_and(|previous| {
                previous["semantics"]["query"] != current["semantics"]["query"]
                    || (previous["semantics"]["reason"] == "skipped")
                        != (current["semantics"]["reason"] == "skipped")
            });
        let mut view = if reset {
            let mut v = current.clone();
            v["kind"] = json!("full");
            v
        } else {
            delta(state.scene.baseline.as_ref().unwrap(), &current)
        };
        if view["kind"] == "delta" {
            let mut full = current.clone();
            full["kind"] = json!("full");
            if full.to_string().len() < view.to_string().len() {
                view = full;
            }
        }
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
        if capture_options.semantics {
            state.scene.active = active.and_then(|r| r["window_ref"].as_str().map(str::to_owned));
        }
        // Keep display IDs across filters, partial reads and observations of
        // other windows. A complete unfiltered read can prove which IDs left
        // this window. The cache never supplies a native action binding.
        if semantic["complete"] == true && semantic.get("query").is_none() {
            if let Some(window) = state.scene.active.as_deref() {
                let prefix = format!("{window}\0");
                let present = nodes
                    .iter()
                    .filter_map(|row| row["id"].as_str())
                    .collect::<std::collections::HashSet<_>>();
                state.scene.element_ids.retain(|key, cached| {
                    !key.starts_with(&prefix) || present.contains(cached.id.as_str())
                });
            }
        }
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
        state.scene.nodes = if explicit_pixels && !capture_options.semantics && image.is_some() {
            // A pixel-only read does not replace a previous semantic sample.
            // Its targets still require normal native revalidation at input.
            previous_nodes
        } else {
            nodes
        };
        drop(state);
        let mut work = data(snapshot);
        work.completed.image = image;
        work
    }
    fn scene_capture(
        &self,
        id: &str,
        rows: &[Value],
        point: Option<[f64; 2]>,
        prepare_focus: bool,
        deadline: Instant,
    ) -> Result<Raster, Value> {
        if self.runtime.uses_foreground_capture() {
            return self.scene_capture_foreground(id, rows, prepare_focus, deadline);
        }
        // For a single-point preflight only surfaces that could receive that
        // point need pixels. Apply the same filter to the post-capture inventory
        // so a surface that moves over the point invalidates this read.
        let relevant = |row: &&Value| covers_point(row, point);
        let capture_rows = ordered(rows.iter().filter(relevant).cloned().collect());
        if capture_rows.is_empty() {
            return Err(
                json!({"code":"outside_app","message":"The point contains no authorized app surface"}),
            );
        }
        if capture_rows.len() > 16 {
            return Err(
                json!({"code":"scene_limit","message":"More than 16 visible surfaces; no partial image binding created"}),
            );
        }
        let mut layers = Vec::new();
        let mut decoded_bytes = 0usize;
        for row in &capture_rows {
            let rect = bounds(row);
            if capture_rows.len() > 1 && rect.is_none() {
                return Err(
                    json!({"code":"scene_geometry_unavailable","message":"Provider cannot compose multiple surfaces"}),
                );
            }
            let window = row["window_ref"].as_str().unwrap();
            let work = self.scene_atomic(id, "capture", json!({"window_ref":window}), deadline);
            let pixels = captured_layer(work)?;
            decoded_bytes += pixels.rgba.len();
            if decoded_bytes > 64 * 1024 * 1024 {
                return Err(
                    json!({"code":"scene_limit","message":"Scene exceeds 64 MiB decoded budget"}),
                );
            }
            let rect =
                rect.unwrap_or([0.0, 0.0, f64::from(pixels.width), f64::from(pixels.height)]);
            let instance = self.state.lock().unwrap().windows
                .get(window)
                .map(|target| target.app.clone())
                .ok_or(json!({"code":"invalid_image_binding","message":"Captured surface ownership is unavailable"}))?;
            layers.push(Layer {
                instance,
                window: window.into(),
                bounds: rect,
                pixels,
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
                .filter(|r| r["visible"] == true && covers_point(r, point))
                .cloned()
                .collect(),
        );
        for row in &capture_rows {
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
        if after_rows.len() != capture_rows.len() {
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
        Raster::new(layers, viewport).map_err(image_encoding_error)
    }
    fn scene_capture_foreground(
        &self,
        id: &str,
        rows: &[Value],
        prepare_focus: bool,
        deadline: Instant,
    ) -> Result<Raster, Value> {
        let window = rows
            .iter()
            .find(|row| row["scene"]["keyboard_focused"] == true)
            .and_then(|row| row["window_ref"].as_str().map(str::to_owned))
            .or_else(|| self.scene_receiver(rows, None, true))
            .or_else(|| rows.first()?.get("window_ref")?.as_str().map(str::to_owned))
            .ok_or_else(|| json!({"code":"no_visible_window"}))?;
        let target = self
            .state
            .lock()
            .unwrap()
            .windows
            .get(&window)
            .cloned()
            .ok_or_else(|| json!({"code":"invalid_image_binding"}))?;
        let work = if prepare_focus {
            self.scene_atomic(id, "capture", json!({"window_ref":window}), deadline)
        } else {
            let mut work = self.capture_with_preparation(None, &target, &window, deadline);
            let publication = std::mem::replace(&mut work.publication, Publication::None);
            apply_publication(&mut self.state.lock().unwrap(), publication);
            work
        };
        let png = work.completed.image.clone();
        let origin = work.completed.value.as_ref().and_then(|value| {
            serde_json::from_value::<[i32; 2]>(value["image"]["screen_origin"].clone()).ok()
        });
        let pixels = captured_layer(work)?;
        let [left, top] = origin.ok_or_else(|| json!({"code":"invalid_image_binding"}))?;
        let mut raster = Raster::new(
            vec![Layer {
                instance: target.app,
                window,
                bounds: [
                    f64::from(left),
                    f64::from(top),
                    f64::from(pixels.width),
                    f64::from(pixels.height),
                ],
                pixels,
            }],
            None,
        )
        .map_err(image_encoding_error)?;
        raster.foreground_png = png;
        Ok(raster)
    }
}

fn captured_layer(work: Work) -> Result<LayerPixels, Value> {
    if let Some(error) = work.completed.error {
        return Err(error_json(&error));
    }
    let value = work
        .completed
        .value
        .ok_or_else(|| json!({"code":"invalid_image"}))?;
    let png = work
        .completed
        .image
        .ok_or_else(|| json!({"code":"invalid_image"}))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png.as_str())
        .map_err(|_| json!({"code":"invalid_image"}))?;
    let (width, height, rgba) =
        decode(&bytes).map_err(|message| json!({"code":"invalid_image","message":message}))?;
    if value["image"]["width"] != width || value["image"]["height"] != height {
        return Err(json!({"code":"invalid_image_binding"}));
    }
    let observation = value["observation_ref"]
        .as_str()
        .ok_or_else(|| json!({"code":"invalid_image_binding"}))?
        .to_owned();
    Ok(LayerPixels {
        observation,
        width,
        height,
        rgba,
    })
}

fn image_encoding_error(message: String) -> Value {
    json!({"code":"image_encoding","message":message})
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
    fn point(&self, args: &Value, fraction: bool) -> Result<Value, wire::WireError> {
        let (layer, pixels, point) = self.hit(args, fraction)?;
        Ok(
            json!({"window_ref":layer.window,"observation_ref":pixels.observation,
            "x":point.x,"y":point.y}),
        )
    }

    fn hit(
        &self,
        args: &Value,
        fraction: bool,
    ) -> Result<(&Layer, &LayerPixels, PixelPoint), wire::WireError> {
        let [sx, sy] = self.screen_point(args, fraction)?;
        for layer in &self.layers {
            if !contains_point(layer.bounds, [sx, sy]) {
                continue;
            }
            let pixels = &layer.pixels;
            let px = (sx - layer.bounds[0]) / layer.bounds[2] * f64::from(pixels.width);
            let py = (sy - layer.bounds[1]) / layer.bounds[3] * f64::from(pixels.height);
            if px < f64::from(pixels.width) && py < f64::from(pixels.height) {
                let index = (py as usize * pixels.width as usize + px as usize) * 4;
                if pixels.rgba[index + 3] > 0 {
                    return Ok((layer, pixels, PixelPoint { x: px, y: py }));
                }
            }
        }
        Err(error(
            "outside_app",
            "The point contains no authorized app surface",
        ))
    }

    fn screen_point(&self, args: &Value, fraction: bool) -> Result<[f64; 2], wire::WireError> {
        let frame = self;
        let (Some(x), Some(y)) = (args["x"].as_f64(), args["y"].as_f64()) else {
            return Err(error(
                "invalid_arguments",
                "Image coordinates require finite x and y",
            ));
        };
        let (x, y) = if fraction {
            (x * f64::from(frame.width), y * f64::from(frame.height))
        } else {
            (x, y)
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
        Ok([
            frame.area[0] + x / f64::from(frame.width) * frame.area[2],
            frame.area[1] + y / f64::from(frame.height) * frame.area[3],
        ])
    }
}
fn delta(previous: &Value, current: &Value) -> Value {
    let mut out = json!({"kind":if previous == current {"unchanged"} else {"delta"}});
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
#[cfg(test)]
fn compose_in(layers: Vec<Layer>, viewport: Option<[f64; 4]>) -> Result<(Raster, String), String> {
    let raster = Raster::new(layers, viewport)?;
    let png = raster.encode().map_err(|e| e.to_string())?;
    Ok((raster, png))
}

impl Raster {
    fn new(layers: Vec<Layer>, viewport: Option<[f64; 4]>) -> Result<Self, String> {
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
        let mut raster = Self {
            bounds,
            area: bounds,
            width: 0,
            height: 0,
            layers,
            foreground_png: None,
        };
        raster.set_image(None, MAX_IMAGE_DIMENSION);
        Ok(raster)
    }

    fn set_image(&mut self, region: Option<[f64; 4]>, max_dimension: u32) {
        let [x, y, width, height] = region.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let [left, top, w, h] = self.bounds;
        self.area = [left + x * w, top + y * h, width * w, height * h];
        let max_scale = if region.is_none() && self.foreground_png.is_some() {
            1.0
        } else {
            2.0
        };
        let scale = (f64::from(max_dimension) / self.area[2].max(self.area[3])).min(max_scale);
        self.width = (self.area[2] * scale).round().max(1.0) as u32;
        self.height = (self.area[3] * scale).round().max(1.0) as u32;
    }

    fn encode(&self) -> Result<String, Value> {
        let bounds = self.area;
        let (width, height) = (self.width, self.height);
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        for layer in self
            .layers
            .iter()
            .rev()
            .filter(|layer| intersects_bounds(layer.bounds, bounds))
        {
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
                    let dest = &mut rgba[(py as usize * width as usize + px as usize) * 4..][..4];
                    let pixels = &layer.pixels;
                    let lx = ((sx - layer.bounds[0]) / layer.bounds[2] * f64::from(pixels.width))
                        as usize;
                    let ly = ((sy - layer.bounds[1]) / layer.bounds[3] * f64::from(pixels.height))
                        as usize;
                    let source = &pixels.rgba[(ly * pixels.width as usize + lx) * 4..][..4];
                    if source[3] == 0 {
                        continue;
                    }
                    if source[3] == 255 || dest[3] == 0 {
                        dest.copy_from_slice(source);
                        continue;
                    }
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
            let mut writer = encoder
                .write_header()
                .map_err(|e| image_encoding_error(e.to_string()))?;
            writer
                .write_image_data(&rgba)
                .map_err(|e| image_encoding_error(e.to_string()))?;
        }
        Ok(base64::engine::general_purpose::STANDARD.encode(png))
    }
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
            bounds: rect,
            pixels: LayerPixels {
                observation: format!("obs_{window}"),
                width: 2,
                height: 2,
                rgba: color.repeat(4),
            },
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
        let point = raster.point(&json!({"x":6,"y":6}), false).unwrap();
        assert_eq!(point["window_ref"], "top");
        assert_eq!(point["x"], 1.0);
        assert_eq!(
            raster.point(&json!({"x":1,"y":6}), false).unwrap_err().code,
            "outside_app"
        );
        assert_eq!(
            raster.point(&json!({"x":8,"y":0}), false).unwrap_err().code,
            "invalid_arguments"
        );
    }
    #[test]
    fn crop_png_and_input_share_the_same_origin_and_scale() {
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255; 4],
        ];
        let mut base = layer("base", [-10.0, -20.0, 10.0, 20.0], [0; 4]);
        base.pixels.rgba = colors.concat();
        let mut raster = Raster::new(vec![base], None).unwrap();
        raster.set_image(Some([0.5, 0.0, 0.5, 0.5]), MAX_IMAGE_DIMENSION);
        let png = base64::engine::general_purpose::STANDARD
            .decode(raster.encode().unwrap())
            .unwrap();
        let (width, height, rgba) = decode(&png).unwrap();
        assert_eq!((width, height), (10, 20));
        assert_eq!(rgba, colors[1].repeat((width * height) as usize));
        let pixels = raster.point(&json!({"x":5,"y":10}), false).unwrap();
        assert_eq!(
            pixels,
            raster.point(&json!({"x":0.5,"y":0.5}), true).unwrap()
        );
        assert_eq!(pixels["x"], 1.5);
        assert_eq!(pixels["y"], 0.5);
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
            before.point(&json!({"x":2,"y":8}), false).unwrap(),
            after.point(&json!({"x":2,"y":8}), false).unwrap()
        );
        assert_eq!(
            after.point(&json!({"x":10,"y":1}), false).unwrap_err().code,
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
            base.pixels.rgba = colors.concat();
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
                        let mapped = after.point(&point, false).unwrap();
                        assert_eq!(mapped, before.point(&point, false).unwrap());
                        assert_eq!(mapped["window_ref"], "base");
                        assert_eq!(mapped["x"], sx);
                        assert_eq!(mapped["y"], sy);
                    } else {
                        assert_eq!(
                            pixel,
                            &[0, 0, 0, 0],
                            "vacated popup={popup:?}, pixel=({x},{y})"
                        );
                        assert_eq!(after.point(&point, false).unwrap_err().code, "outside_app");
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
