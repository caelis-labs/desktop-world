//! Adapted Win32 identity, UIA patterns and SendInput primitives from pinned
//! CUA 0.38.0. DTW owns window incarnation markers, lazy pages and receipts.
use super::input;
use crate::backends::input_sequence::{self, Step};
use crate::{backends::native_client::*, computer::*};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use windows::{
    core::{w, Interface, BSTR, HRESULT, PWSTR},
    Win32::{
        Foundation::{CloseHandle, BOOL, E_NOINTERFACE, FILETIME, HANDLE, HWND, POINT, RECT},
        Graphics::{
            Dwm::{DwmFlush, DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::*,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED,
            },
            Ole::*,
            Threading::{
                GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            Variant::VT_I4,
        },
        UI::{
            Accessibility::*,
            HiDpi::{
                GetDpiForWindow, PhysicalToLogicalPointForPerMonitorDPI,
                SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
};

const MARKER: windows::core::PCWSTR = w!("DesktopWorld.WindowIncarnation.v1");
// PrintWindow is a user32 API; the windows 0.58 binding groups it under the
// unrelated XPS feature. Keep this exact Win32 ABI inside the Windows backend.
#[link(name = "user32")]
extern "system" {
    #[link_name = "PrintWindow"]
    fn render_window(window: HWND, dc: HDC, flags: u32) -> BOOL;
}
pub fn process_stamp(pid: u32) -> Result<AppInstance, Error> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(failed)?;
        let result = (|| {
            let (mut birth, mut exit, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            GetProcessTimes(handle, &mut birth, &mut exit, &mut kernel, &mut user)
                .map_err(failed)?;
            let mut path = vec![0u16; 32768];
            let mut len = path.len() as u32;
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(path.as_mut_ptr()),
                &mut len,
            )
            .map_err(failed)?;
            let identity = String::from_utf16_lossy(&path[..len as usize]).to_lowercase();
            Ok(AppInstance {
                identity: AppIdentity(identity),
                pid,
                generation: ((birth.dwHighDateTime as u64) << 32) | birth.dwLowDateTime as u64,
            })
        })();
        let _ = CloseHandle(handle);
        result
    }
}
fn hwnd(id: u64) -> HWND {
    HWND(id as usize as *mut _)
}
fn id(window: HWND) -> u64 {
    window.0 as usize as u64
}
pub(super) fn rect(window: HWND) -> Result<RECT, Error> {
    let mut result = RECT::default();
    unsafe { GetWindowRect(window, &mut result) }.map_err(failed)?;
    if unsafe { IsZoomed(window) }.as_bool() {
        // Like pinned CUA's win32/windows.rs and capture.rs frame geometry,
        // use DWM's physical visible frame. Maximized GetWindowRect includes
        // invisible resize borders outside the monitor. Do not clip a genuinely
        // off-screen window, and keep every retained pixel/region check aligned.
        let mut frame = RECT::default();
        let read = unsafe {
            DwmGetWindowAttribute(
                window,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut frame as *mut _ as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
        };
        result = maximized_frame(result, read.ok().map(|()| frame));
    }
    Ok(result)
}
fn maximized_frame(window: RECT, frame: Option<RECT>) -> RECT {
    frame
        .filter(|frame| {
            frame.left < frame.right
                && frame.top < frame.bottom
                && frame.left >= window.left
                && frame.top >= window.top
                && frame.right <= window.right
                && frame.bottom <= window.bottom
        })
        .unwrap_or(window)
}
fn equal_rect(a: RECT, b: RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}
fn intersects(a: RECT, b: RECT) -> bool {
    a.left < a.right
        && a.top < a.bottom
        && b.left < b.right
        && b.top < b.bottom
        && a.left < b.right
        && a.right > b.left
        && a.top < b.bottom
        && a.bottom > b.top
}
fn discovery_matches<E>(
    row: &Value,
    role: Option<&str>,
    text: Option<&str>,
    region: Option<RECT>,
    read_bounds: impl FnOnce() -> Result<RECT, E>,
) -> Result<bool, E> {
    if role.is_some_and(|role| row["role"].as_str() != Some(role))
        || text.is_some_and(|text| {
            !super::super::text_page::matches_fields(
                row["label"].as_str().unwrap_or(""),
                row["name"].as_str().unwrap_or(""),
                row["value"].as_str().unwrap_or(""),
                text,
            )
        })
    {
        return Ok(false);
    }
    // Geometry is required only for a matching candidate. Irrelevant nodes
    // with unavailable rectangles must not make a filtered query partial.
    match region {
        Some(region) => Ok(intersects(region, read_bounds()?)),
        None => Ok(true),
    }
}
pub(super) fn visible(window: HWND) -> bool {
    unsafe {
        let mut cloaked = 0u32;
        IsWindowVisible(window).as_bool()
            && !IsIconic(window).as_bool()
            && DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4)
                .is_ok()
            && cloaked == 0
    }
}
fn all_windows() -> Result<Vec<HWND>, Error> {
    super::desktop::windows()
}
fn title(window: HWND) -> String {
    let mut text = [0u16; 1024];
    let len = unsafe { GetWindowTextW(window, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..len])
}
pub(super) fn marker(window: HWND, create: bool) -> Result<u64, Error> {
    unsafe {
        let mut value = GetPropW(window, MARKER).0 as usize as u64;
        if value == 0 && create {
            value = nonce();
            SetPropW(window, MARKER, HANDLE(value as usize as *mut _)).map_err(failed)?;
        }
        if value == 0 {
            return Err(Error::TargetChanged);
        }
        Ok(value)
    }
}
struct Query {
    target: WindowTarget,
    root: IUIAutomationElement,
    scope_root: IUIAutomationElement,
    frontier: Vec<(IUIAutomationElement, u32)>,
    visited: HashSet<Vec<i32>>,
    scanned: usize,
    elements: HashMap<u64, IUIAutomationElement>,
    pending: Option<Value>,
    role: Option<String>,
    text: Option<String>,
    limit: usize,
    expires: Instant,
    gap: Option<String>,
    region: Option<RECT>,
    capture_bounds: Option<(RECT, u32)>,
    render_size: Option<(i32, i32)>,
    texts: HashMap<u64, String>,
}
// Region membership is scoped to the source capture, not just the HWND. Read
// element bounds first: a blocking UIA getter must not hide a concurrent window
// move or DPI change from the final capture-geometry check.
fn validate_region_geometry(
    capture_bounds: Option<(RECT, u32)>,
    region: Option<RECT>,
    read_element: impl FnOnce() -> Result<RECT, Error>,
    read_window: impl FnOnce() -> Result<(RECT, u32), Error>,
) -> Result<(), Error> {
    if let Some(region) = region {
        if !intersects(region, read_element()?) {
            return Err(Error::StaleObservation);
        }
    }
    if let Some((bounds, dpi)) = capture_bounds {
        let (current, current_dpi) = read_window()?;
        if !equal_rect(bounds, current) || dpi != current_dpi {
            return Err(Error::TargetChanged);
        }
    }
    Ok(())
}
impl Query {
    fn validate_region(
        &self,
        window: HWND,
        element: &IUIAutomationElement,
        until: Instant,
    ) -> Result<(), Error> {
        validate_region_geometry(
            self.capture_bounds,
            self.region,
            || unsafe { element.CurrentBoundingRectangle() }.map_err(failed),
            || Ok((rect(window)?, unsafe { GetDpiForWindow(window) })),
        )?;
        if self
            .render_size
            .is_some_and(|saved| render_size(window).map_or(true, |now| now != saved))
        {
            return Err(Error::TargetChanged);
        }
        check(until)
    }
}
struct Pixels {
    target: WindowTarget,
    bounds: RECT,
    dpi: u32,
    render_size: (i32, i32),
    expires: Instant,
}
type PointerPoint = (RECT, POINT, u32, Option<Instant>, Option<(i32, i32)>);
pub struct Store {
    uia: Option<IUIAutomation>,
    windows: HashMap<u64, WindowTarget>,
    queries: HashMap<String, Query>,
    pixels: HashMap<String, Pixels>,
}
impl Drop for Store {
    fn drop(&mut self) {
        self.queries.clear();
        self.uia.take();
        unsafe {
            CoUninitialize();
        }
    }
}
impl Store {
    pub fn new() -> Result<Self, Error> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(failed)?;
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
                .map_err(failed)?;
        }
        Ok(Self {
            uia: None,
            windows: HashMap::new(),
            queries: HashMap::new(),
            pixels: HashMap::new(),
        })
    }
    fn uia(&mut self) -> Result<IUIAutomation, Error> {
        if self.uia.is_none() {
            self.uia = Some(
                unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                    .map_err(failed)?,
            );
        }
        Ok(self.uia.as_ref().unwrap().clone())
    }
    fn validate(&self, target: &WindowTarget) -> Result<HWND, Error> {
        let window = hwnd(target.native_id);
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
        }
        if self.windows.get(&target.native_id) != Some(target)
            || pid != target.app.pid
            || process_stamp(pid)? != target.app
            || marker(window, false)? != target.generation
            || unsafe { GetAncestor(window, GA_ROOT) } != window
        {
            return Err(Error::TargetChanged);
        }
        Ok(window)
    }
    fn consume(&mut self, target: &WindowTarget) {
        self.queries.retain(|_, q| q.target != *target);
        self.pixels.retain(|_, p| p.target != *target);
    }
    pub fn dispatch(&mut self, request: &Value) -> Value {
        let op = request["operation"].as_str().unwrap_or("");
        let action = matches!(
            op,
            "press" | "setValue" | "click" | "focus" | "typeText" | "keypress" | "scroll"
        );
        self.queries.retain(|_, q| q.expires > Instant::now());
        self.pixels.retain(|_, p| p.expires > Instant::now());
        if op == "release" {
            let token = request["binding"].as_str().unwrap_or("");
            let released =
                self.queries.remove(token).is_some() | self.pixels.remove(token).is_some();
            return json!({"released":released});
        }
        let result = (|| {
            let until = deadline(request)?;
            check(until)?;
            match op {
                "prepare_cursor" => {
                    let target = parse_target(&request["target"])?;
                    self.validate(&target)?;
                    let proof =
                        super::cursor::prepare(target).ok_or(Error::UnsupportedCapability)?;
                    Ok(json!({"cursor_target":proof}))
                }
                "catalog" => {
                    super::cursor::begin_proof_cycle();
                    let mut apps = HashMap::new();
                    for window in all_windows()? {
                        check(until)?;
                        if !unsafe { IsWindowVisible(window) }.as_bool()
                            || super::cursor::proven_overlay(window)
                        {
                            continue;
                        }
                        let mut pid = 0;
                        unsafe {
                            GetWindowThreadProcessId(window, Some(&mut pid));
                        }
                        if apps.contains_key(&pid) {
                            continue;
                        }
                        if let Ok(app) = process_stamp(pid) {
                            let name = app
                                .identity
                                .0
                                .rsplit('\\')
                                .next()
                                .unwrap_or(&app.identity.0);
                            apps.insert(
                                pid,
                                json!({"app_id":app.identity.0,"app":app_json(&app),"name":name}),
                            );
                        }
                        if apps.len() >= 512 {
                            return Err(failed("App inventory exceeds 512 instances"));
                        }
                    }
                    Ok(json!({"apps":apps.into_values().collect::<Vec<_>>()}))
                }
                "windows" => {
                    let app = parse_app(&request["app"])?;
                    if process_stamp(app.pid)? != app {
                        return Err(Error::TargetChanged);
                    }
                    let mut result = Vec::new();
                    for window in all_windows()? {
                        check(until)?;
                        let mut pid = 0;
                        unsafe {
                            GetWindowThreadProcessId(window, Some(&mut pid));
                        }
                        if pid != app.pid || !unsafe { IsWindowVisible(window) }.as_bool() {
                            continue;
                        }
                        let target = WindowTarget {
                            app: app.clone(),
                            native_id: id(window),
                            generation: marker(window, true)?,
                        };
                        self.windows.insert(id(window), target.clone());
                        result.push(json!({"target":target_json(&target),"title":title(window),"visible":visible(window)}));
                        if result.len() > 256 || self.windows.len() > 512 {
                            return Err(failed(
                                "Window inventory exceeds retained identity budget",
                            ));
                        }
                    }
                    self.windows
                        .retain(|_, target| unsafe { IsWindow(hwnd(target.native_id)) }.as_bool());
                    Ok(json!({"windows":result}))
                }
                "observe" | "observe_next" => self.observe(request, until, op == "observe_next"),
                "probe_window_state" => self.probe_window_state(request, until),
                "read_text" => self.read_text(request, until),
                "capture" => self.capture(&parse_target(&request["target"])?, until),
                _ if action => Ok(self.action(request, until)),
                _ => Err(Error::UnsupportedCapability),
            }
        })();
        result.unwrap_or_else(|error| windows_error_reply(error, action))
    }
    fn probe_window_state(&mut self, r: &Value, until: Instant) -> Result<Value, Error> {
        let target = parse_target(&r["target"])?;
        let desired = string(r, "state")?;
        if !["dialog", "unblocked", "closed"].contains(&desired) {
            return Err(Error::UnsupportedCapability);
        }
        let anchor = hwnd(target.native_id);
        if desired == "closed" && !unsafe { IsWindow(anchor) }.as_bool() {
            return Ok(json!({"status":"closed"}));
        }
        self.validate(&target)?;
        if desired == "closed" {
            return Ok(json!({"status":"pending","reason":"window_present"}));
        }
        if !visible(anchor) {
            return Ok(json!({"status":"pending","reason":"window_not_visible"}));
        }
        let enabled = unsafe { IsWindowEnabled(anchor) }.as_bool();
        if desired == "unblocked" {
            return if enabled {
                Ok(
                    json!({"status":"matched","kind":"window","relation":"same_window",
                "window":{"target":target_json(&target),"title":title(anchor),"visible":true}}),
                )
            } else {
                Ok(json!({"status":"pending","reason":"blocked_by_modal"}))
            };
        }
        if enabled {
            return Ok(json!({"status":"pending","reason":"dialog_not_present"}));
        }
        let mut candidates = Vec::new();
        let mut not_ready = false;
        for window in all_windows()? {
            check(until)?;
            if window == anchor
                || !visible(window)
                || !unsafe { IsWindowEnabled(window) }.as_bool()
                || !owned_modal_chain(window, anchor, target.app.pid, until)?
            {
                continue;
            }
            let uia = self.uia()?;
            let element = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
            let modal = unsafe {
                optional_pattern::<IUIAutomationWindowPattern>(&element, UIA_WindowPatternId)
            }?;
            let is_modal = match modal {
                Some(pattern) => unsafe { pattern.CurrentIsModal() }
                    .map_err(failed)?
                    .as_bool(),
                None => false,
            };
            if is_modal {
                candidates.push(window);
            } else {
                not_ready = true;
            }
        }
        self.validate(&target)?;
        if unsafe { IsWindowEnabled(anchor) }.as_bool() {
            return Ok(json!({"status":"pending","reason":"dialog_not_present"}));
        }
        if candidates.len() != 1 {
            return Ok(
                json!({"status":"pending","reason":if candidates.len()>1{"ambiguous_dialog"}else if not_ready{"dialog_not_ready"}else{"dialog_not_present"}}),
            );
        }
        let window = candidates[0];
        if !visible(window)
            || !owned_modal_chain(window, anchor, target.app.pid, until)?
            || !unsafe { IsWindowEnabled(window) }.as_bool()
        {
            return Ok(json!({"status":"pending","reason":"dialog_not_ready"}));
        }
        let found = WindowTarget {
            app: target.app.clone(),
            native_id: id(window),
            generation: marker(window, true)?,
        };
        if self.windows.len() >= 512 && !self.windows.contains_key(&found.native_id) {
            return Err(failed("Window identity budget exhausted"));
        }
        self.windows.insert(found.native_id, found.clone());
        self.validate(&found)?;
        Ok(
            json!({"status":"matched","kind":"dialog","relation":"child_modal","window":{"target":target_json(&found),"title":title(window),"visible":true}}),
        )
    }
    fn observe(&mut self, r: &Value, until: Instant, next: bool) -> Result<Value, Error> {
        let target = parse_target(&r["target"])?;
        let window = self.validate(&target)?;
        let uia = self.uia()?;
        let token = if next {
            string(r, "binding")?.to_owned()
        } else {
            if self.queries.len() >= 8 {
                if !self.queries.values().any(|q| q.target == target) {
                    return Err(failed("Eight active semantic query limit"));
                }
            }
            uuid::Uuid::new_v4().to_string()
        };
        let mut q = if next {
            self.queries.remove(&token).ok_or(Error::StaleObservation)?
        } else {
            let limit = r["query"]["limit"].as_u64().unwrap_or(40) as usize;
            let role = r["query"]["role"].as_str().map(str::to_owned);
            let text = r["query"]["text"].as_str().map(str::to_lowercase);
            if !(1..=100).contains(&limit) || text.as_ref().is_some_and(String::is_empty) {
                return Err(failed("Invalid semantic query"));
            }
            let root = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
            let scope = &r["query"]["scope"];
            let mut first = root.clone();
            let mut region = None;
            let mut capture_bounds = None;
            let mut source_render_size = None;
            match scope["kind"].as_str().unwrap_or("window") {
                "window" => (),
                "subtree" => {
                    let old = self
                        .queries
                        .get(string(scope, "binding")?)
                        .ok_or(Error::StaleObservation)?;
                    if old.target != target || old.expires <= Instant::now() {
                        return Err(Error::StaleObservation);
                    }
                    first = old
                        .elements
                        .get(&number(scope, "element")?)
                        .ok_or(Error::ElementNotFound)?
                        .clone();
                    owned(&uia, &first, &old.root, until).map_err(retained_error)?;
                    owned(&uia, &first, &root, until).map_err(retained_error)?;
                    old.validate_region(window, &first, until)?;
                }
                "region" => {
                    let pixels = self
                        .pixels
                        .get(string(scope, "binding")?)
                        .ok_or(Error::StaleObservation)?;
                    if pixels.target != target || pixels.expires <= Instant::now() {
                        return Err(Error::StaleObservation);
                    }
                    if !equal_rect(pixels.bounds, rect(window)?)
                        || pixels.dpi != unsafe { GetDpiForWindow(window) }
                        || pixels.render_size != render_size(window)?
                    {
                        return Err(Error::TargetChanged);
                    }
                    let values = scope["rect"]
                        .as_array()
                        .filter(|r| r.len() == 4)
                        .ok_or(Error::InvalidPoint)?;
                    let coords = values
                        .iter()
                        .map(|v| {
                            v.as_f64()
                                .filter(|x| x.is_finite())
                                .ok_or(Error::InvalidPoint)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let (x, y, w, h) = (coords[0], coords[1], coords[2], coords[3]);
                    if x < 0.
                        || y < 0.
                        || w <= 0.
                        || h <= 0.
                        || x + w > (pixels.bounds.right - pixels.bounds.left) as f64
                        || y + h > (pixels.bounds.bottom - pixels.bounds.top) as f64
                    {
                        return Err(Error::InvalidPoint);
                    }
                    region = Some(RECT {
                        left: pixels.bounds.left + x.floor() as i32,
                        top: pixels.bounds.top + y.floor() as i32,
                        right: pixels.bounds.left + (x + w).ceil() as i32,
                        bottom: pixels.bounds.top + (y + h).ceil() as i32,
                    });
                    capture_bounds = Some((pixels.bounds, pixels.dpi));
                    source_render_size = Some(pixels.render_size);
                }
                _ => return Err(Error::UnsupportedCapability),
            }
            self.queries.retain(|_, q| q.target != target);
            Query {
                target: target.clone(),
                frontier: vec![(first.clone(), 0)],
                scope_root: first,
                root,
                visited: HashSet::new(),
                scanned: 0,
                elements: HashMap::new(),
                pending: None,
                role,
                text,
                limit,
                expires: Instant::now() + Duration::from_secs(15),
                gap: None,
                region,
                capture_bounds,
                render_size: source_render_size,
                texts: HashMap::new(),
            }
        };
        if q.target != target || q.expires <= Instant::now() {
            return Err(Error::StaleObservation);
        }
        if q.capture_bounds.is_some_and(|(bounds, dpi)| {
            rect(window).map_or(true, |now| !equal_rect(bounds, now))
                || dpi != unsafe { GetDpiForWindow(window) }
        }) {
            return Err(Error::TargetChanged);
        }
        if q.render_size
            .is_some_and(|saved| render_size(window).map_or(true, |now| now != saved))
        {
            return Err(Error::TargetChanged);
        }
        owned(&uia, &q.scope_root, &q.root, until).map_err(retained_error)?;
        let current_root = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
        owned(&uia, &q.scope_root, &current_root, until).map_err(retained_error)?;
        let walker = unsafe { uia.RawViewWalker() }.map_err(failed)?;
        let metadata_cache = super::discovery_cache::Cache::new(&uia);
        let page_started = Instant::now();
        let page_until = until.min(Instant::now() + Duration::from_millis(750));
        let mut output = Vec::new();
        let mut bytes = 0;
        let mut steps = 0;
        let (mut cached_nodes, mut live_metadata_nodes) = (0, 0);
        if let Some(row) = q.pending.take() {
            bytes = row.to_string().len();
            output.push(row);
        }
        while output.len() < q.limit
            && !q.frontier.is_empty()
            && Instant::now() < page_until
            && steps < 256
        {
            check(until)?;
            if q.scanned >= 8192 || q.elements.len() >= 4096 {
                q.frontier.clear();
                q.gap = Some("scan_limit".into());
                break;
            }
            let (element, depth) = q.frontier.pop().unwrap();
            q.scanned += 1;
            steps += 1;
            if !(super::discovery::Branches {
                frontier: &mut q.frontier,
                visited: &mut q.visited,
                gap: &mut q.gap,
            })
            .admit(
                depth,
                || optional(unsafe { walker.GetNextSiblingElement(&element) }),
                || optional(unsafe { walker.GetFirstChildElement(&element) }),
                || owned_with_walker(&uia, &walker, &element, &q.scope_root, until),
                || runtime_id(&element),
            )? {
                continue;
            }
            if let Some(expected) = q.role.as_deref() {
                match unsafe { element.CurrentControlType() } {
                    Ok(control) if role(control) != expected => continue,
                    Ok(_) => (),
                    Err(_) => {
                        q.gap.get_or_insert("native_read_gap".into());
                        continue;
                    }
                }
            }
            // Every branch was already queued: an irrelevant role must not
            // prune descendants. It does avoid names/pattern probes which are
            // unnecessary to prove that this exact node cannot match the role.
            let cached = metadata_cache.read(&element);
            if cached.is_some() {
                cached_nodes += 1;
            } else {
                live_metadata_nodes += 1;
            }
            let mut row = match element_row(&element, (q.elements.len() + 1) as u64, cached) {
                Ok(row) => row,
                Err(_) => {
                    q.gap.get_or_insert("native_read_gap".into());
                    continue;
                }
            };
            match discovery_matches(
                &row,
                q.role.as_deref(),
                q.text.as_deref(),
                q.region,
                || unsafe { element.CurrentBoundingRectangle() },
            ) {
                Ok(true) => (),
                Ok(false) => continue,
                Err(_) => {
                    q.gap.get_or_insert("native_read_gap".into());
                    continue;
                }
            }
            // Match complete native fields first. Only the published discovery
            // preview is shortened; read_text has a separate full-text contract.
            for field in ["name", "value"] {
                if let Some(value) = row[field].as_str() {
                    row[field] = json!(value.chars().take(512).collect::<String>());
                }
            }
            row["label"] = json!(format!(
                "{} {}",
                row["role"].as_str().unwrap_or(""),
                row["name"].as_str().unwrap_or("")
            ));
            q.elements.insert((q.elements.len() + 1) as u64, element);
            let size = row.to_string().len();
            if bytes + size > 12 * 1024 {
                q.pending = Some(row);
                break;
            }
            bytes += size;
            output.push(row);
        }
        self.validate(&target)?;
        // HWND identity alone cannot prove that a retained semantic subtree
        // still belongs to this window after UIA calls yielded to the app.
        let current_root = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
        owned(&uia, &q.scope_root, &q.root, until).map_err(retained_error)?;
        owned(&uia, &q.scope_root, &current_root, until).map_err(retained_error)?;
        if q.capture_bounds.is_some_and(|(bounds, dpi)| {
            rect(window).map_or(true, |now| !equal_rect(bounds, now))
                || dpi != unsafe { GetDpiForWindow(window) }
        }) {
            return Err(Error::TargetChanged);
        }
        if q.render_size
            .is_some_and(|saved| render_size(window).map_or(true, |now| now != saved))
        {
            return Err(Error::TargetChanged);
        }
        let more = !q.frontier.is_empty() || q.pending.is_some();
        let reason = q.gap.clone().or_else(|| {
            more.then(|| {
                // A zero-match page is not proof that the provider has no such
                // controls or has not loaded yet. Report the actual page bound.
                if q.pending.is_some() {
                    "page_byte_limit"
                } else if output.len() >= q.limit {
                    "page_element_limit"
                } else if steps >= 256 {
                    "page_node_limit"
                } else {
                    "page_time_limit"
                }
                .into()
            })
        });
        if std::env::var("DTW_AX_READINESS_TRACE").ok().as_deref() == Some("1") {
            eprintln!(
                "DTW Windows discovery: {}",
                json!({"pid":std::process::id(),"page_scanned":steps,
                    "visited":q.visited.len(),"matched":output.len(),
                    "cached_nodes":cached_nodes,"live_metadata_nodes":live_metadata_nodes,
                    "elapsed_ms":page_started.elapsed().as_millis(),"has_more":more,"reason":reason})
            );
        }
        let reply = json!({"target":target_json(&target),"binding":token,"elements":output,"discovery":{"has_more":more,"complete":!more&&q.gap.is_none(),"reason":reason,"visited":q.visited.len()}});
        self.queries.insert(token, q);
        Ok(reply)
    }
    fn read_text(&mut self, r: &Value, until: Instant) -> Result<Value, Error> {
        let target = parse_target(&r["target"])?;
        let window = self.validate(&target)?;
        let token = string(r, "binding")?.to_owned();
        let mut q = self.queries.remove(&token).ok_or(Error::StaleObservation)?;
        let result = (|| {
            if q.target != target || q.expires <= Instant::now() {
                return Err(Error::StaleObservation);
            }
            let id = number(r, "element")?;
            let element = q.elements.get(&id).ok_or(Error::ElementNotFound)?.clone();
            let uia = self.uia()?;
            let current_root = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
            owned(&uia, &element, &q.root, until).map_err(retained_error)?;
            owned(&uia, &element, &current_root, until).map_err(retained_error)?;
            q.validate_region(window, &element, until)?;
            let current = element_text(&element)?;
            check(until)?;
            self.validate(&target)?;
            let current_root = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
            owned(&uia, &element, &q.root, until).map_err(retained_error)?;
            owned(&uia, &element, &current_root, until).map_err(retained_error)?;
            q.validate_region(window, &element, until)?;
            if let Some(previous) = q.texts.get(&id) {
                if previous != &current {
                    return Err(Error::StaleObservation);
                }
            } else {
                if q.texts.values().map(String::len).sum::<usize>() + current.len()
                    > 8 * 1024 * 1024
                {
                    return Err(failed("Retained native text budget exhausted"));
                }
                q.texts.insert(id, current.clone());
            }
            let offset = u32::try_from(number(r, "offset")?).map_err(failed)?;
            let limit = u32::try_from(number(r, "limit")?).map_err(failed)?;
            let page = super::super::text_page::page(&current, offset, limit)?;
            let reply = json!({"target":target_json(&target),"text":page.text,"offset":page.offset,"next_offset":page.next_offset,"total_length":page.total_length,"unit":"utf16"});
            Ok(reply)
        })();
        if result.is_ok()
            || matches!(&result, Err(Error::UnsupportedCapability))
            || matches!(&result,Err(Error::NativeFailure{code,..}) if code=="invalid_text_range")
        {
            self.queries.insert(token, q);
        }
        result
    }
    fn capture(&mut self, target: &WindowTarget, until: Instant) -> Result<Value, Error> {
        let window = self.validate(target)?;
        // CUA also fences the compositor after visible window transitions.
        // Win32 can publish a new maximized rectangle before DWM displays it.
        unsafe { DwmFlush() }.map_err(failed)?;
        let bounds = unobstructed(window)?;
        capture_points_owned(window, bounds)?;
        check(until)?;
        let (image, rendered_size) = capture_rgb(window, bounds)?;
        unsafe { DwmFlush() }.map_err(failed)?;
        self.validate(target)?;
        if !equal_rect(bounds, unobstructed(window)?) {
            return Err(Error::TargetChanged);
        }
        capture_points_owned(window, bounds)?;
        if render_size(window)? != rendered_size {
            return Err(Error::TargetChanged);
        }
        self.pixels.retain(|_, p| p.target != *target);
        if self.pixels.len() >= 32 {
            return Err(failed("Pixel binding budget exhausted"));
        }
        let token = uuid::Uuid::new_v4().to_string();
        let dpi = unsafe { GetDpiForWindow(window) };
        self.pixels.insert(
            token.clone(),
            Pixels {
                target: target.clone(),
                bounds,
                dpi,
                render_size: rendered_size,
                expires: Instant::now() + Duration::from_secs(15),
            },
        );
        Ok(json!({"target":target_json(target),"binding":token,"elements":[],"image":image}))
    }
    fn input_boundary(
        &self,
        target: &WindowTarget,
        window: HWND,
        until: Instant,
    ) -> Result<(), Error> {
        check(until)?;
        if input::closing() {
            return Err(Error::Closed);
        }
        self.validate(target)?;
        if !unsafe { IsWindowEnabled(window) }.as_bool()
            || unsafe { GetForegroundWindow() } != window
        {
            return Err(failed("Exact target is no longer enabled and foreground"));
        }
        Ok(())
    }
    fn window_input(
        &mut self,
        target: &WindowTarget,
        window: HWND,
        r: &Value,
        until: Instant,
        dispatched: &mut bool,
        route: &mut Option<&'static str>,
    ) -> Result<(), Error> {
        input::no_held_keys()?;
        let op = string(r, "operation")?;
        let mut segments: Vec<Vec<INPUT>> = Vec::new();
        let mut point: Option<PointerPoint> = None;
        let mut scroll = None;
        let mut semantic_anchor: Option<(
            IUIAutomation,
            IUIAutomationElement,
            IUIAutomationElement,
            RECT,
            Option<RECT>,
        )> = None;
        match op {
            "typeText" => segments = input::keyboard_segments(Some(string(r, "text")?), None)?,
            "keypress" => {
                let keys = r["keys"]
                    .as_array()
                    .ok_or(Error::InvalidObservation)?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or(Error::InvalidObservation)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                segments = input::keyboard_segments(None, Some(&keys))?;
            }
            "scroll" => {
                let x = r["delta_x"].as_i64().ok_or(Error::InvalidObservation)?;
                let y = r["delta_y"].as_i64().ok_or(Error::InvalidObservation)?;
                if x.unsigned_abs() > 100 || y.unsigned_abs() > 100 || (x == 0 && y == 0) {
                    return Err(failed("Scroll needs nonzero wheel steps within -100..100"));
                }
                scroll = Some((x, y));
                match r["anchor"]["kind"].as_str() {
                    Some("pixels") => {
                        point = Some(self.pixel_point(
                            target,
                            window,
                            string(r, "binding")?,
                            &r["anchor"],
                        )?);
                    }
                    Some("element") => {
                        let q = self
                            .queries
                            .remove(string(r, "binding")?)
                            .ok_or(Error::StaleObservation)?;
                        if q.target != *target || q.expires <= Instant::now() {
                            return Err(Error::StaleObservation);
                        }
                        let element = q
                            .elements
                            .get(&number(&r["anchor"], "element")?)
                            .ok_or(Error::ElementNotFound)?
                            .clone();
                        let uia = self.uia()?;
                        owned(&uia, &element, &q.root, until)?;
                        q.validate_region(window, &element, until)?;
                        let bounds = unobstructed(window)?;
                        let clipped =
                            scroll_element_rect(&uia, &element, &q.root, bounds, q.region, until)?;
                        point = Some((
                            bounds,
                            POINT {
                                x: clipped.left + (clipped.right - clipped.left) / 2,
                                y: clipped.top + (clipped.bottom - clipped.top) / 2,
                            },
                            unsafe { GetDpiForWindow(window) },
                            Some(q.expires),
                            q.render_size,
                        ));
                        semantic_anchor = Some((uia, element, q.root.clone(), clipped, q.region));
                    }
                    None => (),
                    _ => return Err(Error::InvalidObservation),
                }
            }
            "click" => {
                point = Some(self.pixel_point(target, window, string(r, "binding")?, r)?);
                let (down, up) = match r["button"].as_str() {
                    Some("left") => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                    Some("right") => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                    _ => return Err(Error::InvalidObservation),
                };
                let count = number(r, "count")?;
                if !(1..=2).contains(&count) {
                    return Err(Error::InvalidObservation);
                }
                for _ in 0..count {
                    segments.push(vec![input::mouse(down, 0, 0, 0), input::mouse(up, 0, 0, 0)]);
                }
            }
            "focus" => (),
            _ => return Err(Error::UnsupportedCapability),
        }
        self.consume(target);
        self.validate(target)?;
        check(until)?;
        if matches!(op, "click" | "scroll") {
            if point.is_none() {
                let bounds = unobstructed(window)?;
                point = Some((
                    bounds,
                    POINT {
                        x: bounds.left + (bounds.right - bounds.left) / 2,
                        y: bounds.top + (bounds.bottom - bounds.top) / 2,
                    },
                    unsafe { GetDpiForWindow(window) },
                    None,
                    None,
                ));
            }
            let (bounds, hit, dpi, expiry, rendered_size) = point.as_ref().unwrap();
            let count = if op == "click" {
                number(r, "count")?
            } else {
                1
            };
            let receiver = if op == "click" && r["button"] != "left" {
                None
            } else {
                super::posted_mouse::Target::prepare(
                    window,
                    target.app.pid,
                    *hit,
                    op == "scroll",
                    count,
                )?
            };
            if let Some(receiver) = receiver {
                let mut cursor = super::cursor::Emitter::new(&r["cursor_feedback"], target);
                let verify = |state: &Self| -> Result<(), Error> {
                    state.validate(target)?;
                    check(until)?;
                    input::no_held_keys()?;
                    if !unsafe { IsWindowEnabled(window) }.as_bool()
                        || expiry.is_some_and(|e| e <= Instant::now())
                        || *dpi != unsafe { GetDpiForWindow(window) }
                        || !equal_rect(*bounds, unobstructed(window)?)
                        || rendered_size.is_some_and(|saved| {
                            render_size(window).map_or(true, |now| now != saved)
                        })
                    {
                        return Err(Error::StaleObservation);
                    }
                    if let Some((uia, element, root, before, region)) = semantic_anchor.as_ref() {
                        if !equal_rect(
                            scroll_element_rect(uia, element, root, *bounds, *region, until)?,
                            *before,
                        ) {
                            return Err(Error::StaleObservation);
                        }
                    }
                    receiver.validate()
                };
                if op == "click" {
                    for index in 0..count {
                        verify(self)?;
                        *route = Some("background");
                        receiver.click(index == 1, until, dispatched)?;
                        if let Some(cursor) = cursor.as_mut() {
                            cursor.posted(hit.x, hit.y, true);
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                } else if let Some((x, y)) = scroll {
                    for (delta, horizontal, sign) in [(y, false, -1), (x, true, 1)] {
                        for _ in 0..delta.unsigned_abs() {
                            verify(self)?;
                            *route = Some("background");
                            receiver.wheel(
                                horizontal,
                                delta.signum() as i32 * sign * 120,
                                until,
                                dispatched,
                            )?;
                            if let Some(cursor) = cursor.as_mut() {
                                cursor.posted(hit.x, hit.y, false);
                            }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                    }
                }
                return Ok(());
            }
            if r["input_policy"] == "background_only" {
                return Err(Error::NativeFailure {
                    code: "foreground_required".into(),
                    message: "This mouse receiver or gesture requires foreground input".into(),
                });
            }
        }
        *route = Some("foreground");
        input::focus(window, until, dispatched)?;
        self.input_boundary(target, window, until)?;
        let text_scalars: Vec<char> = if op == "typeText" {
            input_sequence::normalized_text(string(r, "text")?)
                .chars()
                .collect()
        } else {
            Vec::new()
        };
        let mut delivery = if text_scalars.iter().any(|scalar| scalar.len_utf16() > 1) {
            let uia = self.uia()?;
            let root = unsafe { uia.ElementFromHandle(window) }.map_err(failed)?;
            let focused = unsafe { uia.GetFocusedElement() }.map_err(failed)?;
            owned(&uia, &focused, &root, until)?;
            let guard =
                super::text_delivery::TextDelivery::new(uia.clone(), focused.clone(), window)?;
            owned(&uia, &focused, &root, until)?;
            self.input_boundary(target, window, until)?;
            Some(guard)
        } else {
            None
        };
        // Scroll centers are computed after activation, which may move a window.
        if let Some((x, y)) = scroll {
            if point.is_none() {
                let bounds = unobstructed(window)?;
                let hit = POINT {
                    x: bounds.left + (bounds.right - bounds.left) / 2,
                    y: bounds.top + (bounds.bottom - bounds.top) / 2,
                };
                point = Some((bounds, hit, unsafe { GetDpiForWindow(window) }, None, None));
            }
            for (delta, flags, sign) in [(y, MOUSEEVENTF_WHEEL, -1i32), (x, MOUSEEVENTF_HWHEEL, 1)]
            {
                for _ in 0..delta.unsigned_abs() {
                    segments.push(vec![input::mouse(
                        flags,
                        (delta.signum() as i32 * sign * 120) as u32,
                        0,
                        0,
                    )]);
                }
            }
        }
        if let Some((_, hit, _, _, _)) = &point {
            segments.insert(0, vec![absolute_move(*hit)?]);
        }
        let mut cursor = point
            .as_ref()
            .and_then(|_| super::cursor::Emitter::new(&r["cursor_feedback"], target));
        let mut placed = false;
        let mut text_index = 0;
        let named_keypress = op == "keypress" && segments.len() == 1;
        let mut terminal_keypress_complete = false;
        let result = input_sequence::run(&segments, |step| match step {
            Step::Verify => {
                if terminal_keypress_complete {
                    // A single named chord may legitimately close this HWND or
                    // open a new foreground dialog. Its complete queue read and
                    // detach prove delivery only; keep stop/held/cleanup checks.
                    return input::terminal_keypress_boundary(until);
                }
                self.input_boundary(target, window, until)?;
                input::no_held_keys()?;
                if let Some((uia, element, root, before, region)) = semantic_anchor.as_ref() {
                    let bounds = unobstructed(window)?;
                    let current = scroll_element_rect(uia, element, root, bounds, *region, until)?;
                    if !equal_rect(current, *before) {
                        return Err(Error::StaleObservation);
                    }
                    self.input_boundary(target, window, until)?;
                    input::no_held_keys()?;
                }
                if let Some((bounds, hit, dpi, expiry, rendered_size)) = &point {
                    if expiry.is_some_and(|expiry| expiry <= Instant::now())
                        || *dpi != unsafe { GetDpiForWindow(window) }
                        || !equal_rect(*bounds, unobstructed(window)?)
                        || rendered_size.is_some_and(|saved| {
                            render_size(window).map_or(true, |now| now != saved)
                        })
                    {
                        return Err(Error::StaleObservation);
                    }
                    verify_point(window, target.app.pid, *hit)?;
                    if placed {
                        let mut current = POINT::default();
                        unsafe { GetCursorPos(&mut current) }.map_err(failed)?;
                        if (current.x - hit.x).abs() > 1 || (current.y - hit.y).abs() > 1 {
                            return Err(failed("Pointer moved after placement"));
                        }
                        verify_point(window, target.app.pid, current)?;
                    }
                }
                Ok(())
            }
            Step::Dispatch(segment) => {
                self.validate(target)?;
                let mut accepted = 0;
                let sent = input::send(
                    segment,
                    window,
                    until,
                    dispatched,
                    &mut accepted,
                    named_keypress,
                );
                if let Some(delivery) = delivery.as_mut() {
                    let scalar = text_scalars[text_index];
                    delivery.inserted(scalar, accepted, segment.len());
                    text_index += 1;
                    terminal_keypress_complete = sent?.terminal_keypress();
                    finish_delivery_read(Ok(()), input::cleanup_failed(), || {
                        if scalar.len_utf16() > 1 {
                            delivery.settle(|| self.validate(target).map(|_| ()))?;
                        }
                        Ok(())
                    })?;
                } else {
                    terminal_keypress_complete = sent?.terminal_keypress();
                }
                if let (Some(cursor), Some((_, hit, _, _, _))) = (cursor.as_mut(), point.as_ref()) {
                    // Emit only after the entire native mouse segment was
                    // inserted. Partial/cleanup and keyboard events emit none.
                    let down = segment.iter().any(|event| unsafe {
                        event.r#type == INPUT_MOUSE
                            && event.Anonymous.mi.dwFlags.0
                                & (MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_RIGHTDOWN).0
                                != 0
                    });
                    cursor.posted(hit.x, hit.y, down);
                }
                if point.is_some() {
                    placed = true;
                }
                // Let normal event processing expose close/focus/overlay changes
                // before validating the next complete segment. This is no proof of effect.
                std::thread::sleep(Duration::from_millis(if point.is_some() { 25 } else { 3 }));
                Ok(())
            }
        });
        finish_delivery_read(result, input::cleanup_failed(), || {
            if let Some(delivery) = delivery.as_mut() {
                delivery.settle(|| self.validate(target).map(|_| ()))?;
            }
            Ok(())
        })
    }
    fn pixel_point(
        &mut self,
        target: &WindowTarget,
        window: HWND,
        token: &str,
        coordinates: &Value,
    ) -> Result<PointerPoint, Error> {
        let pixels = self.pixels.remove(token).ok_or(Error::StaleObservation)?;
        if pixels.target != *target
            || pixels.expires <= Instant::now()
            || pixels.dpi != unsafe { GetDpiForWindow(window) }
            || pixels.render_size != render_size(window)?
            || !equal_rect(pixels.bounds, unobstructed(window)?)
        {
            return Err(Error::StaleObservation);
        }
        let (x, y) = (
            coordinates["x"].as_f64().ok_or(Error::InvalidPoint)?,
            coordinates["y"].as_f64().ok_or(Error::InvalidPoint)?,
        );
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.
            || y < 0.
            || x >= (pixels.bounds.right - pixels.bounds.left) as f64
            || y >= (pixels.bounds.bottom - pixels.bounds.top) as f64
        {
            return Err(Error::InvalidPoint);
        }
        let hit = POINT {
            x: pixels.bounds.left + x.floor() as i32,
            y: pixels.bounds.top + y.floor() as i32,
        };
        Ok((
            pixels.bounds,
            hit,
            pixels.dpi,
            Some(pixels.expires),
            Some(pixels.render_size),
        ))
    }
    fn semantic_admission(
        &self,
        target: &WindowTarget,
        window: HWND,
        uia: &IUIAutomation,
        element: &IUIAutomationElement,
        query: &Query,
        until: Instant,
    ) -> Result<input_sequence::Mutation<'static>, Error> {
        // Pattern and property getters can block. Re-prove membership after
        // those reads, while still outside the active mutation/cleanup interval.
        if !unsafe { element.CurrentIsEnabled() }
            .map_err(failed)?
            .as_bool()
        {
            return Err(Error::ElementNotPressable);
        }
        owned(uia, element, &query.root, until)?;
        query.validate_region(window, element, until)?;
        self.validate(target)?;
        if !unsafe { IsWindowEnabled(window) }.as_bool() {
            return Err(failed("Target is disabled by a modal window"));
        }
        input::mutation(until)
    }
    fn action(&mut self, r: &Value, until: Instant) -> Value {
        let mut dispatched = false;
        let mut route = None;
        let result = (|| {
            let target = parse_target(&r["target"])?;
            let window = self.validate(&target)?;
            let op = string(r, "operation")?;
            let _lease = input::Lease::acquire(until)?;
            self.validate(&target)?;
            // Disabled top-level parents cannot receive input through an open modal.
            if !unsafe { IsWindowEnabled(window) }.as_bool() {
                return Err(failed("Target is disabled by a modal window"));
            }
            if op == "press" || op == "setValue" {
                let token = string(r, "binding")?;
                let q = self.queries.remove(token).ok_or(Error::StaleObservation)?;
                if q.target != target || q.expires <= Instant::now() {
                    return Err(Error::StaleObservation);
                }
                let element = q
                    .elements
                    .get(&number(r, "element")?)
                    .ok_or(Error::ElementNotFound)?
                    .clone();
                let uia = self.uia()?;
                owned(&uia, &element, &q.root, until)?;
                if !unsafe { element.CurrentIsEnabled() }
                    .map_err(failed)?
                    .as_bool()
                {
                    return Err(Error::ElementNotPressable);
                }
                self.consume(&target);
                self.validate(&target)?;
                check(until)?;
                if op == "setValue" {
                    let pattern = unsafe {
                        element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                    }
                    .map_err(failed)?;
                    if unsafe { pattern.CurrentIsReadOnly() }
                        .map_err(failed)?
                        .as_bool()
                    {
                        return Err(Error::UnsupportedCapability);
                    }
                    let value = string(r, "value")?;
                    if !input_sequence::valid_text(value, true) {
                        return Err(failed("Value exceeds the UTF-8/encoded input contract"));
                    }
                    let value = BSTR::from(value);
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q, until)?;
                    dispatched = true;
                    unsafe { pattern.SetValue(&value) }.map_err(failed)?;
                } else if let Ok(pattern) = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                } {
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q, until)?;
                    dispatched = true;
                    unsafe { pattern.Invoke() }.map_err(failed)?;
                } else if let Ok(pattern) = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                } {
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q, until)?;
                    dispatched = true;
                    unsafe { pattern.Toggle() }.map_err(failed)?;
                } else if let Ok(pattern) = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                        UIA_SelectionItemPatternId,
                    )
                } {
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q, until)?;
                    dispatched = true;
                    unsafe { pattern.Select() }.map_err(failed)?;
                } else {
                    return Err(Error::ElementNotPressable);
                }
            } else {
                self.window_input(&target, window, r, until, &mut dispatched, &mut route)?;
            }
            Ok(())
        })();
        let mut reply = match result {
            Ok(()) => json!({"dispatch":"dispatched","evidence":"unverified"}),
            Err(error) => {
                let mut result = windows_error_reply(error, true);
                if dispatched {
                    result["dispatch"] = json!("unknown");
                }
                result
            }
        };
        reply["input_route"] = json!(route);
        if input::cleanup_failed() {
            reply["shutdown_error"] = json!({"code":"input_cleanup_failed","message":"Paired input, accepted text delivery or transaction lease cleanup is unconfirmed"});
        }
        reply
    }
}
fn finish_delivery_read(
    result: Result<(), Error>,
    cleanup_failed: bool,
    settle: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    if cleanup_failed {
        return result.and(Err(Error::NativeFailure {
            code: "input_cleanup_unconfirmed".into(),
            message:
                "Input or queue cleanup is unconfirmed; further UIA delivery reads were skipped"
                    .into(),
        }));
    }
    settle()?;
    result
}
fn windows_error_reply(error: Error, action: bool) -> Value {
    if let Error::NativeFailure { code, message } = error {
        // Keep explicit native diagnostics readable. Debug-formatting here and
        // then wrapping them again across IPC escaped context JSON repeatedly.
        let mut reply = json!({"error":code,"message":message});
        if action {
            reply["dispatch"] = json!("not_dispatched");
        }
        reply
    } else {
        error_reply(error, action)
    }
}
fn optional(
    result: windows::core::Result<IUIAutomationElement>,
) -> Result<Option<IUIAutomationElement>, Error> {
    match result {
        Ok(e) => Ok(Some(e)),
        // windows-rs 0.58 represents S_OK + a null interface as Error::empty
        // (code 0). E_POINTER and all real failures are not normal tree ends.
        Err(e) if e.code().0 == 0 => Ok(None),
        Err(e) => Err(failed(e)),
    }
}
fn runtime_id(element: &IUIAutomationElement) -> Result<Vec<i32>, Error> {
    unsafe {
        let array = element.GetRuntimeId().map_err(failed)?;
        let result = runtime_id_array(array);
        if !array.is_null() {
            let _ = SafeArrayDestroy(array);
        }
        result
    }
}
// UIA client arrays are one-dimensional, zero-based VT_I4. Validate before
// reading; a provider's empty/malformed array is never an element identity.
unsafe fn runtime_id_array(
    array: *const windows::Win32::System::Com::SAFEARRAY,
) -> Result<Vec<i32>, Error> {
    if array.is_null()
        || SafeArrayGetDim(array) != 1
        || SafeArrayGetElemsize(array) != 4
        || SafeArrayGetVartype(array).map_err(failed)? != VT_I4
    {
        return Err(failed("Invalid UIA runtime ID array shape"));
    }
    let lo = SafeArrayGetLBound(array, 1).map_err(failed)?;
    let hi = SafeArrayGetUBound(array, 1).map_err(failed)?;
    let count = hi
        .checked_add(1)
        .filter(|n| lo == 0 && *n > 0 && *n <= 64)
        .ok_or_else(|| failed(format!("Invalid UIA runtime ID bounds: {lo}..{hi}")))?;
    let mut values = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut value = 0i32;
        SafeArrayGetElement(array, &index, (&mut value as *mut i32).cast()).map_err(failed)?;
        values.push(value);
    }
    Ok(values)
}
fn role(id: UIA_CONTROLTYPE_ID) -> &'static str {
    match id.0 {
        50000 => "button",
        50002 => "checkbox",
        50003 => "combo_box",
        50004 => "text_field",
        50005 => "link",
        50007 => "list_item",
        50008 => "list",
        50011 => "menu_item",
        50013 => "radio_button",
        50018 => "tab",
        50019 => "tab_item",
        50020 => "text",
        50023 => "tree",
        50024 => "tree_item",
        50025 => "custom",
        50028 => "data_grid",
        50029 => "data_item",
        50030 => "document",
        50032 => "window",
        50033 => "pane",
        50036 => "table",
        _ => "unknown",
    }
}
// Preserve native failure versus a genuinely unavailable pattern. The typed
// GetCurrentPatternAs wrapper turns S_OK + null into Err(Error::empty()),
// so retain the HRESULT and nullable out pointer at this boundary.
fn pattern_present(status: HRESULT, is_null: bool) -> windows::core::Result<bool> {
    if status == E_NOINTERFACE || status.0 as u32 == UIA_E_NOTSUPPORTED {
        return Ok(false);
    }
    status.ok()?;
    Ok(!is_null)
}
unsafe fn toggle_pattern(
    element: &IUIAutomationElement,
) -> Result<Option<IUIAutomationTogglePattern>, Error> {
    optional_pattern(element, UIA_TogglePatternId)
}
pub(super) unsafe fn optional_pattern<P: Interface>(
    element: &IUIAutomationElement,
    id: UIA_PATTERN_ID,
) -> Result<Option<P>, Error> {
    let mut pointer = std::ptr::null_mut();
    let status = (Interface::vtable(element).GetCurrentPatternAs)(
        Interface::as_raw(element),
        id,
        &P::IID,
        &mut pointer,
    );
    if !pattern_present(status, pointer.is_null()).map_err(failed)? {
        return Ok(None);
    }
    Ok(Some(P::from_raw(pointer)))
}
fn toggle_value(
    state: windows::core::Result<ToggleState>,
) -> windows::core::Result<Option<String>> {
    state.map(|state| match state.0 {
        0 => Some("0".into()),
        1 => Some("1".into()),
        2 => Some("2".into()),
        _ => None, // Unknown provider values are not evidence that a control is off.
    })
}
fn element_row(
    e: &IUIAutomationElement,
    id: u64,
    cached: Option<super::discovery_cache::Metadata>,
) -> Result<Value, Error> {
    unsafe {
        let metadata = super::discovery_cache::metadata_or_live(cached, || {
            Ok(super::discovery_cache::Metadata {
                control: e.CurrentControlType().map_err(failed)?,
                name: e.CurrentName().map_err(failed)?.to_string(),
                enabled: e.CurrentIsEnabled().map_err(failed)?.as_bool(),
                patterns: None,
            })
        })?;
        let role = role(metadata.control);
        let (name, enabled) = (metadata.name, metadata.enabled);
        let patterns = metadata.patterns;
        let mut actions = Vec::new();
        let toggle = if patterns.as_ref().is_some_and(|patterns| !patterns.toggle) {
            None
        } else {
            toggle_pattern(e)?
        };
        let can_press = if let Some(patterns) = &patterns {
            patterns.invoke || patterns.toggle || patterns.selection
        } else {
            optional_pattern::<IUIAutomationInvokePattern>(e, UIA_InvokePatternId)?.is_some()
                || toggle.is_some()
                || optional_pattern::<IUIAutomationSelectionItemPattern>(
                    e,
                    UIA_SelectionItemPatternId,
                )?
                .is_some()
        };
        if can_press {
            actions.push("press");
        }
        let value = if let Some(pattern) = toggle {
            toggle_value(pattern.CurrentToggleState()).map_err(failed)?
        } else if let Some(pattern) = if patterns.as_ref().is_some_and(|patterns| !patterns.value) {
            None
        } else {
            optional_pattern::<IUIAutomationValuePattern>(e, UIA_ValuePatternId)?
        } {
            if !pattern.CurrentIsReadOnly().map_err(failed)?.as_bool() {
                actions.push("setValue");
            }
            if e.CurrentIsPassword().map_err(failed)?.as_bool() {
                None
            } else {
                Some(pattern.CurrentValue().map_err(failed)?.to_string())
            }
        } else {
            None
        };
        Ok(
            json!({"id":id,"label":format!("{role} {name}"),"role":role,"name":name,"value":value,"enabled":enabled,"actions":actions}),
        )
    }
}
fn element_text(element: &IUIAutomationElement) -> Result<String, Error> {
    unsafe {
        if element.CurrentIsPassword().map_err(failed)?.as_bool() {
            return Err(Error::UnsupportedCapability);
        }
        let text = if let Some(pattern) =
            optional_pattern::<IUIAutomationTextPattern>(element, UIA_TextPatternId)?
        {
            pattern
                .DocumentRange()
                .map_err(failed)?
                .GetText(1_048_577)
                .map_err(failed)?
                .to_string()
        } else if let Some(pattern) =
            optional_pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId)?
        {
            pattern.CurrentValue().map_err(failed)?.to_string()
        } else if element.CurrentControlType().map_err(failed)? == UIA_TextControlTypeId {
            element.CurrentName().map_err(failed)?.to_string()
        } else {
            return Err(Error::UnsupportedCapability);
        };
        if text.encode_utf16().count() > 1_048_576 {
            return Err(failed("Native text exceeds retained full-text budget"));
        }
        Ok(text)
    }
}
fn retained_error(error: Error) -> Error {
    match error {
        Error::TargetChanged => Error::StaleObservation,
        other => other,
    }
}
fn owned(
    uia: &IUIAutomation,
    element: &IUIAutomationElement,
    root: &IUIAutomationElement,
    until: Instant,
) -> Result<(), Error> {
    let walker = unsafe { uia.RawViewWalker() }.map_err(failed)?;
    owned_with_walker(uia, &walker, element, root, until)
}
fn owned_with_walker(
    uia: &IUIAutomation,
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    root: &IUIAutomationElement,
    until: Instant,
) -> Result<(), Error> {
    // One page already owns this same raw-view walker. Reuse it for each
    // ancestry proof rather than requesting another COM walker per node; all
    // parent/CompareElements reads and the original deadline remain fresh.
    let mut cursor = element.clone();
    for _ in 0..128 {
        check(until)?;
        if unsafe { uia.CompareElements(&cursor, root) }
            .map_err(failed)?
            .as_bool()
        {
            return Ok(());
        }
        cursor = unsafe { walker.GetParentElement(&cursor) }.map_err(failed)?;
    }
    Err(Error::TargetChanged)
}
fn owned_modal_chain(
    mut window: HWND,
    anchor: HWND,
    pid: u32,
    until: Instant,
) -> Result<bool, Error> {
    let mut visited = HashSet::new();
    for _ in 0..128 {
        check(until)?;
        if window == anchor {
            return Ok(true);
        }
        if window.0.is_null() || !visited.insert(id(window)) {
            return Ok(false);
        }
        let mut actual = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut actual));
        }
        if actual != pid || unsafe { GetAncestor(window, GA_ROOT) } != window {
            return Ok(false);
        }
        window = unsafe { GetWindow(window, GW_OWNER) }.unwrap_or_default();
    }
    Ok(false)
}
fn clipped_rect(a: RECT, b: RECT) -> Result<RECT, Error> {
    let result = RECT {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    if result.left >= result.right || result.top >= result.bottom {
        Err(Error::ElementNotFound)
    } else {
        Ok(result)
    }
}
fn scroll_element_rect(
    uia: &IUIAutomation,
    element: &IUIAutomationElement,
    root: &IUIAutomationElement,
    bounds: RECT,
    region: Option<RECT>,
    until: Instant,
) -> Result<RECT, Error> {
    if unsafe { element.CurrentIsOffscreen() }
        .map_err(failed)?
        .as_bool()
        || !unsafe { element.CurrentIsEnabled() }
            .map_err(failed)?
            .as_bool()
    {
        return Err(Error::ElementNotPressable);
    }
    let mut result = bounds;
    if let Some(region) = region {
        result = clipped_rect(result, region)?;
    }
    let walker = unsafe { uia.RawViewWalker() }.map_err(failed)?;
    let mut current = element.clone();
    for _ in 0..128 {
        check(until)?;
        let rectangle = unsafe { current.CurrentBoundingRectangle() }.map_err(failed)?;
        if rectangle.left >= rectangle.right || rectangle.top >= rectangle.bottom {
            return Err(Error::UnsupportedCapability);
        }
        result = clipped_rect(result, rectangle)?;
        if unsafe { uia.CompareElements(&current, root) }
            .map_err(failed)?
            .as_bool()
        {
            return Ok(result);
        }
        current =
            optional(unsafe { walker.GetParentElement(&current) })?.ok_or(Error::TargetChanged)?;
    }
    Err(Error::TargetChanged)
}
fn unobstructed(window: HWND) -> Result<RECT, Error> {
    super::cursor::begin_proof_cycle();
    if !visible(window) {
        return Err(failed("Capture requires a visible, unminimized window"));
    }
    let bounds = rect(window)?;
    let w = bounds.right - bounds.left;
    let h = bounds.bottom - bounds.top;
    if w <= 0 || h <= 0 || w > 8192 || h > 8192 || w as u64 * h as u64 > 16_777_216 {
        return Err(failed("Capture dimensions exceed pixel budget"));
    }
    let screen = unsafe {
        RECT {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            right: GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: GetSystemMetrics(SM_YVIRTUALSCREEN) + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    if bounds.left < screen.left
        || bounds.top < screen.top
        || bounds.right > screen.right
        || bounds.bottom > screen.bottom
    {
        return Err(failed("Window extends outside the virtual screen"));
    }
    for other in all_windows()? {
        if other == window {
            return Ok(bounds);
        }
        if visible(other)
            && intersects(bounds, rect(other)?)
            && !super::cursor::proven_overlay(other)
        {
            return Err(Error::NativeFailure {
                code: "windows_capture_occluded".into(),
                message: "Window is occluded; screen pixels cannot prove window content. DTW may prepare this exact window's foreground through ordinary input coordination before a fresh capture; persistent topmost or secure overlays must be cleared by their owner.".into(),
            });
        }
    }
    Err(Error::TargetChanged)
}
fn capture_points_owned(window: HWND, bounds: RECT) -> Result<(), Error> {
    // Supplement the conservative z-order rectangle check with CUA's native
    // WindowFromPoint/root proof. Require every sample, rather than accepting a
    // partially covered frame. Inset past invisible borders and rounded corners.
    let inset_x = 16.min((bounds.right - bounds.left - 1) / 2);
    let inset_y = 16.min((bounds.bottom - bounds.top - 1) / 2);
    for (x, y) in [
        (bounds.left + inset_x, bounds.top + inset_y),
        (bounds.right - 1 - inset_x, bounds.top + inset_y),
        (bounds.left + inset_x, bounds.bottom - 1 - inset_y),
        (bounds.right - 1 - inset_x, bounds.bottom - 1 - inset_y),
        (
            bounds.left + (bounds.right - bounds.left) / 2,
            bounds.top + (bounds.bottom - bounds.top) / 2,
        ),
    ] {
        let hit = unsafe { WindowFromPoint(POINT { x, y }) };
        if unsafe { GetAncestor(hit, GA_ROOT) } != window {
            return Err(Error::NativeFailure {
                code: "windows_capture_occluded".into(),
                message: "Window surface is transitioning or occluded; capture pixel ownership is unproven. A fresh coordinated foreground preparation may help; DTW will not synthesize or reuse covered pixels.".into(),
            });
        }
    }
    Ok(())
}
fn rendered_content(rgb: &[u8]) -> bool {
    // CUA capture.rs uses the same 99.5% black sentinel for unsupported native
    // renderers. Reject an unproven render instead of copying desktop pixels.
    let pixels = rgb.len() / 3;
    let stride = (pixels / 1024).max(1);
    let (mut sampled, mut black) = (0, 0);
    for pixel in rgb.chunks_exact(3).step_by(stride) {
        sampled += 1;
        black += usize::from(pixel == [0, 0, 0]);
    }
    sampled > 0 && black * 200 < sampled * 199
}
fn render_size(window: HWND) -> Result<(i32, i32), Error> {
    unsafe {
        let mut raw = RECT::default();
        GetWindowRect(window, &mut raw).map_err(failed)?;
        let mut first = POINT {
            x: raw.left,
            y: raw.top,
        };
        let mut last = POINT {
            x: raw.right,
            y: raw.bottom,
        };
        PhysicalToLogicalPointForPerMonitorDPI(window, &mut first)
            .ok()
            .map_err(failed)?;
        PhysicalToLogicalPointForPerMonitorDPI(window, &mut last)
            .ok()
            .map_err(failed)?;
        let width = last.x.checked_sub(first.x).ok_or(Error::InvalidPoint)?;
        let height = last.y.checked_sub(first.y).ok_or(Error::InvalidPoint)?;
        if width <= 0
            || height <= 0
            || width > 8192
            || height > 8192
            || width as u64 * height as u64 > 16_777_216
        {
            return Err(Error::InvalidPoint);
        }
        Ok((width, height))
    }
}
fn capture_rgb(window: HWND, bounds: RECT) -> Result<(Value, (i32, i32)), Error> {
    unsafe {
        let mut raw = RECT::default();
        GetWindowRect(window, &mut raw).map_err(failed)?;
        let w = raw.right.checked_sub(raw.left).ok_or(Error::InvalidPoint)?;
        let h = raw.bottom.checked_sub(raw.top).ok_or(Error::InvalidPoint)?;
        if w <= 0
            || h <= 0
            || w > 8192
            || h > 8192
            || w as u64 * h as u64 > 16_777_216
            || !equal_rect(maximized_frame(raw, Some(bounds)), bounds)
        {
            return Err(Error::TargetChanged);
        }
        let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        let (crop_x, crop_y) = (bounds.left - raw.left, bounds.top - raw.top);
        // PrintWindow paints in the target's backing coordinates. A DPI-unaware
        // window on a scaled display can paint a 416x469 bitmap into a physical
        // 520x586 frame. Derive its own logical extent through the HWND's native
        // transform, then map the exact-window render into physical output.
        // The worker and public input coordinates remain per-monitor/physical.
        let (rw, rh) = render_size(window)?;
        let screen = GetDC(HWND::default());
        if screen.0.is_null() {
            return Err(failed("GetDC failed"));
        }
        let memory = CreateCompatibleDC(screen);
        if memory.0.is_null() {
            ReleaseDC(HWND::default(), screen);
            return Err(failed("CreateCompatibleDC failed"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rw,
                biHeight: -rh,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = (|| {
            let bitmap = CreateDIBSection(
                memory,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                HANDLE::default(),
                0,
            )
            .map_err(failed)?;
            if bits.is_null() {
                let _ = DeleteObject(bitmap);
                return Err(failed("CreateDIBSection returned no pixel storage"));
            }
            let old = SelectObject(memory, bitmap);
            if old.0.is_null() || old.0 as isize == -1 {
                let _ = DeleteObject(bitmap);
                return Err(failed("Cannot select native render bitmap"));
            }
            std::slice::from_raw_parts_mut(bits as *mut u8, rw as usize * rh as usize * 4).fill(0);
            // Port CUA 0.38.0 capture.rs's PW_RENDERFULLCONTENT route. A desktop
            // BitBlt can return another app during maximize/restore animation
            // even when Win32 rectangles and hit tests already name this HWND.
            // Render only this exact window; there is no desktop-copy fallback.
            let captured = render_window(window, memory, 2).ok().map_err(failed);
            let result = captured.and_then(|_| {
                let bgra =
                    std::slice::from_raw_parts(bits as *const u8, rw as usize * rh as usize * 4);
                let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
                for y in crop_y..crop_y + height {
                    let sy = y as usize * rh as usize / h as usize;
                    for x in crop_x..crop_x + width {
                        let sx = x as usize * rw as usize / w as usize;
                        let offset = (sy * rw as usize + sx) * 4;
                        let p = &bgra[offset..offset + 4];
                        rgb.extend_from_slice(&[p[2], p[1], p[0]]);
                    }
                }
                let mut after = RECT::default();
                GetWindowRect(window, &mut after).map_err(failed)?;
                if !equal_rect(raw, after) || render_size(window)? != (rw, rh) {
                    return Err(Error::TargetChanged);
                }
                if !rendered_content(&rgb) {
                    return Err(failed(
                        "Native window render is blank or unsupported; capture content is unproven",
                    ));
                }
                Ok((encode_png(width as u32, height as u32, &rgb)?, (rw, rh)))
            });
            SelectObject(memory, old);
            let _ = DeleteObject(bitmap);
            result
        })();
        let _ = DeleteDC(memory);
        ReleaseDC(HWND::default(), screen);
        result
    }
}

fn verify_point(window: HWND, pid: u32, point: POINT) -> Result<(), Error> {
    unsafe {
        let hit = WindowFromPoint(point);
        let mut hit_pid = 0;
        GetWindowThreadProcessId(hit, Some(&mut hit_pid));
        if GetAncestor(hit, GA_ROOT) != window || hit_pid != pid {
            Err(failed("Point is occluded or belongs to a foreign process"))
        } else {
            Ok(())
        }
    }
}
fn absolute_move(point: POINT) -> Result<INPUT, Error> {
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if vw <= 1 || vh <= 1 {
        return Err(Error::InvalidPoint);
    }
    Ok(input::mouse(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        0,
        ((point.x - vx) as i64 * 65535 / (vw - 1) as i64) as i32,
        ((point.y - vy) as i64 * 65535 / (vh - 1) as i64) as i32,
    ))
}

#[cfg(test)]
mod semantic_tests {
    use super::*;
    #[test]
    fn failed_queue_cleanup_skips_all_delivery_uia_reads_and_preserves_original_error() {
        let original = Error::BackendFailed("original dispatch uncertainty".into());
        assert_eq!(
            finish_delivery_read(Err(original.clone()), true, || panic!(
                "UIA forbidden after uncertain detach"
            )),
            Err(original)
        );
        assert!(
            matches!(finish_delivery_read(Ok(()),true,||panic!("UIA forbidden after uncertain detach")),Err(Error::NativeFailure{code,..}) if code=="input_cleanup_unconfirmed")
        );
        let settled = std::cell::Cell::new(false);
        finish_delivery_read(Ok(()), false, || {
            settled.set(true);
            Ok(())
        })
        .unwrap();
        assert!(settled.get());
    }
    #[test]
    fn explicit_windows_diagnostics_preserve_message_and_dispatch_without_debug_wrapping() {
        let error = || Error::NativeFailure {
            code: "desktop_enumeration_unproven".into(),
            message: "Native worker context: {\"thread_desktop\":\"fixture\"}".into(),
        };
        let read = windows_error_reply(error(), false);
        assert_eq!(read["error"], "desktop_enumeration_unproven");
        assert_eq!(
            read["message"],
            "Native worker context: {\"thread_desktop\":\"fixture\"}"
        );
        assert!(read.get("dispatch").is_none());
        assert_eq!(
            windows_error_reply(error(), true)["dispatch"],
            "not_dispatched"
        );
    }
    #[test]
    fn native_runtime_id_arrays_reject_empty_wrong_type_shape_and_preserve_signed_ids() {
        use windows::Win32::System::Variant::VT_R8;
        unsafe {
            assert!(runtime_id_array(std::ptr::null()).is_err());
            for (kind, lo, count) in [(VT_I4, 0, 0), (VT_R8, 0, 2), (VT_I4, 1, 2)] {
                let array = SafeArrayCreateVector(kind, lo, count);
                assert!(!array.is_null());
                assert!(runtime_id_array(array).is_err());
                SafeArrayDestroy(array).unwrap();
            }
            let array = SafeArrayCreateVector(VT_I4, 0, 3);
            assert!(!array.is_null());
            let expected = [42i32, -17, i32::MAX];
            for (index, value) in expected.iter().enumerate() {
                SafeArrayPutElement(array, &(index as i32), (value as *const i32).cast()).unwrap();
            }
            assert_eq!(runtime_id_array(array).unwrap(), expected);
            SafeArrayDestroy(array).unwrap();
        }
    }
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_POINTER, S_OK};

    #[test]
    fn blank_native_render_is_not_replaced_with_desktop_pixels() {
        assert!(!rendered_content(&[]));
        assert!(!rendered_content(&[0; 3000]));
        assert!(rendered_content(&[1; 3000]));
        let mut dark = vec![0; 3000];
        dark[0..30].fill(1);
        assert!(rendered_content(&dark));
    }

    #[test]
    fn maximized_frame_excludes_invisible_resize_borders() {
        let raw = RECT {
            left: -8,
            top: -8,
            right: 2568,
            bottom: 1388,
        };
        let visible = RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1380,
        };
        assert!(equal_rect(maximized_frame(raw, Some(visible)), visible));
    }

    #[test]
    fn invalid_or_unavailable_dwm_frame_preserves_native_bounds() {
        let raw = RECT {
            left: -8,
            top: -8,
            right: 2568,
            bottom: 1388,
        };
        for frame in [
            None,
            Some(RECT::default()),
            Some(RECT { left: -9, ..raw }),
            Some(RECT { right: 2569, ..raw }),
            Some(RECT {
                left: 5,
                right: 4,
                ..raw
            }),
        ] {
            assert!(equal_rect(maximized_frame(raw, frame), raw));
        }
    }

    #[test]
    fn visible_frame_does_not_clip_real_offscreen_content() {
        let raw = RECT {
            left: -108,
            top: -8,
            right: 908,
            bottom: 708,
        };
        let visible = RECT {
            left: -100,
            top: 0,
            right: 900,
            bottom: 700,
        };
        let actual = maximized_frame(raw, Some(visible));
        assert!(equal_rect(actual, visible));
        assert!(actual.left < 0);
    }

    #[test]
    fn successful_null_tree_edge_is_not_a_native_read_gap() {
        // This is the exact Result produced by Type::from_abi for S_OK + NULL
        // in the locked windows-core 0.58 binding.
        assert_eq!(windows::core::Error::empty().code(), S_OK);
        assert!(optional(Err(windows::core::Error::empty()))
            .unwrap()
            .is_none());
        for code in [
            E_POINTER,
            E_ACCESSDENIED,
            HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32),
        ] {
            // The traversal records an Err as native_read_gap; real HRESULT
            // failures must not be converted into a normal terminal edge.
            assert!(optional(Err(windows::core::Error::from_hresult(code))).is_err());
        }
    }

    #[test]
    fn retained_region_rejects_window_move_resize_and_dpi_change() {
        let bounds = RECT {
            left: 10,
            top: 20,
            right: 210,
            bottom: 220,
        };
        let region = RECT {
            left: 20,
            top: 30,
            right: 100,
            bottom: 110,
        };
        let element = RECT {
            left: 30,
            top: 40,
            right: 90,
            bottom: 100,
        };
        assert!(validate_region_geometry(
            Some((bounds, 96)),
            Some(region),
            || Ok(element),
            || Ok((bounds, 96))
        )
        .is_ok());
        for current in [
            (
                RECT {
                    left: 11,
                    right: 211,
                    ..bounds
                },
                96,
            ),
            (
                RECT {
                    bottom: 221,
                    ..bounds
                },
                96,
            ),
            (bounds, 144),
        ] {
            assert!(matches!(
                validate_region_geometry(
                    Some((bounds, 96)),
                    Some(region),
                    || Ok(element),
                    || Ok(current)
                ),
                Err(Error::TargetChanged)
            ));
        }
    }

    #[test]
    fn retained_region_rejects_departed_or_unreadable_control() {
        let region = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        for moved in [
            RECT {
                left: 100,
                top: 10,
                right: 120,
                bottom: 30,
            },
            RECT {
                left: 20,
                top: 20,
                right: 20,
                bottom: 30,
            },
        ] {
            assert!(matches!(
                validate_region_geometry(
                    None,
                    Some(region),
                    || Ok(moved),
                    || panic!("no capture constraint")
                ),
                Err(Error::StaleObservation)
            ));
        }
        assert!(matches!(
            validate_region_geometry(
                None,
                Some(region),
                || Err(Error::Closed),
                || panic!("no capture constraint")
            ),
            Err(Error::Closed)
        ));
        assert!(validate_region_geometry(
            None,
            None,
            || panic!("unscoped semantic refs do not require rectangles"),
            || panic!("unscoped semantic refs do not require capture geometry")
        )
        .is_ok());
    }

    #[test]
    fn capture_geometry_is_checked_after_blocking_element_geometry() {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let dpi = std::cell::Cell::new(96);
        let result = validate_region_geometry(
            Some((bounds, 96)),
            Some(bounds),
            || {
                dpi.set(144); // Window moved to another DPI while UIA yielded.
                Ok(bounds)
            },
            || Ok((bounds, dpi.get())),
        );
        assert!(matches!(result, Err(Error::TargetChanged)));
    }

    #[test]
    fn region_geometry_is_read_only_for_role_and_full_text_matches() {
        let row = json!({"role":"text_field", "name":"Field", "value":format!("{} needle", "x".repeat(600))});
        let region = Some(RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        });
        for (role, text) in [(Some("button"), None), (None, Some("absent"))] {
            assert_eq!(
                discovery_matches::<()>(&row, role, text, region, || {
                    panic!("a nonmatching candidate must not read geometry")
                }),
                Ok(false)
            );
        }
        assert_eq!(
            discovery_matches(&row, Some("text_field"), Some("needle"), region, || {
                Err("native_read_gap")
            }),
            Err("native_read_gap")
        );
        assert_eq!(
            discovery_matches::<()>(&row, None, Some("needle"), region, || {
                Ok(RECT {
                    left: 10,
                    top: 10,
                    right: 20,
                    bottom: 20,
                })
            }),
            Ok(true)
        );
        assert_eq!(
            discovery_matches::<()>(&row, None, Some("needle"), None, || {
                panic!("an unscoped match must not read geometry")
            }),
            Ok(true)
        );
    }

    #[test]
    fn region_intersection_requires_positive_area_in_both_rectangles() {
        let region = RECT {
            left: 10,
            top: 10,
            right: 30,
            bottom: 30,
        };
        let inside = RECT {
            left: 15,
            top: 15,
            right: 25,
            bottom: 25,
        };
        assert!(intersects(region, inside));
        assert!(intersects(inside, region));
        for absent in [
            RECT {
                right: 15,
                ..inside
            },
            RECT {
                bottom: 15,
                ..inside
            },
            RECT {
                right: 14,
                ..inside
            },
            RECT {
                bottom: 14,
                ..inside
            },
            RECT {
                left: 30,
                right: 40,
                ..inside
            },
            RECT {
                top: 30,
                bottom: 40,
                ..inside
            },
        ] {
            assert!(!intersects(region, absent));
            assert!(!intersects(absent, region));
        }
    }

    #[test]
    fn common_checkbox_role_is_canonical_without_guessing_edit_layout() {
        assert_eq!(role(UIA_CheckBoxControlTypeId), "checkbox");
        assert_eq!(role(UIA_EditControlTypeId), "text_field");
        assert_eq!(role(UIA_CONTROLTYPE_ID(-1)), "unknown");
    }

    #[test]
    fn toggle_values_preserve_all_three_native_states_and_unknowns() {
        for (state, value) in [
            (ToggleState_Off, "0"),
            (ToggleState_On, "1"),
            (ToggleState_Indeterminate, "2"),
        ] {
            assert_eq!(toggle_value(Ok(state)).unwrap().as_deref(), Some(value));
        }
        assert_eq!(toggle_value(Ok(ToggleState(99))).unwrap(), None);
        assert_eq!(toggle_value(Ok(ToggleState(-1))).unwrap(), None);
        let failure = windows::core::Error::from_hresult(E_ACCESSDENIED);
        assert_eq!(
            toggle_value(Err(failure)).unwrap_err().code(),
            E_ACCESSDENIED
        );
    }

    #[test]
    fn missing_native_pattern_is_distinct_from_a_failed_native_read() {
        assert!(pattern_present(S_OK, false).unwrap());
        assert!(!pattern_present(S_OK, true).unwrap());
        assert!(!pattern_present(E_NOINTERFACE, true).unwrap());
        assert!(!pattern_present(HRESULT(UIA_E_NOTSUPPORTED as i32), true).unwrap());
        for code in [
            E_POINTER,
            E_ACCESSDENIED,
            HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32),
        ] {
            assert_eq!(pattern_present(code, true).unwrap_err().code(), code);
            assert_eq!(pattern_present(code, false).unwrap_err().code(), code);
        }
    }
}
