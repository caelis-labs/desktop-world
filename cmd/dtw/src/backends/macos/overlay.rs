//! DTW's optional, session-owned cursor presentation. No input is dispatched here.
//! See docs/cursor-overlay-port.md for the fixed CUA source slices and wiring.
use super::local_socket::{connect_peer, wait_socket};
use crate::backends::process::{StopReason, Worker};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, DirBuilder},
    io::{self, BufRead, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixDatagram, UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(not(test))]
use std::{
    ffi::{CStr, CString},
    io::BufReader,
};

const FRAME_LIMIT: usize = 8_192;
const CONTROL_BUDGET: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OverlayIdentity {
    pub pid: u32,
    pub birth: u64,
    pub windows: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OverlayEndpoint {
    path: PathBuf,
    token: String,
    pub operation: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OverlayTarget {
    pub pid: u32,
    pub birth: u64,
    pub window: u32,
    /// WindowServer desktop coordinates, top-left origin, in points.
    pub bounds: [f64; 4],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerKind {
    Move,
    Press,
    Release,
    Scroll,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerRoute {
    Background,
    Foreground,
}
#[derive(Serialize, Deserialize)]
struct PointerEvent {
    token: String,
    operation: u64,
    sequence: u64,
    press_sequence: u64,
    target: OverlayTarget,
    point: [f64; 2],
    route: PointerRoute,
    kind: PointerKind,
}

/// One emitter per admitted native operation. Call only AFTER actual pointer
/// posting, never for a speculative route, semantic AX action or primer.
/// Losing presentation cannot turn a posted action into an error/replay.
pub struct OverlayEmitter {
    endpoint: OverlayEndpoint,
    socket: UnixDatagram,
    sequence: u64,
    press_sequence: u64,
}
impl OverlayEmitter {
    pub fn connect(endpoint: OverlayEndpoint) -> io::Result<Self> {
        let socket = UnixDatagram::unbound()?;
        socket.connect(&endpoint.path)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            endpoint,
            socket,
            sequence: 0,
            press_sequence: 0,
        })
    }
    pub fn posted(
        &mut self,
        target: OverlayTarget,
        point: [f64; 2],
        route: PointerRoute,
        kind: PointerKind,
    ) -> io::Result<bool> {
        if !valid_position(&target, point) {
            return Err(invalid("invalid overlay target or point"));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("overlay sequence exhausted"))?;
        if matches!(kind, PointerKind::Press) {
            self.press_sequence = self.sequence;
        }
        let data = serde_json::to_vec(&PointerEvent {
            token: self.endpoint.token.clone(),
            operation: self.endpoint.operation,
            sequence: self.sequence,
            press_sequence: self.press_sequence,
            target,
            point,
            route,
            kind,
        })
        .map_err(invalid)?;
        match self.socket.send(&data) {
            Ok(size) => Ok(size == data.len()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(error),
        }
    }
}
fn valid_position(target: &OverlayTarget, point: [f64; 2]) -> bool {
    let [x, y, width, height] = target.bounds;
    target.pid > 0
        && target.pid <= i32::MAX as u32
        && target.birth > 0
        && target.window > 0
        && target
            .bounds
            .iter()
            .chain(point.iter())
            .all(|v| v.is_finite())
        && width > 0.0
        && height > 0.0
        && point[0] >= x
        && point[0] < x + width
        && point[1] >= y
        && point[1] < y + height
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}

const DISCOVERY_BUDGET: Duration = Duration::from_millis(40);
const PEER_BUDGET: Duration = Duration::from_millis(20);

fn registry_root() -> PathBuf {
    PathBuf::from(format!("/private/tmp/dtw-overlays-{}", unsafe {
        libc::geteuid()
    }))
}
fn private_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(invalid("overlay directory owner/mode mismatch"));
    }
    Ok(())
}
fn create_registry_root() -> io::Result<PathBuf> {
    let path = registry_root();
    match DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    private_directory(&path)?;
    Ok(path)
}
fn valid_session_name(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|value| value.is_ascii_hexdigit())
}

