//! An EnumWindows/class match is only a discovery hint. Exemption requires a
//! nonce response from a kernel-identified local pipe server, same executable
//! and user, exact immutable helper HWND, and live birth/owner checks.
use super::{native, CLASS};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    sync::{mpsc, OnceLock},
    time::{Duration, Instant},
};
use windows::{
    core::{HRESULT, PCWSTR},
    Win32::{
        Foundation::*,
        Security::*,
        Storage::FileSystem::*,
        System::{Pipes::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};
pub(super) struct Handle(pub HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn own_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        native::process_stamp(std::process::id())
            .ok()
            .map(|p| p.identity.0)
    })
    .as_deref()
}
fn sid(pid: u32) -> Option<Vec<u8>> {
    unsafe {
        let process = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut token = HANDLE::default();
        OpenProcessToken(process.0, TOKEN_QUERY, &mut token).ok()?;
        let token = Handle(token);
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        if needed == 0 || needed > 65536 {
            return None;
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .ok()?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        if !IsValidSid(user.User.Sid).as_bool() {
            return None;
        }
        let length = GetLengthSid(user.User.Sid) as usize;
        if length == 0 || length > 1024 {
            return None;
        }
        Some(std::slice::from_raw_parts(user.User.Sid.0.cast::<u8>(), length).to_vec())
    }
}
fn same_user(pid: u32) -> bool {
    static OWN: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    OWN.get_or_init(|| sid(std::process::id()))
        .as_ref()
        .is_some_and(|own| sid(pid).as_ref() == Some(own))
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Identity {
    pub pid: u32,
    pub birth: u64,
    pub hwnd: u64,
}
impl Identity {
    pub fn json(&self) -> Value {
        json!({"pid":self.pid,"birth":self.birth,"hwnd":self.hwnd})
    }
    pub fn parse(v: &Value) -> Option<Self> {
        let result = Self {
            pid: u32::try_from(v["pid"].as_u64()?).ok()?,
            birth: v["birth"].as_u64()?,
            hwnd: v["hwnd"].as_u64()?,
        };
        (result.pid > 0
            && result.birth > 0
            && result.hwnd > 0
            && usize::try_from(result.hwnd).is_ok())
        .then_some(result)
    }
    pub fn live(&self) -> bool {
        let Some(before) = native::process_stamp(self.pid).ok() else {
            return false;
        };
        if before.generation != self.birth
            || Some(before.identity.0.as_str()) != own_path()
            || !same_user(self.pid)
        {
            return false;
        }
        let h = HWND(self.hwnd as usize as *mut _);
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(h, Some(&mut pid));
        }
        let mut class = [0u16; 128];
        let n = unsafe { GetClassNameW(h, &mut class) };
        let flags = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
        let required = (WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0;
        pid == self.pid
            && n > 0
            && String::from_utf16_lossy(&class[..n as usize]) == CLASS
            && flags & required == required
            && flags & WS_EX_TOPMOST.0 == 0
            && unsafe { GetWindowLongPtrW(h, GWL_STYLE) } as u32 & WS_DISABLED.0 != 0
            && native::process_stamp(self.pid).ok().as_ref() == Some(&before)
    }
    fn pipe(&self) -> Vec<u16> {
        wide(&format!(
            r"\\.\pipe\DesktopWorld.CursorProof.{}.{}.{}",
            self.pid, self.birth, self.hwnd
        ))
    }
}
fn peer_process(pid: u32) -> bool {
    native::process_stamp(pid)
        .ok()
        .is_some_and(|p| Some(p.identity.0.as_str()) == own_path() && same_user(pid))
}
pub(super) fn serve(identity: Identity) -> bool {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || unsafe {
        let pipe = Handle(CreateNamedPipeW(
            PCWSTR(identity.pipe().as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            2048,
            2048,
            20,
            None,
        ));
        if pipe.0 == INVALID_HANDLE_VALUE {
            let _ = tx.send(false);
            return;
        }
        let _ = tx.send(true);
        loop {
            if ConnectNamedPipe(pipe.0, None).is_err()
                && windows::core::Error::from_win32().code()
                    != HRESULT::from_win32(ERROR_PIPE_CONNECTED.0)
            {
                break;
            }
            let mut pid = 0;
            let mut buffer = [0u8; 1024];
            let mut read = 0;
            let response = (|| -> Option<Vec<u8>> {
                GetNamedPipeClientProcessId(pipe.0, &mut pid).ok()?;
                if !peer_process(pid) {
                    return None;
                }
                ReadFile(pipe.0, Some(&mut buffer), Some(&mut read), None).ok()?;
                let v: Value = serde_json::from_slice(&buffer[..read as usize]).ok()?;
                let nonce = v["nonce"].as_str()?;
                if nonce.len() != 32
                    || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
                    || !identity.live()
                {
                    return None;
                }
                Some(
                    json!({"nonce":nonce,"identity":identity.json()})
                        .to_string()
                        .into_bytes(),
                )
            })();
            if let Some(response) = response {
                let mut written = 0;
                if WriteFile(pipe.0, Some(&response), Some(&mut written), None).is_ok()
                    && written as usize == response.len()
                {
                    // The client acknowledges after consuming the response;
                    // disconnecting earlier could discard its unread reply.
                    let _ = ReadFile(pipe.0, Some(&mut buffer[..1]), Some(&mut read), None);
                }
            }
            let _ = DisconnectNamedPipe(pipe.0);
        }
    });
    rx.recv_timeout(Duration::from_millis(80)).unwrap_or(false)
}
fn authenticate(identity: &Identity, until: Instant) -> bool {
    unsafe {
        if Instant::now() >= until || !identity.live() {
            return false;
        }
        let pipe = loop {
            match CreateFileW(
                PCWSTR(identity.pipe().as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            ) {
                Ok(pipe) => break pipe,
                Err(error)
                    if error.code() == HRESULT::from_win32(ERROR_PIPE_BUSY.0)
                        && Instant::now() < until =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return false,
            }
        };
        let pipe = Handle(pipe);
        let mut server = 0;
        if GetNamedPipeServerProcessId(pipe.0, &mut server).is_err() || server != identity.pid {
            return false;
        }
        // Nonblocking message mode applies to this client's reads and writes. No
        // synchronous IO against an unresponsive server may outlive the deadline.
        let mode = PIPE_READMODE_MESSAGE | PIPE_NOWAIT;
        if SetNamedPipeHandleState(pipe.0, Some(&mode), None, None).is_err() {
            return false;
        }
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let request = json!({"nonce":nonce}).to_string();
        let mut written = 0;
        if WriteFile(pipe.0, Some(request.as_bytes()), Some(&mut written), None).is_err()
            || written as usize != request.len()
        {
            return false;
        }
        let mut buffer = [0u8; 2048];
        let mut read = 0;
        while Instant::now() < until {
            match ReadFile(pipe.0, Some(&mut buffer), Some(&mut read), None) {
                Ok(()) if read > 0 => {
                    let _ = WriteFile(pipe.0, Some(&[1]), Some(&mut written), None);
                    let Ok(reply) = serde_json::from_slice::<Value>(&buffer[..read as usize])
                    else {
                        return false;
                    };
                    return reply["nonce"] == nonce
                        && Identity::parse(&reply["identity"]).as_ref() == Some(identity)
                        && identity.live();
                }
                Err(error) if error.code() != HRESULT::from_win32(ERROR_NO_DATA.0) => return false,
                _ => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        false
    }
}
thread_local! {static CACHE:RefCell<Vec<(Identity,Instant)>>=const{RefCell::new(Vec::new())};}
thread_local! {static PROOF_DEADLINE:RefCell<Option<Instant>>=const{RefCell::new(None)};}
pub(super) fn begin_cycle() {
    PROOF_DEADLINE
        .with(|deadline| *deadline.borrow_mut() = Some(Instant::now() + Duration::from_millis(40)));
}
pub(super) fn proven(h: HWND) -> bool {
    let mut class = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut class) };
    if n <= 0 || String::from_utf16_lossy(&class[..n as usize]) != CLASS {
        return false;
    }
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    let Some(birth) = native::process_stamp(pid).ok().map(|p| p.generation) else {
        return false;
    };
    let id = Identity {
        pid,
        birth,
        hwnd: h.0 as usize as u64,
    };
    let now = Instant::now();
    CACHE.with(|cache| {
        let mut entries = cache.borrow_mut();
        entries.retain(|(_, expiry)| *expiry > now);
        if entries.iter().any(|(cached, _)| cached == &id) {
            return id.live();
        }
        let deadline = PROOF_DEADLINE
            .with(|deadline| deadline.borrow().unwrap_or(now + Duration::from_millis(20)));
        if authenticate(&id, deadline.min(now + Duration::from_millis(20))) {
            entries.push((id, now + Duration::from_millis(100)));
            true
        } else {
            false
        }
    })
}
