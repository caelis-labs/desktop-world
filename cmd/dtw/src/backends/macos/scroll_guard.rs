//! One read-only semantic admission check for an already bound scroll action.
//! The existing parent Worker poll services it; there is no extra input owner.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::CString,
    fs::{self, DirBuilder},
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 8_192;
const POLL: Duration = Duration::from_millis(5);

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Endpoint {
    path: PathBuf,
    token: String,
}
struct Channel {
    listener: Option<UnixListener>,
    stream: Option<UnixStream>,
    bytes: Vec<u8>,
    claimed: bool,
}
pub(super) struct Owner {
    directory: PathBuf,
    endpoint: Endpoint,
    closed: AtomicBool,
    channel: Mutex<Channel>,
}
pub(super) struct Request {
    owner: Arc<Owner>,
    stream: UnixStream,
}
impl Owner {
    pub fn new() -> io::Result<Arc<Self>> {
        let directory = PathBuf::from(format!(
            "/private/tmp/dtw-scroll-{}",
            uuid::Uuid::new_v4().simple()
        ));
        DirBuilder::new().mode(0o700).create(&directory)?;
        let path = directory.join("guard");
        let setup = (|| {
            let listener = UnixListener::bind(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            Ok(listener)
        })();
        let listener = match setup {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_dir(&directory);
                return Err(error);
            }
        };
        Ok(Arc::new(Self {
            directory,
            endpoint: Endpoint {
                path,
                token: uuid::Uuid::new_v4().to_string(),
            },
            closed: AtomicBool::new(false),
            channel: Mutex::new(Channel {
                listener: Some(listener),
                stream: None,
                bytes: vec![],
                claimed: false,
            }),
        }))
    }
    pub fn endpoint(&self) -> Value {
        json!(&self.endpoint)
    }
    pub fn invalidate(&self) {
        self.closed.store(true, Ordering::Release);
        let mut channel = self.channel.lock().unwrap();
        channel.listener = None;
        if let Some(stream) = &channel.stream {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
    /// Return an owned request and release the channel lock BEFORE the parent
    /// enters its resident Worker RPC. Cancellation can then invalidate freely.
    pub fn pending(self: &Arc<Self>) -> io::Result<Option<Request>> {
        if self.closed.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut channel = self.channel.lock().unwrap();
        if channel.claimed {
            return Ok(None);
        }
        if channel.stream.is_none() {
            let Some(listener) = &channel.listener else {
                return Ok(None);
            };
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true)?;
                    channel.stream = Some(stream);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        let mut bytes = [0; 512];
        loop {
            match channel.stream.as_mut().unwrap().read(&mut bytes) {
                Ok(0) => return Err(io::Error::other("scroll guard closed before admission")),
                Ok(count) => channel.bytes.extend_from_slice(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if channel.bytes.len() > MAX_FRAME {
                return Err(io::Error::other("oversized scroll guard request"));
            }
            if let Some(end) = channel.bytes.iter().position(|byte| *byte == b'\n') {
                if end + 1 != channel.bytes.len() {
                    return Err(io::Error::other("unexpected scroll guard frames"));
                }
                let request: Value =
                    serde_json::from_slice(&channel.bytes).map_err(io::Error::other)?;
                if request["token"] != self.endpoint.token || request["command"] != "check" {
                    return Err(io::Error::other("invalid scroll guard request"));
                }
                channel.claimed = true;
                let stream = channel.stream.as_ref().unwrap().try_clone()?;
                return Ok(Some(Request {
                    owner: self.clone(),
                    stream,
                }));
            }
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.invalidate();
        let _ = fs::remove_file(&self.endpoint.path);
        let _ = fs::remove_dir(&self.directory);
    }
}
impl Request {
    pub fn respond(mut self, result: Value) -> io::Result<()> {
        if self.owner.closed.load(Ordering::Acquire) {
            return Err(io::Error::other("scroll guard invalidated"));
        }
        let mut bytes = serde_json::to_vec(&result).map_err(io::Error::other)?;
        if bytes.len() >= MAX_FRAME {
            return Err(io::Error::other("oversized scroll guard response"));
        }
        bytes.push(b'\n');
        // One small response on a private channel. Backpressure is a failed
        // proof, never permission to wait beyond the original call budget.
        self.stream.write_all(&bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Awaiting,
    Checked,
    Posted,
}
struct Client {
    endpoint: Option<Endpoint>,
    deadline: Instant,
    stopping: fn() -> bool,
    phase: Phase,
    error: Option<Value>,
}
thread_local! { static ACTIVE: RefCell<Option<Client>> = const { RefCell::new(None) }; }
pub(super) struct Scope(Option<Client>);
impl Scope {
    pub fn new(request: &Value, remaining: Duration, stopping: fn() -> bool) -> Self {
        let client = request.get("_scroll_guard").map(|value| {
            let endpoint = serde_json::from_value(value.clone()).ok();
            Client {
                error: endpoint
                    .is_none()
                    .then(|| unavailable("invalid scroll anchor guard")),
                endpoint,
                deadline: Instant::now() + remaining,
                stopping,
                phase: Phase::Awaiting,
            }
        });
        Self(ACTIVE.with(|active| active.replace(client)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.replace(self.0.take());
        });
    }
}
fn unavailable(message: impl std::fmt::Display) -> Value {
    json!({"error":"scroll_anchor_unavailable", "message":message.to_string()})
}
impl Client {
    fn live(&self) -> Result<(), Value> {
        if (self.stopping)() {
            return Err(
                json!({"error":"native_stopping","message":"Scroll anchor admission closed"}),
            );
        }
        if Instant::now() >= self.deadline {
            return Err(
                json!({"error":"deadline_exceeded","message":"Scroll anchor admission expired"}),
            );
        }
        Ok(())
    }
    fn check(&mut self) -> Result<(), Value> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let result = self.live().and_then(|_| {
            if self.phase != Phase::Awaiting {
                Ok(())
            } else {
                self.exchange()
            }
        });
        match &result {
            Ok(()) if self.phase == Phase::Awaiting => self.phase = Phase::Checked,
            Ok(()) => (),
            Err(error) => self.error = Some(error.clone()),
        }
        result
    }
    fn exchange(&self) -> Result<(), Value> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| unavailable("missing scroll anchor endpoint"))?;
        let mut stream = super::local_socket::connect_peer(
            &endpoint.path,
            self.deadline
                .min(Instant::now() + Duration::from_millis(50)),
        )
        .map_err(unavailable)?;
        let mut request = serde_json::to_vec(&json!({"command":"check","token":endpoint.token}))
            .map_err(unavailable)?;
        request.push(b'\n');
        let mut sent = 0;
        let mut response = Vec::new();
        loop {
            self.live()?;
            if sent < request.len() {
                match stream.write(&request[sent..]) {
                    Ok(0) => return Err(unavailable("scroll guard write closed")),
                    Ok(count) => sent += count,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            || error.kind() == io::ErrorKind::Interrupted =>
                    {
                        ()
                    }
                    Err(error) => return Err(unavailable(error)),
                }
            } else {
                let mut bytes = [0; 1024];
                match stream.read(&mut bytes) {
                    Ok(0) => return Err(unavailable("scroll guard closed before proof")),
                    Ok(count) => response.extend_from_slice(&bytes[..count]),
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            || error.kind() == io::ErrorKind::Interrupted =>
                    {
                        ()
                    }
                    Err(error) => return Err(unavailable(error)),
                }
                if response.len() > MAX_FRAME {
                    return Err(unavailable("oversized scroll guard proof"));
                }
                if let Some(end) = response.iter().position(|byte| *byte == b'\n') {
                    if end + 1 != response.len() {
                        return Err(unavailable("unexpected scroll guard proofs"));
                    }
                    let reply: Value = serde_json::from_slice(&response).map_err(unavailable)?;
                    self.live()?;
                    return if reply["ok"] == true {
                        Ok(())
                    } else if reply["error"].is_string() {
                        Err(reply)
                    } else {
                        Err(unavailable("invalid scroll guard proof"))
                    };
                }
            }
            std::thread::sleep(POLL.min(self.deadline.saturating_duration_since(Instant::now())));
        }
    }
}
pub(super) fn check() -> Result<(), Value> {
    ACTIVE.with(|active| active.borrow_mut().as_mut().map_or(Ok(()), Client::check))
}
pub(super) fn posted() {
    ACTIVE.with(|active| {
        if let Some(client) = active.borrow_mut().as_mut() {
            if client.phase == Phase::Checked {
                client.phase = Phase::Posted;
            }
        }
    });
}
pub(super) fn failure() -> Option<Value> {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .and_then(|client| client.error.clone())
    })
}
#[no_mangle]
pub extern "C" fn dtw_scroll_anchor_check() -> *mut libc::c_char {
    match check() {
        Ok(()) => std::ptr::null_mut(),
        Err(error) => CString::new(error.to_string()).unwrap().into_raw(),
    }
}
#[no_mangle]
pub extern "C" fn dtw_scroll_anchor_posted() {
    posted();
}
#[no_mangle]
pub unsafe extern "C" fn dtw_scroll_anchor_free(value: *mut libc::c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn never_stopping() -> bool {
        false
    }
    fn wait_request(owner: &Arc<Owner>) -> Request {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(request) = owner.pending().unwrap() {
                return request;
            }
            assert!(Instant::now() < until, "guard request was not received");
            std::thread::sleep(POLL);
        }
    }
    fn phase() -> Phase {
        ACTIVE.with(|active| active.borrow().as_ref().unwrap().phase)
    }
    #[test]
    fn first_proof_is_not_a_wheel_and_later_content_movement_needs_no_ax_request() {
        let owner = Owner::new().unwrap();
        let request = json!({"_scroll_guard":owner.endpoint()});
        let child = std::thread::spawn(move || {
            let _scope = Scope::new(&request, Duration::from_secs(2), never_stopping);
            posted(); // A callback without a checked wheel cannot admit anything.
            assert_eq!(phase(), Phase::Awaiting);
            check().unwrap();
            assert_eq!(phase(), Phase::Checked); // ACK itself is not dispatch.
            posted();
            assert_eq!(phase(), Phase::Posted);
            check().unwrap(); // Content may now move/virtualize; no second AX read.
            assert_eq!(phase(), Phase::Posted);
        });
        wait_request(&owner).respond(json!({"ok":true})).unwrap();
        child.join().unwrap();
        assert!(owner.pending().unwrap().is_none());
    }
    #[test]
    fn replaced_object_before_first_wheel_preserves_the_original_error() {
        let owner = Owner::new().unwrap();
        let request = json!({"_scroll_guard":owner.endpoint()});
        let child = std::thread::spawn(move || {
            let _scope = Scope::new(&request, Duration::from_secs(2), never_stopping);
            let error = check().unwrap_err();
            assert_eq!(error["error"], "stale_observation");
            assert_eq!(failure(), Some(error.clone()));
            assert_eq!(check().unwrap_err(), error); // No proof or mutation replay.
            assert_eq!(phase(), Phase::Awaiting);
        });
        wait_request(&owner)
            .respond(json!({"error":"stale_observation","message":"Original AX object replaced"}))
            .unwrap();
        child.join().unwrap();
    }
    #[test]
    fn eof_or_cancel_invalidates_a_claimed_guard_and_late_ack_cannot_admit() {
        let owner = Owner::new().unwrap();
        let request = json!({"_scroll_guard":owner.endpoint()});
        let child = std::thread::spawn(move || {
            let _scope = Scope::new(&request, Duration::from_secs(2), never_stopping);
            assert_eq!(check().unwrap_err()["error"], "scroll_anchor_unavailable");
            assert_eq!(phase(), Phase::Awaiting);
        });
        let pending = wait_request(&owner);
        owner.invalidate(); // No channel mutex is retained across resident work.
        assert!(pending.respond(json!({"ok":true})).is_err());
        child.join().unwrap();
    }
    #[test]
    fn local_cancel_and_deadline_are_kept_as_the_first_failure() {
        for (left, stopping, expected) in [
            (
                Duration::ZERO,
                never_stopping as fn() -> bool,
                "deadline_exceeded",
            ),
            (
                Duration::from_secs(1),
                (|| true) as fn() -> bool,
                "native_stopping",
            ),
        ] {
            let owner = Owner::new().unwrap();
            let _scope = Scope::new(&json!({"_scroll_guard":owner.endpoint()}), left, stopping);
            assert_eq!(check().unwrap_err()["error"], expected);
            assert_eq!(failure().unwrap()["error"], expected);
            assert!(owner.pending().unwrap().is_none());
        }
    }
}