/// Directories are only locators. Every candidate requires a fresh socket peer
/// proof; stale files, a PID, title, level, basename or process arguments grant
/// nothing. The caller must still verify the exact WindowServer owner and birth.
/// Exhausted discovery simply omits candidates, never authorizes input/replay.
pub fn discover_candidates(deadline: Instant) -> Vec<OverlayIdentity> {
    let deadline = deadline.min(Instant::now() + DISCOVERY_BUDGET);
    if Instant::now() >= deadline {
        return vec![];
    }
    let Ok(executable) = std::env::current_exe().and_then(fs::canonicalize) else {
        return vec![];
    };
    discover_in(&registry_root(), &executable, deadline)
}
fn discover_in(root: &Path, executable: &Path, deadline: Instant) -> Vec<OverlayIdentity> {
    let mut candidates = Vec::new();
    if Instant::now() >= deadline || private_directory(root).is_err() {
        return candidates;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return candidates;
    };
    for entry in entries {
        if Instant::now() >= deadline {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_name().to_str().is_some_and(valid_session_name)
            || private_directory(&entry.path()).is_err()
        {
            continue;
        }
        let endpoint = entry.path().join("identity");
        let Ok(metadata) = fs::symlink_metadata(&endpoint) else {
            continue;
        };
        if !metadata.file_type().is_socket()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
        {
            continue;
        }
        let peer_deadline = deadline.min(Instant::now() + PEER_BUDGET);
        if let Ok(identity) = query_peer(&endpoint, executable, peer_deadline) {
            if !candidates.contains(&identity) {
                candidates.push(identity);
            }
        }
    }
    candidates
}

#[derive(Serialize, Deserialize)]
struct PeerChallenge {
    nonce: String,
}
#[derive(Serialize, Deserialize)]
struct PeerReply {
    nonce: String,
    session: String,
    identity: OverlayIdentity,
}
struct PeerProcessProof {
    pid: u32,
    birth: u64,
    executable: PathBuf,
}
fn prove_peer(
    reply: PeerReply,
    nonce: &str,
    before: &PeerProcessProof,
    after: &PeerProcessProof,
    expected_executable: &Path,
) -> io::Result<OverlayIdentity> {
    let identity = reply.identity;
    if reply.nonce != nonce
        || !valid_session_name(&reply.session)
        || before.pid == 0
        || before.pid != after.pid
        || before.birth == 0
        || before.birth != after.birth
        || before.executable != expected_executable
        || after.executable != expected_executable
        || identity.pid != before.pid
        || identity.birth != before.birth
        || identity.windows.len() != 1
        || identity.windows[0] == 0
    {
        return Err(invalid("overlay peer identity was not proven"));
    }
    Ok(identity)
}
fn process_birth(pid: u32) -> io::Result<u64> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(invalid("invalid overlay peer pid"));
    }
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if count != size
        || info.pbi_pid != pid
        || info.pbi_status == libc::SZOMB
        || info.pbi_start_tvusec >= 1_000_000
    {
        return Err(invalid("overlay peer birth unavailable"));
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(info.pbi_start_tvusec))
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("invalid overlay peer birth"))
}
fn process_executable(pid: u32) -> io::Result<PathBuf> {
    let mut bytes = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count =
        unsafe { libc::proc_pidpath(pid as i32, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if count <= 0 || count as usize >= bytes.len() {
        return Err(invalid("overlay peer executable unavailable"));
    }
    let length = bytes
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| invalid("invalid peer executable path"))?;
    fs::canonicalize(std::ffi::OsStr::from_bytes(&bytes[..length]))
}
fn peer_pid(stream: &UnixStream) -> io::Result<u32> {
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut size,
        )
    };
    let (mut uid, mut gid) = (0, 0);
    let owner = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if result != 0
        || size as usize != std::mem::size_of_val(&pid)
        || pid <= 0
        || owner != 0
        || uid != unsafe { libc::geteuid() }
    {
        return Err(invalid("overlay socket peer unavailable"));
    }
    Ok(pid as u32)
}
fn query_peer(
    endpoint: &Path,
    executable: &Path,
    deadline: Instant,
) -> io::Result<OverlayIdentity> {
    let mut stream = connect_peer(endpoint, deadline)?;
    let pid = peer_pid(&stream)?;
    let before = PeerProcessProof {
        pid,
        birth: process_birth(pid)?,
        executable: process_executable(pid)?,
    };
    if before.executable != executable {
        return Err(invalid("overlay peer executable mismatch"));
    }
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let mut request = serde_json::to_vec(&PeerChallenge {
        nonce: nonce.clone(),
    })
    .map_err(invalid)?;
    request.push(b'\n');
    write_until(&mut stream, &request, deadline)?;
    let reply = serde_json::from_slice(&read_until(&mut stream, deadline)?).map_err(invalid)?;
    // Executable reads are inside the two birth reads. Neither exec nor PID reuse
    // can substitute a different peer at either boundary of this proof.
    let after_executable = process_executable(pid)?;
    let after = PeerProcessProof {
        // The connected socket cannot retarget. After a valid reply the server
        // may close it; getpeereid need not remain available after that EOF.
        pid,
        birth: process_birth(pid)?,
        executable: after_executable,
    };
    if Instant::now() >= deadline {
        return Err(invalid("overlay discovery deadline reached"));
    }
    prove_peer(reply, &nonce, &before, &after, executable)
}
fn write_until(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(invalid("overlay peer deadline"));
        }
        match stream.write(bytes) {
            Ok(0) => return Err(invalid("overlay peer closed")),
            Ok(size) => bytes = &bytes[size..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_socket(stream, libc::POLLOUT, deadline)?
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn read_until(stream: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut frame = Vec::new();
    let mut bytes = [0u8; 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(invalid("overlay peer deadline"));
        }
        match stream.read(&mut bytes) {
            Ok(0) => return Err(invalid("overlay peer closed")),
            Ok(size) => {
                frame.extend_from_slice(&bytes[..size]);
                if frame.len() > FRAME_LIMIT {
                    return Err(invalid("overlay peer frame too large"));
                }
                if let Some(index) = frame.iter().position(|v| *v == b'\n') {
                    if index + 1 != frame.len() {
                        return Err(invalid("unexpected overlay peer frames"));
                    }
                    return Ok(frame);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_socket(stream, libc::POLLIN, deadline)?
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(not(test))]
static HELPER_IDENTITY: std::sync::OnceLock<OverlayIdentity> = std::sync::OnceLock::new();
#[cfg(not(test))]
static HELPER_SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
#[cfg(not(test))]
static TERMINATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(not(test))]
extern "C" fn request_termination(_: libc::c_int) {
    // Signal context only sets a lock-free flag; the watchdog owns AppKit calls.
    TERMINATING.store(true, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(not(test))]
fn install_termination_handler() -> io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = request_termination as *const () as usize;
    action.sa_flags = libc::SA_RESTART;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[cfg(not(test))]
extern "C" fn publish_identity(raw: *const libc::c_char) -> i32 {
    if raw.is_null() {
        return 0;
    }
    let Ok(reply) = serde_json::from_slice::<Value>(unsafe { CStr::from_ptr(raw).to_bytes() })
    else {
        return 0;
    };
    let Ok(identity) = serde_json::from_value::<OverlayIdentity>(reply["identity"].clone()) else {
        return 0;
    };
    if identity.pid != std::process::id()
        || process_birth(identity.pid).ok() != Some(identity.birth)
        || identity.windows.len() != 1
        || identity.windows[0] == 0
    {
        return 0;
    }
    if HELPER_IDENTITY.set(identity).is_ok() {
        1
    } else {
        0
    }
}
#[cfg(not(test))]
fn serve_peer(mut stream: UnixStream) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    let deadline = Instant::now() + PEER_BUDGET;
    let challenge: PeerChallenge =
        serde_json::from_slice(&read_until(&mut stream, deadline)?).map_err(invalid)?;
    if !valid_session_name(&challenge.nonce)
        || TERMINATING.load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err(invalid("overlay identity unavailable"));
    }
    let identity = HELPER_IDENTITY
        .get()
        .ok_or_else(|| invalid("overlay not initialized"))?;
    let mut bytes = serde_json::to_vec(&PeerReply {
        nonce: challenge.nonce,
        session: HELPER_SESSION
            .get()
            .ok_or_else(|| invalid("overlay session unavailable"))?
            .clone(),
        identity: identity.clone(),
    })
    .map_err(invalid)?;
    bytes.push(b'\n');
    write_until(&mut stream, &bytes, deadline)
}

fn remaining_control(
    deadline: Instant,
    poll: &dyn Fn() -> Result<Duration, StopReason>,
) -> Result<Duration, StopReason> {
    let left = poll()?.min(deadline.saturating_duration_since(Instant::now()));
    if left.is_zero() {
        Err(StopReason::TimedOut)
    } else {
        Ok(left)
    }
}

struct OwnerState {
    identity: Option<OverlayIdentity>,
    generation: u64,
    closed: bool,
}
/// Share one Arc<OverlayOwner> across the MCP session's application providers.
/// Existing Worker owns pipes, timeout retirement, process lifetime and reaping.
/// This owner adds visual state only, never a focus/physical-input lease.
pub struct OverlayOwner {
    session: String,
    instance: String,
    directory: PathBuf,
    token: String,
    worker: Worker,
    state: Mutex<OwnerState>,
}
impl OverlayOwner {
    #[cfg(test)]
    pub(super) fn test_helper(session: &str, mode: &str, log: &Path) -> Self {
        // A headless byte-protocol fixture exercises the real Worker lifecycle.
        // It creates no AppKit process/window and never dispatches input.
        let instance = uuid::Uuid::new_v4().simple().to_string();
        let directory = create_registry_root().unwrap().join(&instance);
        DirBuilder::new().mode(0o700).create(&directory).unwrap();
        let birth_path = directory.join("test-birth");
        let script = r#"
import json, os, signal, sys, time
mode, log, birth_path = sys.argv[1:]
signal.signal(signal.SIGTERM, lambda *_: sys.exit(74 if mode == 'unclean' else 0))
operation = 0
for line in sys.stdin:
    request = json.loads(line)
    if 'session' in request:
        with open(birth_path) as file: birth = int(file.read())
        os.unlink(birth_path)
    with open(log, 'a') as file: file.write(json.dumps(request) + '\n')
    if ('session' in request and mode == 'startup_wait') or (request.get('command') == 'begin' and mode == 'begin_wait'):
        time.sleep(60)
    if 'session' in request:
        result = {'ready': True, 'identity': {'pid': os.getpid(), 'birth': birth, 'windows': [42]}}
    else:
        if request['command'] == 'begin':
            assert request['operation'] > operation
            operation = request['operation']
        result = {'ok': True, 'operation': operation}
    print(json.dumps(result), flush=True)
"#;
        let worker = Worker::spawn(
            Path::new("/usr/bin/python3"),
            &[
                "-u",
                "-c",
                script,
                mode,
                log.to_str().unwrap(),
                birth_path.to_str().unwrap(),
            ],
        )
        .unwrap();
        fs::write(&birth_path, process_birth(worker.id()).unwrap().to_string()).unwrap();
        Self {
            session: session.to_owned(),
            instance,
            directory,
            token: uuid::Uuid::new_v4().to_string(),
            worker,
            state: Mutex::new(OwnerState {
                identity: None,
                generation: 0,
                closed: false,
            }),
        }
    }
    pub fn spawn_with_poll(
        session: &str,
        poll: &dyn Fn() -> Result<Duration, StopReason>,
    ) -> io::Result<Self> {
        let left =
            poll().map_err(|reason| invalid(format!("overlay startup stopped: {reason:?}")))?;
        if left.is_zero() {
            return Err(invalid("overlay startup deadline reached"));
        }
        if !valid_session_name(session) {
            return Err(invalid("invalid overlay session"));
        }
        let instance = uuid::Uuid::new_v4().simple().to_string();
        // A short per-uid root leaves room in macOS sockaddr_un for both endpoints.
        let directory = create_registry_root()?.join(&instance);
        DirBuilder::new().mode(0o700).create(&directory)?;
        let result = Self::spawn_in(session.to_owned(), instance, directory.clone());
        if result.is_err() {
            let _ = fs::remove_file(directory.join("events"));
            let _ = fs::remove_file(directory.join("identity"));
            let _ = fs::remove_dir(directory);
        }
        result
    }
    fn spawn_in(session: String, instance: String, directory: PathBuf) -> io::Result<Self> {
        let worker =
            Worker::spawn(&std::env::current_exe()?, &["--dtw-overlay-worker"]).map_err(invalid)?;
        Ok(Self {
            session,
            instance,
            directory,
            token: uuid::Uuid::new_v4().to_string(),
            worker,
            state: Mutex::new(OwnerState {
                identity: None,
                generation: 0,
                closed: false,
            }),
        })
    }
    /// One generation per native pointer action. Ack publishes the generation
    /// BEFORE its native worker may emit and hides the preceding marker.
    pub fn begin_with_poll(
        &self,
        operation: u64,
        poll: &dyn Fn() -> Result<Duration, StopReason>,
    ) -> io::Result<OverlayEndpoint> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err(invalid("overlay owner closed"));
        }
        if operation == 0 || operation <= state.generation {
            return Err(invalid("overlay generation is not newer"));
        }
        // Retain this owner in CursorSession before starting the handshake, so
        // cancellation cannot discard an unconfirmed helper cleanup.
        if state.identity.is_none() {
            let response = self.exchange_with_poll(
                &mut state,
                json!({"session":self.session, "instance":self.instance,
                    "path":self.directory.join("events"), "token":self.token}),
                Duration::from_secs(2),
                poll,
            )?;
            match self.parse_identity(&response) {
                Ok(identity) => state.identity = Some(identity),
                Err(error) => {
                    self.retire(&mut state);
                    return Err(error);
                }
            }
        }
        self.exchange_with_poll(
            &mut state,
            json!({"command":"begin","operation":operation}),
            CONTROL_BUDGET,
            poll,
        )?;
        state.generation = operation;
        Ok(OverlayEndpoint {
            path: self.directory.join("events"),
            token: self.token.clone(),
            operation,
        })
    }
    /// A cached identity is only a liveness hint here, never an occlusion proof.
    /// The next independent action may replace it only after close succeeds.
    pub fn needs_replacement(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.closed
            || state
                .identity
                .as_ref()
                .is_some_and(|identity| process_birth(identity.pid).ok() != Some(identity.birth))
    }
    /// Cancel only this action. A delayed event stays invalid; canceling an old
    /// action cannot hide a newer action. Server cancellation first stops new
    /// native admissions, then calls this with the saved action generation.
    pub fn hide(&self, operation: u64) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Ok(());
        }
        self.exchange(
            &mut state,
            json!({"command":"hide","operation":operation}),
            CONTROL_BUDGET,
        )?;
        Ok(())
    }
    /// No IO and no wait: the owning session may reuse its authenticated bootstrap
    /// identity. Missing/contended state yields no candidate. Scene must still
    /// verify live kernel birth and exact WindowServer ownership on every read.
    pub fn identity_candidate(&self) -> Option<OverlayIdentity> {
        let state = self.state.try_lock().ok()?;
        if state.closed {
            None
        } else {
            state.identity.clone()
        }
    }
    fn parse_identity(&self, response: &Value) -> io::Result<OverlayIdentity> {
        let identity: OverlayIdentity =
            serde_json::from_value(response["identity"].clone()).map_err(invalid)?;
        if identity.pid != self.worker.id()
            || identity.birth == 0
            || identity.windows.len() != 1
            || identity.windows[0] == 0
        {
            return Err(invalid("overlay initialization identity was not proven"));
        }
        Ok(identity)
    }
    fn retire(&self, state: &mut OwnerState) {
        state.closed = true;
        state.identity = None;
        let _ = self.worker.stop();
    }
    fn exchange(
        &self,
        state: &mut OwnerState,
        message: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        self.exchange_with_poll(state, message, timeout, &|| Ok(timeout))
    }
    fn exchange_with_poll(
        &self,
        state: &mut OwnerState,
        message: Value,
        timeout: Duration,
        poll: &dyn Fn() -> Result<Duration, StopReason>,
    ) -> io::Result<Value> {
        let deadline = Instant::now() + timeout;
        let result = (|| {
            let bytes = serde_json::to_vec(&message).map_err(invalid)?;
            let bytes = self
                .worker
                .call(&bytes, || remaining_control(deadline, poll))
                .map_err(invalid)?;
            let response: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
            let accepted = if message.get("session").is_some() {
                response["ready"] == true
            } else {
                response["ok"] == true
            };
            if !accepted {
                return Err(invalid("overlay rejected control command"));
            }
            Ok(response)
        })();
        if result.is_err() {
            // Worker retires a failed transport; also retire malformed/rejected
            // adapter replies. Never reuse an uncertain generation or late ack.
            self.retire(state);
        }
        result
    }
    /// Existing Worker's bounded stop owns termination and reaping. Any error
    /// here describes visual cleanup, never input dispatch/cleanup evidence.
    pub fn close(&self) -> io::Result<()> {
        let result = self.worker.stop().map_err(invalid);
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        state.identity = None;
        let _ = fs::remove_file(self.directory.join("events"));
        let _ = fs::remove_file(self.directory.join("identity"));
        let _ = fs::remove_dir(&self.directory);
        result
    }
}
impl Drop for OverlayOwner {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let size =
        std::io::Read::take(reader, (FRAME_LIMIT + 1) as u64).read_until(b'\n', &mut bytes)?;
    if size == 0 {
        return Ok(None);
    }
    if size > FRAME_LIMIT || bytes.last() != Some(&b'\n') {
        return Err(invalid("invalid overlay frame"));
    }
    Ok(Some(bytes))
}

