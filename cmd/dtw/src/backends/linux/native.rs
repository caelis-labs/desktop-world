//! Narrow adaptation of CUA 0.38.0's XRes ownership, XGetImage and XTest path.
//! No shell tools, clipboard writes, temporary keymap edits or Wayland fallback.
use super::{client_pid::*, worker};
#[path = "inventory.rs"]
mod inventory;
use crate::backends::input_sequence::{self, Cleanup, HeldInput, HeldInputs, Step};
use x11rb::errors::ReplyError;
type InputEvent = (u8, u8, i16, i16);
use crate::{backends::native_client::*, computer::*};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        res::{ClientIdMask, ClientIdSpec, ConnectionExt as _},
        xinput::ConnectionExt as _,
        xproto::*,
        xtest::ConnectionExt as _,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
    CURRENT_TIME,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bounds {
    x: i16,
    y: i16,
    w: u16,
    h: u16,
}
struct Pixels {
    target: WindowTarget,
    bounds: Bounds,
    expires: Instant,
}
pub struct Store {
    conn: RustConnection,
    root: u32,
    atoms: HashMap<String, u32>,
    windows: HashMap<u64, WindowTarget>,
    pixels: HashMap<String, Pixels>,
}
struct Lease(Option<File>);
impl Lease {
    fn acquire(until: Instant) -> Result<Self, Error> {
        let uid = unsafe { libc::geteuid() };
        let path = input_sequence::linux_input_lock_path(uid);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(failed)?;
        let meta = file.metadata().map_err(failed)?;
        if !meta.is_file() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
            return Err(failed("Input lock owner or mode is invalid"));
        }
        loop {
            check(until)?;
            if worker::closing() {
                return Err(Error::Closed);
            }
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                if worker::closing() || worker::cleanup_failed() {
                    return Err(Error::Closed);
                }
                return Ok(Self(Some(file)));
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
                return Err(failed(error));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(file) = self.0.take() {
            if worker::cleanup_failed() {
                std::mem::forget(file);
                return;
            }
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } != 0 {
                worker::fail_cleanup();
                std::mem::forget(file);
            }
        }
    }
}
fn process_stamp(pid: u32) -> Result<AppInstance, Error> {
    let read = || {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map_err(failed)
            .and_then(|text| {
                text.rsplit_once(") ")
                    .and_then(|(_, tail)| tail.split_whitespace().nth(19))
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| failed("Unreadable process start time"))
            })
    };
    let generation = read()?;
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).map_err(failed)?;
    if read()? != generation {
        return Err(Error::TargetChanged);
    }
    Ok(AppInstance {
        identity: AppIdentity(path.to_string_lossy().into_owned()),
        pid,
        generation,
    })
}
impl Store {
    pub fn new() -> Result<Self, Error> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland")
        {
            return Err(Error::NativeFailure{code:"wayland_unsupported".into(),message:"The experimental provider requires a native X11 session; Wayland/XWayland is not claimed".into()});
        }
        if std::env::var_os("DISPLAY").is_none() {
            return Err(Error::NativeFailure {
                code: "display_unavailable".into(),
                message: "A local X11 DISPLAY is required".into(),
            });
        }
        let (conn, screen) = RustConnection::connect(None).map_err(failed)?;
        let root = conn.setup().roots[screen].root;
        let version = conn
            .res_query_version(1, 2)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if !xres_supports_client_ids(version.server_major, version.server_minor) {
            return Err(failed("XRes 1.2 is required for local process identity"));
        }
        let xi = conn
            .xinput_xi_query_version(2, 0)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if xi.major_version < 2 {
            return Err(failed(
                "XInput 2 is required to prove the complete pointer-button state",
            ));
        }
        let own = conn.setup().resource_id_base;
        let ids = conn
            .res_query_client_ids(&[ClientIdSpec {
                client: own,
                mask: ClientIdMask::LOCAL_CLIENT_PID,
            }])
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let values: Vec<_> = ids
            .ids
            .iter()
            .map(|id| XresClientValue {
                client: id.spec.client,
                mask: id.spec.mask.into(),
                value: &id.value,
            })
            .collect();
        if select_owner_pid(&values, own, std::process::id(), own) != Some(std::process::id()) {
            return Err(failed(
                "XRes cannot prove the DTW connection is local and in this PID namespace",
            ));
        }
        conn.xtest_get_version(2, 2)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        Ok(Self {
            conn,
            root,
            atoms: HashMap::new(),
            windows: HashMap::new(),
            pixels: HashMap::new(),
        })
    }
    fn atom(&mut self, name: &str) -> Result<u32, Error> {
        if let Some(value) = self.atoms.get(name) {
            return Ok(*value);
        }
        let value = self
            .conn
            .intern_atom(false, name.as_bytes())
            .map_err(failed)?
            .reply()
            .map_err(failed)?
            .atom;
        self.atoms.insert(name.into(), value);
        Ok(value)
    }
    fn property(&mut self, window: u32, name: &str) -> Result<GetPropertyReply, Error> {
        let atom = self.atom(name)?;
        let reply = self
            .conn
            .get_property(false, window, atom, AtomEnum::ANY, 0, 4096)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if reply.bytes_after != 0 {
            return Err(failed("X11 property exceeds bounded inventory"));
        }
        Ok(reply)
    }
    fn owner(&mut self, window: u32) -> Result<u32, Error> {
        let machine = self.property(window, "WM_CLIENT_MACHINE")?;
        let machine = String::from_utf8_lossy(&machine.value);
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname").map_err(failed)?;
        if !client_machine_is_local(Some(&machine), Some(&host)) {
            return Err(failed("Remote X11 client is unsupported"));
        }
        let setup = self.conn.setup();
        let own = setup.resource_id_base;
        let target = client_base(window, setup.resource_id_mask);
        let reply = self
            .conn
            .res_query_client_ids(&[
                ClientIdSpec {
                    client: own,
                    mask: ClientIdMask::LOCAL_CLIENT_PID,
                },
                ClientIdSpec {
                    client: window,
                    mask: ClientIdMask::LOCAL_CLIENT_PID,
                },
            ])
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let values: Vec<_> = reply
            .ids
            .iter()
            .map(|id| XresClientValue {
                client: id.spec.client,
                mask: id.spec.mask.into(),
                value: &id.value,
            })
            .collect();
        select_owner_pid(&values, own, std::process::id(), target)
            .ok_or_else(|| failed("XRes cannot prove a unique local process owner"))
    }
    fn marker(&mut self, window: u32, create: bool) -> Result<u64, Error> {
        let old = self.property(window, "_DTW_WINDOW_INCARNATION_V1")?;
        if let Some(mut values) = old.value32() {
            let values: Vec<_> = values.by_ref().collect();
            if values.len() == 2 {
                return Ok(values[0] as u64 | ((values[1] as u64) << 32));
            }
        }
        if !create {
            return Err(Error::TargetChanged);
        }
        let value = nonce();
        let atom = self.atom("_DTW_WINDOW_INCARNATION_V1")?;
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                atom,
                AtomEnum::CARDINAL,
                &[value as u32, (value >> 32) as u32],
            )
            .map_err(failed)?
            .check()
            .map_err(failed)?;
        Ok(value)
    }
    fn clients(&mut self) -> Result<Vec<u32>, Error> {
        let reply = self.property(self.root, "_NET_CLIENT_LIST")?;
        let windows = reply
            .value32()
            .ok_or_else(|| failed("An EWMH window manager is required"))?
            .collect::<Vec<_>>();
        if windows.len() > 256 {
            return Err(failed("X11 client list exceeds 256 windows"));
        }
        Ok(windows)
    }
    fn validate(&mut self, target: &WindowTarget) -> Result<u32, Error> {
        let window = u32::try_from(target.native_id).map_err(failed)?;
        if self.windows.get(&target.native_id) != Some(target)
            || self.owner(window)? != target.app.pid
            || process_stamp(target.app.pid)? != target.app
            || self.marker(window, false)? != target.generation
            || !self.clients()?.contains(&window)
        {
            return Err(Error::TargetChanged);
        }
        Ok(window)
    }
    fn bounds(&self, window: u32) -> Result<Bounds, Error> {
        let geometry = self
            .conn
            .get_geometry(window)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let point = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if !point.same_screen {
            return Err(Error::UnsupportedCapability);
        }
        Ok(Bounds {
            x: point.dst_x,
            y: point.dst_y,
            w: geometry.width,
            h: geometry.height,
        })
    }
    fn frame(&self, window: u32) -> Result<u32, Error> {
        let mut current = window;
        for _ in 0..32 {
            let tree = self
                .conn
                .query_tree(current)
                .map_err(failed)?
                .reply()
                .map_err(failed)?;
            if tree.parent == self.root {
                return Ok(current);
            }
            if tree.parent == 0 {
                return Err(Error::TargetChanged);
            }
            current = tree.parent;
        }
        Err(Error::TargetChanged)
    }
    fn unobstructed(&self, window: u32) -> Result<Bounds, Error> {
        let attrs = self
            .conn
            .get_window_attributes(window)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if attrs.map_state != MapState::VIEWABLE {
            return Err(failed("Window is not viewable"));
        }
        let bounds = self.bounds(window)?;
        let root = self
            .conn
            .get_geometry(self.root)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if bounds.w == 0
            || bounds.h == 0
            || bounds.w > 8192
            || bounds.h > 8192
            || bounds.w as u64 * bounds.h as u64 > 16_777_216
            || bounds.x < 0
            || bounds.y < 0
            || bounds.x as u32 + bounds.w as u32 > root.width as u32
            || bounds.y as u32 + bounds.h as u32 > root.height as u32
        {
            return Err(failed("Window capture is outside supported pixel bounds"));
        }
        let frame = self.frame(window)?;
        let tree = self
            .conn
            .query_tree(self.root)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if tree.children.len() > 4096 {
            return Err(failed("Root stack exceeds scan budget"));
        }
        for other in tree.children.iter().rev() {
            if *other == frame {
                return Ok(bounds);
            }
            let attrs = self
                .conn
                .get_window_attributes(*other)
                .map_err(failed)?
                .reply()
                .map_err(failed)?;
            if attrs.map_state != MapState::VIEWABLE || attrs.class == WindowClass::INPUT_ONLY {
                continue;
            }
            let b = self.bounds(*other)?;
            if (bounds.x as i32) < (b.x as i32 + b.w as i32)
                && bounds.x as i32 + bounds.w as i32 > b.x as i32
                && (bounds.y as i32) < (b.y as i32 + b.h as i32)
                && bounds.y as i32 + bounds.h as i32 > b.y as i32
            {
                return Err(failed(
                    "Window is occluded; XGetImage pixels are not proven",
                ));
            }
        }
        Err(Error::TargetChanged)
    }
    pub fn dispatch(&mut self, r: &Value) -> Value {
        self.pixels.retain(|_, p| p.expires > Instant::now());
        let op = r["operation"].as_str().unwrap_or("");
        let action = matches!(op, "click" | "focus" | "typeText" | "keypress" | "scroll");
        if op == "release" {
            return json!({"released":self.pixels.remove(r["binding"].as_str().unwrap_or("")).is_some()});
        }
        let result = (|| {
            let until = deadline(r)?;
            match op {
                "catalog" => {
                    let mut apps = HashMap::new();
                    for window in self.clients()? {
                        check(until)?;
                        if let Ok(pid) = self.owner(window) {
                            if let Ok(app) = process_stamp(pid) {
                                let name =
                                    app.identity.0.rsplit('/').next().unwrap_or(&app.identity.0);
                                apps.insert(pid,json!({"app_id":app.identity.0,"app":app_json(&app),"name":name}));
                            }
                        }
                    }
                    Ok(json!({"apps":apps.into_values().collect::<Vec<_>>()}))
                }
                "windows" => {
                    let app = parse_app(&r["app"])?;
                    if process_stamp(app.pid)? != app {
                        return Err(Error::TargetChanged);
                    }
                    // Only a proven foreign PID excludes a candidate. An unreadable
                    // owner, including a newly seen window, cannot prove absence.
                    let clients = self.clients()?;
                    let owned = inventory::owned_windows(clients, app.pid, |window| {
                        check(until)?;
                        self.owner(window)
                    })?;
                    let mut out = Vec::new();
                    for window in owned {
                        check(until)?;
                        let target = WindowTarget {
                            app: app.clone(),
                            native_id: window as u64,
                            generation: self.marker(window, true)?,
                        };
                        let mut title = self.property(window, "_NET_WM_NAME")?;
                        if title.value.is_empty() {
                            title = self.property(window, "WM_NAME")?;
                        }
                        let attrs = self
                            .conn
                            .get_window_attributes(window)
                            .map_err(failed)?
                            .reply()
                            .map_err(failed)?;
                        self.windows.insert(window as u64, target.clone());
                        out.push(json!({"target":target_json(&target),"title":String::from_utf8_lossy(&title.value).chars().take(1024).collect::<String>(),"visible":attrs.map_state==MapState::VIEWABLE}));
                    }
                    if self.windows.len() > 512 {
                        return Err(failed("Retained window identity budget exhausted"));
                    }
                    Ok(json!({"windows":out}))
                }
                "capture" => self.capture(&parse_target(&r["target"])?, until),
                _ if action => Ok(self.action(r, until)),
                _ => Err(Error::UnsupportedCapability),
            }
        })();
        result.unwrap_or_else(|error| error_reply(error, action))
    }
    fn capture(&mut self, target: &WindowTarget, until: Instant) -> Result<Value, Error> {
        let window = self.validate(target)?;
        let bounds = self.unobstructed(window)?;
        check(until)?;
        let image = self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                window,
                0,
                0,
                bounds.w,
                bounds.h,
                !0u32,
            )
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let setup = self.conn.setup();
        let format = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == image.depth)
            .ok_or(Error::UnsupportedCapability)?;
        if format.bits_per_pixel != 32 || setup.image_byte_order != ImageOrder::LSB_FIRST {
            return Err(Error::UnsupportedCapability);
        }
        let visual = setup
            .roots
            .iter()
            .flat_map(|s| &s.allowed_depths)
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual_id == image.visual)
            .ok_or(Error::UnsupportedCapability)?;
        if visual.class != VisualClass::TRUE_COLOR
            || visual.red_mask != 0xff0000
            || visual.green_mask != 0xff00
            || visual.blue_mask != 0xff
            || image.data.len() != bounds.w as usize * bounds.h as usize * 4
        {
            return Err(Error::UnsupportedCapability);
        }
        let mut rgb = Vec::with_capacity(bounds.w as usize * bounds.h as usize * 3);
        for p in image.data.chunks_exact(4) {
            rgb.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        let png = encode_png(bounds.w as u32, bounds.h as u32, &rgb)?;
        self.validate(target)?;
        if self.unobstructed(window)? != bounds {
            return Err(Error::TargetChanged);
        }
        self.pixels.retain(|_, p| p.target != *target);
        if self.pixels.len() >= 32 {
            return Err(failed("Pixel binding limit"));
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.pixels.insert(
            token.clone(),
            Pixels {
                target: target.clone(),
                bounds,
                expires: Instant::now() + Duration::from_secs(15),
            },
        );
        Ok(json!({"target":target_json(target),"binding":token,"elements":[],"image":png}))
    }
    fn focus(
        &mut self,
        target: &WindowTarget,
        window: u32,
        until: Instant,
        dispatched: &mut bool,
    ) -> Result<(), Error> {
        let active = self
            .property(self.root, "_NET_ACTIVE_WINDOW")?
            .value32()
            .and_then(|mut v| v.next());
        if active != Some(window) {
            let atom = self.atom("_NET_ACTIVE_WINDOW")?;
            let event = ClientMessageEvent::new(32, window, atom, [2, CURRENT_TIME, 0, 0, 0]);
            self.validate(target)?;
            let _mutation = worker::begin(until)?;
            *dispatched = true;
            self.conn
                .send_event(
                    false,
                    self.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                )
                .map_err(failed)?
                .check()
                .map_err(failed)?;
        }
        let limit = until.min(Instant::now() + Duration::from_millis(500));
        loop {
            let active = self
                .property(self.root, "_NET_ACTIVE_WINDOW")?
                .value32()
                .and_then(|mut v| v.next());
            if active == Some(window) && self.focus_owned(window)? {
                return Ok(());
            }
            check(limit)?;
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn focus_owned(&self, window: u32) -> Result<bool, Error> {
        let mut focus = self
            .conn
            .get_input_focus()
            .map_err(failed)?
            .reply()
            .map_err(failed)?
            .focus;
        for _ in 0..32 {
            if focus == window {
                return Ok(true);
            }
            if focus <= 1 {
                return Ok(false);
            }
            focus = self
                .conn
                .query_tree(focus)
                .map_err(failed)?
                .reply()
                .map_err(failed)?
                .parent;
        }
        Ok(false)
    }
    fn point(&mut self, window: u32, target: &WindowTarget, x: i16, y: i16) -> Result<(), Error> {
        let mut current = self.root;
        let mut saw = false;
        for _ in 0..64 {
            if current == window {
                saw = true;
            }
            let next = self
                .conn
                .translate_coordinates(self.root, current, x, y)
                .map_err(failed)?
                .reply()
                .map_err(failed)?
                .child;
            if next == 0 {
                return if saw && self.owner(current)? == target.app.pid {
                    Ok(())
                } else {
                    Err(Error::TargetChanged)
                };
            }
            current = next;
        }
        Err(Error::TargetChanged)
    }
    fn keyboard(
        &self,
        text: Option<&str>,
        keys: Option<&[String]>,
    ) -> Result<Vec<Vec<InputEvent>>, Error> {
        let setup = self.conn.setup();
        let min = setup.min_keycode;
        let count = setup.max_keycode - min + 1;
        let map = self
            .conn
            .get_keyboard_mapping(min, count)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let per = map.keysyms_per_keycode as usize;
        if per == 0 || map.keysyms.len() != per * count as usize {
            return Err(failed("Malformed X11 keyboard mapping"));
        }
        let lookup = |sym: u32| -> Result<(u8, bool), Error> {
            for (row, values) in map.keysyms.chunks(per).enumerate() {
                for (i, value) in values.iter().take(2).enumerate() {
                    if *value == sym {
                        return Ok((min + row as u8, i == 1));
                    }
                }
            }
            Err(Error::NativeFailure {
                code: "keyboard_mapping_unsupported".into(),
                message: format!(
                    "Keysym {sym:#x} is not in the current X11 mapping; no input was sent"
                ),
            })
        };
        let mut segments = Vec::new();
        let shift = lookup(0xffe1)?.0;
        if let Some(text) = text {
            if !input_sequence::valid_text(text, false) {
                return Err(failed("Text exceeds the UTF-8/encoded input contract"));
            }
            for c in input_sequence::normalized_text(text).chars() {
                let mut events = Vec::new();
                let sym = match c {
                    '\n' | '\r' => 0xff0d,
                    '\t' => 0xff09,
                    _ if (c as u32) <= 255 => c as u32,
                    _ => 0x01000000 | c as u32,
                };
                let (code, shifted) = lookup(sym)?;
                if shifted {
                    events.push((KEY_PRESS_EVENT, shift, 0, 0));
                }
                events.push((KEY_PRESS_EVENT, code, 0, 0));
                events.push((KEY_RELEASE_EVENT, code, 0, 0));
                if shifted {
                    events.push((KEY_RELEASE_EVENT, shift, 0, 0));
                }
                segments.push(events);
            }
        }
        if let Some(keys) = keys {
            let mut events = Vec::new();
            if keys.is_empty() || keys.len() > 16 {
                return Err(failed("Chord needs 1..16 keys"));
            }
            let mut codes = Vec::new();
            for name in keys {
                let sym = keysym(name).ok_or(Error::UnsupportedCapability)?;
                let (code, shifted) = lookup(sym)?;
                if shifted || codes.contains(&code) {
                    return Err(Error::UnsupportedCapability);
                }
                codes.push(code);
            }
            for code in &codes {
                events.push((KEY_PRESS_EVENT, *code, 0, 0));
            }
            for code in codes.iter().rev() {
                events.push((KEY_RELEASE_EVENT, *code, 0, 0));
            }
            segments.push(events);
        }
        Ok(segments)
    }
    fn idle_input(&self, text: bool) -> Result<(), Error> {
        let held = self
            .conn
            .query_keymap()
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let pointer = self
            .conn
            .xinput_xi_get_client_pointer(0)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if !pointer.set {
            return Err(failed("Client master pointer identity is unavailable"));
        }
        let state = self
            .conn
            .xinput_xi_query_pointer(self.root, pointer.deviceid)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if !input_sequence::linux_input_idle(&held.keys, &state.buttons) {
            return Err(Error::Busy);
        }
        if text
            && !input_sequence::linux_text_group_supported(
                state.mods.effective,
                state.group.effective,
            )
        {
            return Err(failed(
                "Active keyboard modifiers or group make text mapping unproven",
            ));
        }
        Ok(())
    }
    fn input_boundary(
        &mut self,
        target: &WindowTarget,
        window: u32,
        until: Instant,
    ) -> Result<(), Error> {
        check(until)?;
        if worker::closing() {
            return Err(Error::Closed);
        }
        self.validate(target)?;
        if !self.focus_owned(window)? {
            return Err(failed("Exact target focus changed before the next segment"));
        }
        Ok(())
    }
    // A checked XTest request establishes the accepted prefix. Connection loss
    // makes that request's state unknown: never resend its possibly accepted up.
    fn send_segment(
        &mut self,
        target: &WindowTarget,
        events: &[InputEvent],
        until: Instant,
        dispatched: &mut bool,
    ) -> Result<(), Error> {
        let window = self.validate(target)?;
        if !self.focus_owned(window)? {
            return Err(failed("Exact target focus changed before dispatch"));
        }
        let _mutation = worker::begin(until)?;
        let mut held = HeldInputs::default();
        for &(kind, detail, x, y) in events {
            *dispatched = true;
            let result = self
                .conn
                .xtest_fake_input(kind, detail, CURRENT_TIME, self.root, x, y, 0)
                .map_err(ReplyError::from)
                .and_then(|cookie| cookie.check());
            match result {
                Ok(()) => match kind {
                    KEY_PRESS_EVENT => held.pressed(HeldInput::Key(detail)),
                    KEY_RELEASE_EVENT => held.released(HeldInput::Key(detail)),
                    BUTTON_PRESS_EVENT => held.pressed(HeldInput::Button(detail)),
                    BUTTON_RELEASE_EVENT => held.released(HeldInput::Button(detail)),
                    _ => (),
                },
                Err(error) => {
                    match held.after_failed_event(
                        matches!(error, ReplyError::ConnectionError(_)),
                        matches!(kind, KEY_PRESS_EVENT | BUTTON_PRESS_EVENT),
                    ) {
                        Cleanup::Unconfirmed(may_be_held) => {
                            // A lost up acknowledgement is never replayed.
                            if may_be_held {
                                worker::fail_cleanup();
                            }
                        }
                        Cleanup::Releases(releases) => {
                            for input in releases {
                                let (up, code) = match input {
                                    HeldInput::Key(code) => (KEY_RELEASE_EVENT, code),
                                    HeldInput::Button(code) => (BUTTON_RELEASE_EVENT, code),
                                };
                                let release = self
                                    .conn
                                    .xtest_fake_input(up, code, CURRENT_TIME, self.root, 0, 0, 0)
                                    .map_err(ReplyError::from)
                                    .and_then(|cookie| cookie.check());
                                if release.is_err() {
                                    worker::fail_cleanup();
                                    break;
                                }
                                held.released(input);
                            }
                        }
                    }
                    return Err(failed(error));
                }
            }
        }
        if !held.is_empty() {
            worker::fail_cleanup();
            return Err(failed("Input segment did not close its held prefix"));
        }
        Ok(())
    }
    fn action(&mut self, r: &Value, until: Instant) -> Value {
        let mut dispatched = false;
        let result = (|| {
            let target = parse_target(&r["target"])?;
            let window = self.validate(&target)?;
            let op = string(r, "operation")?;
            let _lease = Lease::acquire(until)?;
            self.validate(&target)?;
            self.idle_input(op == "typeText")?;
            let mut segments: Vec<Vec<InputEvent>> = Vec::new();
            let mut point: Option<(Bounds, i16, i16, Option<Instant>)> = None;
            let mut scroll = None;
            match op {
                "typeText" => segments = self.keyboard(Some(string(r, "text")?), None)?,
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
                    segments = self.keyboard(None, Some(&keys))?;
                }
                "click" => {
                    let saved = self
                        .pixels
                        .remove(string(r, "binding")?)
                        .ok_or(Error::StaleObservation)?;
                    let bounds = self.unobstructed(window)?;
                    if saved.target != target
                        || saved.bounds != bounds
                        || saved.expires <= Instant::now()
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
                        || x >= bounds.w as f64
                        || y >= bounds.h as f64
                    {
                        return Err(Error::InvalidPoint);
                    }
                    let x = i16::try_from(bounds.x as i32 + x.floor() as i32)
                        .map_err(|_| Error::InvalidPoint)?;
                    let y = i16::try_from(bounds.y as i32 + y.floor() as i32)
                        .map_err(|_| Error::InvalidPoint)?;
                    point = Some((bounds, x, y, Some(saved.expires)));
                    let button = match r["button"].as_str() {
                        Some("left") => 1,
                        Some("right") => 3,
                        _ => return Err(Error::InvalidObservation),
                    };
                    let count = number(r, "count")?;
                    if !(1..=2).contains(&count) {
                        return Err(Error::InvalidObservation);
                    }
                    for _ in 0..count {
                        segments.push(vec![
                            (BUTTON_PRESS_EVENT, button, 0, 0),
                            (BUTTON_RELEASE_EVENT, button, 0, 0),
                        ]);
                    }
                }
                "scroll" => {
                    let x = r["delta_x"].as_i64().ok_or(Error::InvalidObservation)?;
                    let y = r["delta_y"].as_i64().ok_or(Error::InvalidObservation)?;
                    if x.unsigned_abs() > 100 || y.unsigned_abs() > 100 || (x == 0 && y == 0) {
                        return Err(failed("Scroll needs nonzero wheel steps within -100..100"));
                    }
                    scroll = Some((x, y));
                }
                "focus" => (),
                _ => return Err(Error::UnsupportedCapability),
            }
            self.pixels.retain(|_, p| p.target != target);
            self.focus(&target, window, until, &mut dispatched)?;
            self.input_boundary(&target, window, until)?;
            if let Some((x, y)) = scroll {
                let bounds = self.unobstructed(window)?;
                let hit_x = i16::try_from(bounds.x as i32 + bounds.w as i32 / 2)
                    .map_err(|_| Error::InvalidPoint)?;
                let hit_y = i16::try_from(bounds.y as i32 + bounds.h as i32 / 2)
                    .map_err(|_| Error::InvalidPoint)?;
                point = Some((bounds, hit_x, hit_y, None));
                for (delta, negative, positive) in [(x, 6, 7), (y, 4, 5)] {
                    for _ in 0..delta.unsigned_abs() {
                        let button = if delta < 0 { negative } else { positive };
                        segments.push(vec![
                            (BUTTON_PRESS_EVENT, button, 0, 0),
                            (BUTTON_RELEASE_EVENT, button, 0, 0),
                        ]);
                    }
                }
            }
            if let Some((_, x, y, _)) = point {
                segments.insert(0, vec![(MOTION_NOTIFY_EVENT, 0, x, y)]);
            }
            let mut placed = false;
            input_sequence::run(&segments, |step| match step {
                Step::Verify => {
                    self.input_boundary(&target, window, until)?;
                    self.idle_input(op == "typeText")?;
                    if let Some((bounds, x, y, expiry)) = point {
                        if expiry.is_some_and(|expiry| expiry <= Instant::now())
                            || self.unobstructed(window)? != bounds
                        {
                            return Err(Error::StaleObservation);
                        }
                        self.point(window, &target, x, y)?;
                        if placed {
                            let actual = self
                                .conn
                                .query_pointer(self.root)
                                .map_err(failed)?
                                .reply()
                                .map_err(failed)?;
                            if !actual.same_screen || actual.root_x != x || actual.root_y != y {
                                return Err(failed("Pointer moved after placement"));
                            }
                        }
                    }
                    Ok(())
                }
                Step::Dispatch(segment) => {
                    self.send_segment(&target, segment, until, &mut dispatched)?;
                    if point.is_some() {
                        placed = true;
                    }
                    std::thread::sleep(Duration::from_millis(if point.is_some() { 25 } else { 3 }));
                    Ok(())
                }
            })
        })();
        let mut reply = match result {
            Ok(()) => json!({"dispatch":"dispatched","evidence":"unverified"}),
            Err(error) => {
                let mut value = error_reply(error, true);
                if dispatched {
                    value["dispatch"] = json!("unknown");
                }
                value
            }
        };
        if worker::cleanup_failed() {
            reply["shutdown_error"] = json!({"code":"input_cleanup_failed","message":"Paired input or transaction lease cleanup is unconfirmed"});
        }
        reply
    }
}
fn keysym(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    Some(match upper.as_str() {
        "CTRL" | "CONTROL" => 0xffe3,
        "SHIFT" => 0xffe1,
        "ALT" | "OPTION" => 0xffe9,
        "META" | "SUPER" | "WIN" => 0xffeb,
        "ENTER" | "RETURN" => 0xff0d,
        "TAB" => 0xff09,
        "ESC" | "ESCAPE" => 0xff1b,
        "SPACE" => 32,
        "BACKSPACE" => 0xff08,
        "DELETE" => 0xffff,
        "LEFT" => 0xff51,
        "UP" => 0xff52,
        "RIGHT" => 0xff53,
        "DOWN" => 0xff54,
        "HOME" => 0xff50,
        "END" => 0xff57,
        "PAGEUP" | "PAGE_UP" => 0xff55,
        "PAGEDOWN" | "PAGE_DOWN" => 0xff56,
        _ => {
            if upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric() {
                upper.to_ascii_lowercase().as_bytes()[0] as u32
            } else if let Some(n) = upper
                .strip_prefix('F')
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| (1..=24).contains(n))
            {
                0xffbd + n
            } else {
                return None;
            }
        }
    })
}
