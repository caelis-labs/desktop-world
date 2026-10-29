use super::{input, native};
use crate::backends::{native_client::failed, process};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
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
struct ProcessHandle(HANDLE);
unsafe impl Send for ProcessHandle {}
impl ProcessHandle {
    fn alive(&self) -> bool {
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
fn parent_pid() -> std::io::Result<u32> {
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
fn event_name(pid: u32) -> Vec<u16> {
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
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--dtw-native-worker") {
        return false;
    }
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
        let alive = parent_alive_check().map_err(failed)?;
        let name = event_name(std::process::id());
        let event = ProcessHandle(
            unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }.map_err(failed)?,
        );
        std::thread::spawn(move || loop {
            if !alive() || !event.alive() || input::closing() {
                let clean = input::quiesce(Duration::from_millis(400));
                std::process::exit(if clean {
                    0
                } else {
                    process::INPUT_QUIESCE_FAILED_EXIT_CODE
                });
            }
            std::thread::sleep(Duration::from_millis(5));
        });
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(std::io::stdin());
            loop {
                let mut line = Vec::new();
                let result = reader
                    .by_ref()
                    .take((process::MAX_REQUEST_BYTES + 2) as u64)
                    .read_until(b'\n', &mut line);
                if !matches!(result,Ok(n) if n>0)
                    || line.len() > process::MAX_REQUEST_BYTES + 1
                    || line.last() != Some(&b'\n')
                {
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
            out.write_all(bytes.as_bytes()).map_err(failed)?;
            out.write_all(b"\n").map_err(failed)?;
            out.flush().map_err(failed)?;
        }
        Ok(())
    };
    if let Err(error) = run() {
        eprintln!("DTW Windows worker: {error:?}");
        input::close();
        let clean = input::quiesce(Duration::from_millis(400));
        std::process::exit(if clean {
            1
        } else {
            process::INPUT_QUIESCE_FAILED_EXIT_CODE
        });
    }
    if !input::quiesce(Duration::from_millis(400)) {
        std::process::exit(process::INPUT_QUIESCE_FAILED_EXIT_CODE);
    }
    true
}
