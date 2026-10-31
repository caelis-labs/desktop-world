use super::{diagnostic, native, Target, MAX_PACKET};
use crate::{
    backends::process::{StopReason, Worker},
    computer::CallContext,
};
use serde_json::{json, Value};
use std::{
    io,
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Owner {
    worker: Worker,
    session: String,
    endpoint: Mutex<Option<Value>>,
}
impl Owner {
    fn spawn(session: &str) -> io::Result<Self> {
        let parent = native::process_stamp(std::process::id())
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
        let args = [
            "--dtw-cursor-worker".to_owned(),
            parent.pid.to_string(),
            parent.generation.to_string(),
        ];
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        Ok(Self {
            worker: Worker::spawn_cooperative(&std::env::current_exe()?, &refs)
                .map_err(io::Error::other)?,
            session: session.into(),
            endpoint: Mutex::new(None),
        })
    }
    fn command(
        &self,
        request: Value,
        poll: &dyn Fn() -> Result<Duration, StopReason>,
    ) -> io::Result<Value> {
        let response = self
            .worker
            .call(request.to_string().as_bytes(), poll)
            .map_err(io::Error::other)?;
        let value: Value = serde_json::from_slice(&response)?;
        if value["ok"] != true {
            return Err(io::Error::other("cursor command was rejected"));
        }
        Ok(value)
    }
    fn begin(
        &self,
        operation: u64,
        target: &Value,
        poll: &dyn Fn() -> Result<Duration, StopReason>,
    ) -> io::Result<Value> {
        let mut endpoint = self.endpoint.lock().unwrap();
        if endpoint.is_none() {
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            let response = self.command(
                json!({"command":"initialize","session":self.session,"token":token}),
                poll,
            )?;
            let identity = super::identity::Identity::parse(&response["identity"])
                .filter(|i| i.pid == self.worker.id() && i.live())
                .ok_or_else(|| io::Error::other("cursor bootstrap identity is unproven"))?;
            if response["session"] != self.session
                || response["port"]
                    .as_u64()
                    .is_none_or(|p| p == 0 || p > 65535)
            {
                return Err(io::Error::other("cursor bootstrap differs from its owner"));
            }
            *endpoint =
                Some(json!({"port":response["port"],"token":token,"identity":identity.json()}));
        }
        self.command(
            json!({"command":"begin","operation":operation,"target":target}),
            poll,
        )?;
        let mut result = endpoint.as_ref().unwrap().clone();
        result["operation"] = json!(operation);
        result["target"] = target.clone();
        Ok(result)
    }
    fn hide(&self, operation: u64) {
        let deadline = Instant::now() + Duration::from_millis(200);
        if let Err(error) = self.command(json!({"command":"hide","operation":operation}), &|| {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(StopReason::TimedOut)
            } else {
                Ok(left)
            }
        }) {
            diagnostic("hide", error);
        }
    }
}
#[derive(Default)]
struct State {
    operation: u64,
    owner: Option<Arc<Owner>>,
}
pub struct CursorSession {
    session: String,
    closed: AtomicBool,
    state: Mutex<State>,
}
impl Default for CursorSession {
    fn default() -> Self {
        Self {
            session: uuid::Uuid::new_v4().simple().to_string(),
            closed: AtomicBool::new(false),
            state: Mutex::new(State::default()),
        }
    }
}
impl CursorSession {
    pub(crate) fn begin(&self, target: Value, call: &CallContext<'_>) -> Option<Action> {
        // A presentation budget never extends the caller's input deadline.
        let until = Instant::now() + call.remaining().ok()?.min(Duration::from_millis(400));
        let poll = || {
            if self.closed.load(Ordering::Acquire) {
                return Err(StopReason::Cancelled);
            }
            let left = call
                .remaining()
                .map_err(|_| StopReason::Cancelled)?
                .min(until.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                Err(StopReason::TimedOut)
            } else {
                Ok(left)
            }
        };
        let mut state = loop {
            if poll().is_err() {
                return None;
            }
            match self.state.try_lock() {
                Ok(state) => break state,
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(_) => return None,
            }
        };
        state.operation = state.operation.checked_add(1)?;
        if let Some(owner) = state
            .owner
            .as_ref()
            .filter(|owner| owner.worker.is_stopped())
        {
            if let Err(error) = owner.worker.stop() {
                diagnostic("cleanup", error);
                // This process owns presentation only. A nonzero exit remains
                // diagnostic, but a reaped process cannot retain an HWND or
                // injected key. Recover only on this later independent action.
                if !owner.worker.is_reaped() {
                    return None;
                }
            }
            state.owner = None;
        }
        if poll().is_err() {
            return None;
        }
        if state.owner.is_none() {
            match Owner::spawn(&self.session) {
                Ok(owner) => state.owner = Some(Arc::new(owner)),
                Err(error) => {
                    diagnostic("startup", error);
                    return None;
                }
            }
        }
        let owner = state.owner.as_ref()?.clone();
        match owner.begin(state.operation, &target, &poll) {
            Ok(endpoint) => Some(Action {
                owner,
                endpoint,
                operation: state.operation,
                keep: false,
            }),
            Err(error) => {
                diagnostic("unavailable", error);
                let _ = owner.worker.stop();
                None
            }
        }
    }
    pub(crate) fn hide(&self, operation: u64) {
        if operation == 0 {
            return;
        }
        let owner = self.state.lock().unwrap().owner.clone();
        if let Some(owner) = owner {
            owner.hide(operation)
        }
    }
    pub fn close(&self) -> io::Result<()> {
        self.closed.store(true, Ordering::Release);
        let owner = self.state.lock().unwrap().owner.clone();
        owner.map_or(Ok(()), |owner| {
            owner.worker.stop().map_err(io::Error::other)
        })
    }
}
impl Drop for CursorSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
pub(crate) struct Action {
    owner: Arc<Owner>,
    endpoint: Value,
    pub operation: u64,
    keep: bool,
}
impl Action {
    pub fn endpoint(&self) -> Value {
        self.endpoint.clone()
    }
    pub fn keep(&mut self) {
        self.keep = true;
    }
}
impl Drop for Action {
    fn drop(&mut self) {
        if !self.keep {
            self.owner.hide(self.operation)
        }
    }
}

