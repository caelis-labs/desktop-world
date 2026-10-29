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
        Foundation::{
            CloseHandle, BOOL, E_NOINTERFACE, FILETIME, HANDLE, HWND, LPARAM, POINT, RECT,
        },
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
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
        },
        UI::{
            Accessibility::*,
            HiDpi::{
                GetDpiForWindow, SetProcessDpiAwarenessContext,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
};

const MARKER: windows::core::PCWSTR = w!("DesktopWorld.WindowIncarnation.v1");
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
fn rect(window: HWND) -> Result<RECT, Error> {
    let mut result = RECT::default();
    unsafe { GetWindowRect(window, &mut result) }.map_err(failed)?;
    Ok(result)
}
fn equal_rect(a: RECT, b: RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}
fn intersects(a: RECT, b: RECT) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}
fn visible(window: HWND) -> bool {
    unsafe {
        let mut cloaked = 0u32;
        IsWindowVisible(window).as_bool()
            && !IsIconic(window).as_bool()
            && DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4)
                .is_ok()
            && cloaked == 0
    }
}
unsafe extern "system" fn enum_window(window: HWND, param: LPARAM) -> BOOL {
    let out = &mut *(param.0 as *mut Vec<HWND>);
    if out.len() < 4096 {
        out.push(window);
        BOOL(1)
    } else {
        BOOL(0)
    }
}
fn all_windows() -> Result<Vec<HWND>, Error> {
    let mut out = Vec::new();
    unsafe { EnumWindows(Some(enum_window), LPARAM(&mut out as *mut _ as isize)) }
        .map_err(failed)?;
    if out.len() >= 4096 {
        return Err(failed("Desktop inventory exceeds 4096 windows"));
    }
    Ok(out)
}
fn title(window: HWND) -> String {
    let mut text = [0u16; 1024];
    let len = unsafe { GetWindowTextW(window, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..len])
}
fn marker(window: HWND, create: bool) -> Result<u64, Error> {
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
    frontier: Vec<(IUIAutomationElement, u32)>,
    visited: HashSet<Vec<i32>>,
    elements: HashMap<u64, IUIAutomationElement>,
    pending: Option<Value>,
    role: Option<String>,
    text: Option<String>,
    limit: usize,
    expires: Instant,
    gap: Option<String>,
}
struct Pixels {
    target: WindowTarget,
    bounds: RECT,
    dpi: u32,
    expires: Instant,
}
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
                "catalog" => {
                    let mut apps = HashMap::new();
                    for window in all_windows()? {
                        check(until)?;
                        if !unsafe { IsWindowVisible(window) }.as_bool() {
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
                "capture" => self.capture(&parse_target(&request["target"])?, until),
                _ if action => Ok(self.action(request, until)),
                _ => Err(Error::UnsupportedCapability),
            }
        })();
        result.unwrap_or_else(|error| error_reply(error, action))
    }
    fn observe(&mut self, r: &Value, until: Instant, next: bool) -> Result<Value, Error> {
        let target = parse_target(&r["target"])?;
        let window = self.validate(&target)?;
        let uia = self.uia()?;
        let token = if next {
            string(r, "binding")?.to_owned()
        } else {
            self.queries.retain(|_, q| q.target != target);
            if self.queries.len() >= 8 {
                return Err(failed("Eight active semantic query limit"));
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
            Query {
                target: target.clone(),
                frontier: vec![(root.clone(), 0)],
                root,
                visited: HashSet::new(),
                elements: HashMap::new(),
                pending: None,
                role,
                text,
                limit,
                expires: Instant::now() + Duration::from_secs(15),
                gap: None,
            }
        };
        if q.target != target || q.expires <= Instant::now() {
            return Err(Error::StaleObservation);
        }
        let walker = unsafe { uia.RawViewWalker() }.map_err(failed)?;
        let page_until = until.min(Instant::now() + Duration::from_millis(750));
        let mut output = Vec::new();
        let mut bytes = 0;
        let mut steps = 0;
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
            if q.visited.len() >= 8192 || q.elements.len() >= 4096 {
                q.frontier.clear();
                q.gap = Some("scan_limit".into());
                break;
            }
            let (element, depth) = q.frontier.pop().unwrap();
            steps += 1;
            let runtime = runtime_id(&element)?;
            if !q.visited.insert(runtime) {
                q.gap.get_or_insert("tree_changed".into());
                continue;
            }
            if depth > 0 {
                match optional(unsafe { walker.GetNextSiblingElement(&element) }) {
                    Ok(Some(sibling)) => q.frontier.push((sibling, depth)),
                    Ok(None) => (),
                    Err(_) => {
                        q.gap.get_or_insert("native_read_gap".into());
                    }
                }
            }
            match optional(unsafe { walker.GetFirstChildElement(&element) }) {
                Ok(Some(child)) if depth < 64 => q.frontier.push((child, depth + 1)),
                Ok(Some(_)) => {
                    q.gap.get_or_insert("depth_limit".into());
                }
                Ok(None) => (),
                Err(_) => {
                    q.gap.get_or_insert("native_read_gap".into());
                }
            }
            let row = match element_row(&element, (q.elements.len() + 1) as u64) {
                Ok(row) => row,
                Err(_) => {
                    q.gap.get_or_insert("native_read_gap".into());
                    continue;
                }
            };
            if q.role
                .as_ref()
                .is_some_and(|role| row["role"].as_str() != Some(role))
                || q.text.as_ref().is_some_and(|text| {
                    !format!(
                        "{} {}",
                        row["name"].as_str().unwrap_or(""),
                        row["value"].as_str().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(text)
                })
            {
                continue;
            }
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
        let more = !q.frontier.is_empty() || q.pending.is_some();
        let reason = q.gap.clone().or_else(|| more.then(|| "page_limit".into()));
        let reply = json!({"target":target_json(&target),"binding":token,"elements":output,"discovery":{"has_more":more,"complete":!more&&q.gap.is_none(),"reason":reason,"visited":q.visited.len()}});
        self.queries.insert(token, q);
        Ok(reply)
    }
    fn capture(&mut self, target: &WindowTarget, until: Instant) -> Result<Value, Error> {
        let window = self.validate(target)?;
        let bounds = unobstructed(window)?;
        check(until)?;
        let image = capture_rgb(bounds)?;
        self.validate(target)?;
        if !equal_rect(bounds, unobstructed(window)?) {
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
    ) -> Result<(), Error> {
        input::no_held_keys()?;
        let op = string(r, "operation")?;
        let mut segments: Vec<Vec<INPUT>> = Vec::new();
        let mut point: Option<(RECT, POINT, u32, Option<Instant>)> = None;
        let mut scroll = None;
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
            }
            "click" => {
                let p = self
                    .pixels
                    .remove(string(r, "binding")?)
                    .ok_or(Error::StaleObservation)?;
                if p.target != *target
                    || p.expires <= Instant::now()
                    || p.dpi != unsafe { GetDpiForWindow(window) }
                    || !equal_rect(p.bounds, unobstructed(window)?)
                {
                    return Err(Error::StaleObservation);
                }
                let (x, y) = (
                    r["x"].as_f64().ok_or(Error::InvalidPoint)?,
                    r["y"].as_f64().ok_or(Error::InvalidPoint)?,
                );
                if !x.is_finite()
                    || !y.is_finite()
                    || x < 0.0
                    || y < 0.0
                    || x >= (p.bounds.right - p.bounds.left) as f64
                    || y >= (p.bounds.bottom - p.bounds.top) as f64
                {
                    return Err(Error::InvalidPoint);
                }
                let hit = POINT {
                    x: p.bounds.left + x.floor() as i32,
                    y: p.bounds.top + y.floor() as i32,
                };
                point = Some((p.bounds, hit, p.dpi, Some(p.expires)));
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
        input::focus(window, until, dispatched)?;
        self.input_boundary(target, window, until)?;
        // Scroll centers are computed after activation, which may move a window.
        if let Some((x, y)) = scroll {
            let bounds = unobstructed(window)?;
            let hit = POINT {
                x: bounds.left + (bounds.right - bounds.left) / 2,
                y: bounds.top + (bounds.bottom - bounds.top) / 2,
            };
            point = Some((bounds, hit, unsafe { GetDpiForWindow(window) }, None));
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
        if let Some((_, hit, _, _)) = &point {
            segments.insert(0, vec![absolute_move(*hit)?]);
        }
        let mut placed = false;
        input_sequence::run(&segments, |step| match step {
            Step::Verify => {
                self.input_boundary(target, window, until)?;
                input::no_held_keys()?;
                if let Some((bounds, hit, dpi, expiry)) = &point {
                    if expiry.is_some_and(|expiry| expiry <= Instant::now())
                        || *dpi != unsafe { GetDpiForWindow(window) }
                        || !equal_rect(*bounds, unobstructed(window)?)
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
                input::send(segment, window, until, dispatched)?;
                if point.is_some() {
                    placed = true;
                }
                // Let normal event processing expose close/focus/overlay changes
                // before validating the next complete segment. This is no proof of effect.
                std::thread::sleep(Duration::from_millis(if point.is_some() { 25 } else { 3 }));
                Ok(())
            }
        })
    }
    fn semantic_admission(
        &self,
        target: &WindowTarget,
        window: HWND,
        uia: &IUIAutomation,
        element: &IUIAutomationElement,
        root: &IUIAutomationElement,
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
        owned(uia, element, root, until)?;
        self.validate(target)?;
        if !unsafe { IsWindowEnabled(window) }.as_bool() {
            return Err(failed("Target is disabled by a modal window"));
        }
        input::mutation(until)
    }
    fn action(&mut self, r: &Value, until: Instant) -> Value {
        let mut dispatched = false;
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
                        self.semantic_admission(&target, window, &uia, &element, &q.root, until)?;
                    dispatched = true;
                    unsafe { pattern.SetValue(&value) }.map_err(failed)?;
                } else if let Ok(pattern) = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                } {
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q.root, until)?;
                    dispatched = true;
                    unsafe { pattern.Invoke() }.map_err(failed)?;
                } else if let Ok(pattern) = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                } {
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q.root, until)?;
                    dispatched = true;
                    unsafe { pattern.Toggle() }.map_err(failed)?;
                } else if let Ok(pattern) = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                        UIA_SelectionItemPatternId,
                    )
                } {
                    let _mutation =
                        self.semantic_admission(&target, window, &uia, &element, &q.root, until)?;
                    dispatched = true;
                    unsafe { pattern.Select() }.map_err(failed)?;
                } else {
                    return Err(Error::ElementNotPressable);
                }
            } else {
                self.window_input(&target, window, r, until, &mut dispatched)?;
            }
            Ok(())
        })();
        let mut reply = match result {
            Ok(()) => json!({"dispatch":"dispatched","evidence":"unverified"}),
            Err(error) => {
                let mut result = error_reply(error, true);
                if dispatched {
                    result["dispatch"] = json!("unknown");
                }
                result
            }
        };
        if input::cleanup_failed() {
            reply["shutdown_error"] = json!({"code":"input_cleanup_failed","message":"Paired input or transaction lease cleanup is unconfirmed"});
        }
        reply
    }
}
fn optional(
    result: windows::core::Result<IUIAutomationElement>,
) -> Result<Option<IUIAutomationElement>, Error> {
    match result {
        Ok(e) => Ok(Some(e)),
        Err(e) if e.code().0 == 0x80004003u32 as i32 => Ok(None),
        Err(e) => Err(failed(e)),
    }
}
fn runtime_id(element: &IUIAutomationElement) -> Result<Vec<i32>, Error> {
    unsafe {
        let array = element.GetRuntimeId().map_err(failed)?;
        let result = (|| {
            let lo = SafeArrayGetLBound(array, 1).map_err(failed)?;
            let hi = SafeArrayGetUBound(array, 1).map_err(failed)?;
            let count = hi
                .checked_sub(lo)
                .and_then(|n| n.checked_add(1))
                .filter(|n| *n > 0 && *n <= 64)
                .ok_or_else(|| failed("Invalid UIA runtime ID"))?;
            let mut data = std::ptr::null_mut();
            SafeArrayAccessData(array, &mut data).map_err(failed)?;
            let result = std::slice::from_raw_parts(data as *const i32, count as usize).to_vec();
            SafeArrayUnaccessData(array).map_err(failed)?;
            Ok(result)
        })();
        let _ = SafeArrayDestroy(array);
        result
    }
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
fn toggle_pattern_present(status: HRESULT, is_null: bool) -> windows::core::Result<bool> {
    if status == E_NOINTERFACE || status.0 as u32 == UIA_E_NOTSUPPORTED {
        return Ok(false);
    }
    status.ok()?;
    Ok(!is_null)
}
unsafe fn toggle_pattern(
    element: &IUIAutomationElement,
) -> Result<Option<IUIAutomationTogglePattern>, Error> {
    let mut pointer = std::ptr::null_mut();
    let status = (Interface::vtable(element).GetCurrentPatternAs)(
        Interface::as_raw(element),
        UIA_TogglePatternId,
        &IUIAutomationTogglePattern::IID,
        &mut pointer,
    );
    if !toggle_pattern_present(status, pointer.is_null()).map_err(failed)? {
        return Ok(None);
    }
    Ok(Some(IUIAutomationTogglePattern::from_raw(pointer)))
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
fn element_row(e: &IUIAutomationElement, id: u64) -> Result<Value, Error> {
    unsafe {
        let role = role(e.CurrentControlType().map_err(failed)?);
        let name = e
            .CurrentName()
            .map_err(failed)?
            .to_string()
            .chars()
            .take(512)
            .collect::<String>();
        let enabled = e.CurrentIsEnabled().map_err(failed)?.as_bool();
        let mut actions = Vec::new();
        let toggle = toggle_pattern(e)?;
        if e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
            .is_ok()
            || toggle.is_some()
            || e.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            )
            .is_ok()
        {
            actions.push("press");
        }
        let value = if let Some(pattern) = toggle {
            toggle_value(pattern.CurrentToggleState()).map_err(failed)?
        } else if let Ok(pattern) =
            e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        {
            if !pattern.CurrentIsReadOnly().map_err(failed)?.as_bool() {
                actions.push("setValue");
            }
            if e.CurrentIsPassword().map_err(failed)?.as_bool() {
                None
            } else {
                Some(
                    pattern
                        .CurrentValue()
                        .map_err(failed)?
                        .to_string()
                        .chars()
                        .take(512)
                        .collect::<String>(),
                )
            }
        } else {
            None
        };
        Ok(
            json!({"id":id,"label":format!("{role} {name}"),"role":role,"name":name,"value":value,"enabled":enabled,"actions":actions}),
        )
    }
}
fn owned(
    uia: &IUIAutomation,
    element: &IUIAutomationElement,
    root: &IUIAutomationElement,
    until: Instant,
) -> Result<(), Error> {
    let walker = unsafe { uia.RawViewWalker() }.map_err(failed)?;
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
fn unobstructed(window: HWND) -> Result<RECT, Error> {
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
        if visible(other) && intersects(bounds, rect(other)?) {
            return Err(failed(
                "Window is occluded; screen pixels cannot prove window content",
            ));
        }
    }
    Err(Error::TargetChanged)
}
fn capture_rgb(bounds: RECT) -> Result<Value, Error> {
    unsafe {
        let (w, h) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
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
                biWidth: w,
                biHeight: -h,
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
            let old = SelectObject(memory, bitmap);
            let captured = BitBlt(
                memory,
                0,
                0,
                w,
                h,
                screen,
                bounds.left,
                bounds.top,
                SRCCOPY | CAPTUREBLT,
            )
            .map_err(failed);
            let result = captured.and_then(|_| {
                let bgra =
                    std::slice::from_raw_parts(bits as *const u8, w as usize * h as usize * 4);
                let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
                for p in bgra.chunks_exact(4) {
                    rgb.extend_from_slice(&[p[2], p[1], p[0]]);
                }
                encode_png(w as u32, h as u32, &rgb)
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
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_POINTER, S_OK};

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
    fn missing_toggle_pattern_is_distinct_from_a_failed_native_read() {
        assert!(toggle_pattern_present(S_OK, false).unwrap());
        assert!(!toggle_pattern_present(S_OK, true).unwrap());
        assert!(!toggle_pattern_present(E_NOINTERFACE, true).unwrap());
        assert!(!toggle_pattern_present(HRESULT(UIA_E_NOTSUPPORTED as i32), true).unwrap());
        for code in [
            E_POINTER,
            E_ACCESSDENIED,
            HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32),
        ] {
            assert_eq!(toggle_pattern_present(code, true).unwrap_err().code(), code);
        }
    }
}
