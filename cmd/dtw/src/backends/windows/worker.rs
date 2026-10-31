use super::{input, native};
use crate::backends::{native_client::failed, process};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_TIMEOUT},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                CreateEventW, GetProcessTimes, OpenEventW, OpenProcess, SetEvent,
                WaitForSingleObject, EVENT_MODIFY_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE,
            },
        },
    },
};
pub(super) struct ProcessHandle(pub(super) HANDLE);
unsafe impl Send for ProcessHandle {}
impl ProcessHandle {
    pub(super) fn alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }
}
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub(super) fn parent_pid() -> std::io::Result<u32> {
    unsafe {
        let snap =
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(std::io::Error::other)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut parent = None;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == std::process::id() {
                    parent = Some(entry.th32ParentProcessID);
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        parent.ok_or_else(|| std::io::Error::other("Cannot identify native parent"))
    }
}
pub fn parent_alive_check() -> std::io::Result<Box<dyn Fn() -> bool + Send>> {
    let pid = parent_pid()?;
    let handle = ProcessHandle(
        unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .map_err(std::io::Error::other)?,
    );
    let (mut birth, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetProcessTimes(handle.0, &mut birth, &mut exit, &mut kernel, &mut user) }
        .map_err(std::io::Error::other)?;
    let birth = ((birth.dwHighDateTime as u64) << 32) | birth.dwLowDateTime as u64;
    let own = native::process_stamp(std::process::id())
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    if birth > own.generation || !handle.alive() {
        return Err(std::io::Error::other(
            "Parent exited or PID was reused before watcher binding",
        ));
    }
    Ok(Box::new(move || handle.alive()))
}
pub(super) fn event_name(pid: u32) -> Vec<u16> {
    format!("Local\\DesktopWorld.NativeStop.{pid}\0")
        .encode_utf16()
        .collect()
}
pub fn request_stop(pid: u32) -> std::io::Result<()> {
    unsafe {
        let name = event_name(pid);
        let event = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr()))
            .map_err(std::io::Error::other)?;
        let result = SetEvent(event).map_err(std::io::Error::other);
        let _ = CloseHandle(event);
        result
    }
}
pub fn worker_entry() -> bool {
    if super::cursor::worker_entry() {
        return true;
    }
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--dtw-native-worker") {
        return false;
    }
    let expected_stop = Arc::new(AtomicBool::new(false));
    let protocol_fault = Arc::new(AtomicBool::new(false));
    let run = || -> Result<(), crate::computer::Error> {
        let expected = args
            .get(2)
            .and_then(|x| x.parse::<u32>().ok())
            .ok_or_else(|| failed("Missing parent PID"))?;
        let birth = args
            .get(3)
            .and_then(|x| x.parse::<u64>().ok())
            .ok_or_else(|| failed("Missing parent creation stamp"))?;
        if parent_pid().map_err(failed)? != expected
            || native::process_stamp(expected)?.generation != birth
        {
            return Err(failed("Parent identity changed"));
        }
        let alive = Arc::new(Mutex::new(parent_alive_check().map_err(failed)?));
        let name = event_name(std::process::id());
        let event = ProcessHandle(
            unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }.map_err(failed)?,
        );
        let (watched, stopping, fault) =
            (alive.clone(), expected_stop.clone(), protocol_fault.clone());
        std::thread::spawn(move || loop {
            let parent_gone = !(watched.lock().unwrap())();
            let requested = parent_gone || !event.alive();
            if requested {
                stopping.store(true, Ordering::Release);
            }
            if requested || input::closing() {
                let clean = input::quiesce(Duration::from_millis(400));
                std::process::exit(exit_status(
                    clean,
                    stopping.load(Ordering::Acquire),
                    fault.load(Ordering::Acquire),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        });
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let (reading, stopping, fault) =
            (alive.clone(), expected_stop.clone(), protocol_fault.clone());
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(std::io::stdin());
            loop {
                let mut line = Vec::new();
                let result = reader
                    .by_ref()
                    .take((process::MAX_REQUEST_BYTES + 2) as u64)
                    .read_until(b'\n', &mut line);
                if matches!(result, Ok(0)) {
                    stopping.store(true, Ordering::Release);
                    input::close();
                    break;
                }
                if !matches!(result,Ok(n) if n>0)
                    || line.len() > process::MAX_REQUEST_BYTES + 1
                    || line.last() != Some(&b'\n')
                {
                    if !(reading.lock().unwrap())() {
                        stopping.store(true, Ordering::Release);
                    } else {
                        fault.store(true, Ordering::Release);
                    }
                    input::close();
                    break;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(v) => {
                        if send.send(v).is_err() {
                            input::close();
                            break;
                        }
                    }
                    Err(_) => {
                        fault.store(true, Ordering::Release);
                        input::close();
                        break;
                    }
                }
            }
        });
        let mut store = native::Store::new()?;
        for request in receive {
            let result = store.dispatch(&request);
            let reply = json!({"request_id":request["request_id"],"result":result});
            let bytes = reply.to_string();
            if bytes.len() > process::MAX_RESPONSE_BYTES {
                return Err(failed("Native response exceeds frame budget"));
            }
            let mut out = std::io::stdout().lock();
            let written = (|| -> std::io::Result<()> {
                out.write_all(bytes.as_bytes())?;
                out.write_all(b"\n")?;
                out.flush()
            })();
            if written.is_err() && !(alive.lock().unwrap())() {
                expected_stop.store(true, Ordering::Release);
            }
            written.map_err(failed)?;
        }
        Ok(())
    };
    if let Err(error) = run() {
        let _ = writeln!(std::io::stderr().lock(), "DTW Windows worker: {error:?}");
        input::close();
        let clean = input::quiesce(Duration::from_millis(400));
        std::process::exit(exit_status(
            clean,
            expected_stop.load(Ordering::Acquire),
            protocol_fault.load(Ordering::Acquire),
        ));
    }
    if !input::quiesce(Duration::from_millis(400)) {
        std::process::exit(process::INPUT_QUIESCE_FAILED_EXIT_CODE);
    }
    if protocol_fault.load(Ordering::Acquire) {
        std::process::exit(1);
    }
    true
}
fn exit_status(clean: bool, expected: bool, protocol_fault: bool) -> i32 {
    if !clean {
        process::INPUT_QUIESCE_FAILED_EXIT_CODE
    } else if expected && !protocol_fault {
        0
    } else {
        1
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_parent_loss_does_not_hide_protocol_or_cleanup_failure() {
        assert_eq!(exit_status(true, true, false), 0);
        assert_eq!(exit_status(true, false, false), 1);
        assert_eq!(exit_status(true, true, true), 1);
        assert_eq!(
            exit_status(false, true, false),
            process::INPUT_QUIESCE_FAILED_EXIT_CODE
        );
    }
}