#[cfg(not(test))]
#[derive(Deserialize)]
struct Bootstrap {
    session: String,
    instance: String,
    path: PathBuf,
    token: String,
}
#[cfg(not(test))]
unsafe extern "C" {
    fn dtw_overlay_run(
        session: *const libc::c_char,
        publish: extern "C" fn(*const libc::c_char) -> i32,
    ) -> i32;
    fn dtw_overlay_control(command: *const libc::c_char) -> *mut libc::c_char;
    fn dtw_overlay_event(event: *const libc::c_char);
    fn dtw_overlay_stop();
    fn dtw_overlay_free(value: *mut libc::c_char);
}
#[cfg(test)]
pub fn worker_entry() -> bool {
    false
}

/// Wire before the normal CLI/MCP entry. This path owns the AppKit main thread.
#[cfg(not(test))]
pub fn worker_entry() -> bool {
    if std::env::args().nth(1).as_deref() != Some("--dtw-overlay-worker") {
        return false;
    }
    let code = run_worker().unwrap_or_else(|error| {
        let _ = writeln!(io::stderr().lock(), "dtw overlay unavailable: {error}");
        1
    });
    std::process::exit(code)
}
#[cfg(not(test))]
fn run_worker() -> io::Result<i32> {
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return Err(invalid("overlay has no live owner"));
    }
    install_termination_handler()?;
    std::thread::spawn(move || loop {
        if TERMINATING.load(std::sync::atomic::Ordering::Relaxed)
            || unsafe { libc::getppid() } != parent
        {
            TERMINATING.store(true, std::sync::atomic::Ordering::Relaxed);
            unsafe { dtw_overlay_stop() };
            std::thread::sleep(CONTROL_BUDGET);
            // This helper owns presentation only. Process exit destroys its
            // windows even if AppKit did not process the orderly stop request.
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(10));
    });
    let mut reader = BufReader::new(io::stdin());
    let frame = read_frame(&mut reader)?.ok_or_else(|| invalid("overlay bootstrap missing"))?;
    let bootstrap: Bootstrap = serde_json::from_slice(&frame).map_err(invalid)?;
    if !valid_session_name(&bootstrap.session)
        || !valid_session_name(&bootstrap.instance)
        || bootstrap.token.len() != 36
    {
        return Err(invalid("invalid overlay bootstrap"));
    }
    let root = registry_root();
    let directory = root.join(&bootstrap.instance);
    private_directory(&root)?;
    private_directory(&directory)?;
    if bootstrap.path != directory.join("events") {
        return Err(invalid(
            "overlay endpoint is outside its instance directory",
        ));
    }
    HELPER_SESSION
        .set(bootstrap.session.clone())
        .map_err(|_| invalid("overlay session already initialized"))?;
    let listener = UnixListener::bind(directory.join("identity"))?;
    fs::set_permissions(
        directory.join("identity"),
        fs::Permissions::from_mode(0o600),
    )?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if TERMINATING.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { break };
            let _ = serve_peer(stream);
        }
    });
    let socket = UnixDatagram::bind(&bootstrap.path)?;
    fs::set_permissions(&bootstrap.path, fs::Permissions::from_mode(0o600))?;
    let token = bootstrap.token;
    std::thread::spawn(move || {
        let mut bytes = [0u8; FRAME_LIMIT + 1];
        while let Ok(size) = socket.recv(&mut bytes) {
            if size > FRAME_LIMIT {
                continue;
            }
            let Ok(event) = serde_json::from_slice::<PointerEvent>(&bytes[..size]) else {
                continue;
            };
            if event.token != token || !valid_position(&event.target, event.point) {
                continue;
            }
            if let Ok(input) = CString::new(&bytes[..size]) {
                unsafe { dtw_overlay_event(input.as_ptr()) };
            }
        }
    });
    std::thread::spawn(move || {
        while let Ok(Some(bytes)) = read_frame(&mut reader) {
            let Ok(input) = CString::new(bytes) else {
                break;
            };
            let output = unsafe { dtw_overlay_control(input.as_ptr()) };
            if output.is_null() {
                break;
            }
            let result = unsafe { CStr::from_ptr(output).to_bytes() }.to_vec();
            unsafe { dtw_overlay_free(output) };
            let mut stdout = io::stdout().lock();
            if stdout
                .write_all(&result)
                .and_then(|_| stdout.write_all(b"\n"))
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
        TERMINATING.store(true, std::sync::atomic::Ordering::Relaxed);
        unsafe { dtw_overlay_stop() };
    });
    let session = CString::new(bootstrap.session).map_err(invalid)?;
    Ok(unsafe { dtw_overlay_run(session.as_ptr(), publish_identity) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directed_points_preserve_negative_origins_and_reject_invalid_positions() {
        let target = OverlayTarget {
            pid: 1,
            birth: 2,
            window: 3,
            bounds: [-1920.0, -200.0, 1000.0, 800.0],
        };
        assert!(valid_position(&target, [-1500.0, 50.0]));
        assert!(!valid_position(&target, [-1.0, -1.0]));
        let crossing_origin = OverlayTarget {
            bounds: [-100.0, -100.0, 200.0, 200.0],
            ..target.clone()
        };
        assert!(valid_position(&crossing_origin, [-1.0, -1.0])); // Legitimate display coordinate, not a primer heuristic.
        assert!(!valid_position(&target, [f64::NAN, 0.0]));
        assert!(!valid_position(&target, [-920.0, 50.0]));
    }
    #[test]
    fn channel_keeps_press_edge_in_later_release_and_uses_no_real_input() {
        let path = PathBuf::from(format!(
            "/private/tmp/dtw-overlay-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let receiver = UnixDatagram::bind(&path).unwrap();
        let endpoint = OverlayEndpoint {
            path: path.clone(),
            token: "private".into(),
            operation: 3,
        };
        let mut sender = OverlayEmitter::connect(endpoint).unwrap();
        let target = OverlayTarget {
            pid: 1,
            birth: 2,
            window: 3,
            bounds: [0.0, 0.0, 100.0, 100.0],
        };
        sender
            .posted(
                target.clone(),
                [30.0, 40.0],
                PointerRoute::Background,
                PointerKind::Press,
            )
            .unwrap();
        sender
            .posted(
                target,
                [30.0, 40.0],
                PointerRoute::Background,
                PointerKind::Release,
            )
            .unwrap();
        let mut bytes = [0; FRAME_LIMIT];
        receiver.recv(&mut bytes).unwrap();
        let size = receiver.recv(&mut bytes).unwrap();
        let event: PointerEvent = serde_json::from_slice(&bytes[..size]).unwrap();
        assert_eq!(
            (event.operation, event.sequence, event.press_sequence),
            (3, 2, 1)
        );
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_partial_and_oversized_control_frames_stop() {
        assert!(read_frame(&mut &b"{}"[..]).is_err());
        assert!(read_frame(&mut &[b'a'; FRAME_LIMIT + 2][..]).is_err());
        assert!(read_frame(&mut &b""[..]).unwrap().is_none());
        assert_eq!(read_frame(&mut &b"{}\n"[..]).unwrap().unwrap(), b"{}\n");
    }
    #[test]
    fn peer_proof_rejects_replay_pid_reuse_executable_change_and_extra_ids() {
        let expected = Path::new("/trusted/Contents/MacOS/dtw");
        let proof = || PeerProcessProof {
            pid: 7,
            birth: 99,
            executable: expected.into(),
        };
        let reply = || PeerReply {
            nonce: "fresh".into(),
            session: "0123456789abcdef0123456789abcdef".into(),
            identity: OverlayIdentity {
                pid: 7,
                birth: 99,
                windows: vec![42],
            },
        };
        assert!(prove_peer(reply(), "fresh", &proof(), &proof(), expected).is_ok());
        assert!(prove_peer(reply(), "old", &proof(), &proof(), expected).is_err());
        let mut malformed_session = reply();
        malformed_session.session = "instance-is-not-a-logical-session".into();
        assert!(prove_peer(malformed_session, "fresh", &proof(), &proof(), expected).is_err());
        let mut after = proof();
        after.birth += 1;
        assert!(prove_peer(reply(), "fresh", &proof(), &after, expected).is_err());
        after = proof();
        after.executable = "/other/Contents/MacOS/dtw".into();
        assert!(prove_peer(reply(), "fresh", &proof(), &after, expected).is_err());
        assert!(prove_peer(reply(), "fresh", &after, &proof(), expected).is_err());
        let mut foreign = reply();
        foreign.identity.pid = 8;
        assert!(prove_peer(foreign, "fresh", &proof(), &proof(), expected).is_err());
        let mut foreign = reply();
        foreign.identity.windows.push(43);
        assert!(prove_peer(foreign, "fresh", &proof(), &proof(), expected).is_err());
        let mut missing = reply();
        missing.identity.windows.clear();
        assert!(prove_peer(missing, "fresh", &proof(), &proof(), expected).is_err());
    }

    #[test]
    fn kernel_peer_nonce_exchange_and_stale_files_are_distinct() {
        let root = PathBuf::from(format!(
            "/private/tmp/dtw-peer-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let directory = root.join(uuid::Uuid::new_v4().simple().to_string());
        DirBuilder::new().mode(0o700).create(&directory).unwrap();
        let endpoint = directory.join("identity");
        let listener = UnixListener::bind(&endpoint).unwrap();
        fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600)).unwrap();
        let pid = std::process::id();
        let identity = OverlayIdentity {
            pid,
            birth: process_birth(pid).unwrap(),
            windows: vec![42],
        };
        let response_identity = identity.clone();
        let serving = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            let challenge: PeerChallenge =
                serde_json::from_slice(&read_until(&mut stream, deadline).unwrap()).unwrap();
            let mut bytes = serde_json::to_vec(&PeerReply {
                nonce: challenge.nonce,
                session: "0123456789abcdef0123456789abcdef".into(),
                identity: response_identity,
            })
            .unwrap();
            bytes.push(b'\n');
            write_until(&mut stream, &bytes, deadline).unwrap();
        });
        let executable = std::env::current_exe().and_then(fs::canonicalize).unwrap();
        assert_eq!(process_executable(pid).unwrap(), executable);
        let found = discover_in(&root, &executable, Instant::now() + Duration::from_secs(1));
        serving.join().unwrap();
        assert_eq!(found, vec![identity.clone()]);
        // The now-stale socket and a forged regular identity file grant nothing.
        assert!(discover_in(&root, &executable, Instant::now() + PEER_BUDGET).is_empty());
        fs::remove_file(&endpoint).unwrap();
        fs::write(&endpoint, serde_json::to_vec(&identity).unwrap()).unwrap();
        fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(discover_in(&root, &executable, Instant::now() + PEER_BUDGET).is_empty());
        fs::remove_file(&endpoint).unwrap();
        fs::remove_dir(directory).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn peer_io_has_an_absolute_deadline_and_discovery_never_ignores_expiry() {
        let (mut client, _silent_peer) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        assert_eq!(peer_pid(&client).unwrap(), std::process::id());
        let start = Instant::now();
        assert!(read_until(&mut client, start + Duration::from_millis(5)).is_err());
        assert!(start.elapsed() < Duration::from_millis(250));
        assert!(discover_candidates(Instant::now()).is_empty());
    }

    #[test]
    fn caller_cancellation_precedes_visual_startup_and_control_budget() {
        let session = "0123456789abcdef0123456789abcdef";
        assert!(OverlayOwner::spawn_with_poll(session, &|| Err(StopReason::Cancelled)).is_err());
        assert!(OverlayOwner::spawn_with_poll(session, &|| Ok(Duration::ZERO)).is_err());
        assert_eq!(
            remaining_control(Instant::now(), &|| Err(StopReason::Cancelled)),
            Err(StopReason::Cancelled)
        );
        assert_eq!(
            remaining_control(Instant::now(), &|| Ok(Duration::from_secs(9))),
            Err(StopReason::TimedOut)
        );
        assert!(
            remaining_control(Instant::now() + Duration::from_secs(9), &|| Ok(
                Duration::from_millis(5)
            ))
            .unwrap()
                <= Duration::from_millis(5)
        );
    }
}