/// One emitter per validated pointer operation. UDP is bounded and nonblocking;
/// a missing helper or full socket buffer loses feedback, never input results.
pub(in crate::backends::windows) struct Emitter {
    socket: UdpSocket,
    token: String,
    operation: u64,
    sequence: u64,
    press: u64,
    target: Target,
}
impl Emitter {
    pub(in crate::backends::windows) fn new(
        value: &Value,
        window: &crate::computer::WindowTarget,
    ) -> Option<Self> {
        let target = Target::parse(&value["target"])?;
        if &target.window != window || !target.valid() {
            return None;
        }
        let identity = super::identity::Identity::parse(&value["identity"])?;
        if !identity.live() {
            return None;
        }
        let token = value["token"].as_str()?.to_owned();
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let port = u16::try_from(value["port"].as_u64()?).ok()?;
        if port == 0 {
            return None;
        }
        let socket = UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).ok()?;
        socket.set_nonblocking(true).ok()?;
        socket.connect((std::net::Ipv4Addr::LOCALHOST, port)).ok()?;
        Some(Self {
            socket,
            token,
            operation: value["operation"].as_u64()?,
            sequence: 0,
            press: 0,
            target,
        })
    }
    pub(in crate::backends::windows) fn posted(&mut self, x: i32, y: i32, down: bool) {
        if !self.target.contains(x, y) {
            return;
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            return;
        };
        self.sequence = sequence;
        if down {
            self.press = sequence
        }
        let value = json!({"token":self.token,"operation":self.operation,"sequence":sequence,"press":self.press,"x":x,"y":y});
        let bytes = value.to_string();
        if bytes.len() <= MAX_PACKET {
            let _ = self.socket.send(bytes.as_bytes());
        }
    }
}
