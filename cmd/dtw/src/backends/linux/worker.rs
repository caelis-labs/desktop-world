use super::native;
use crate::backends::input_sequence::{AdmissionError, Mutation, MutationGate};
use crate::{
    backends::{
        native_client::{error_reply, failed},
        process,
    },
    computer::Error,
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    time::{Duration, Instant},
};
static GATE: MutationGate = MutationGate::new();
pub fn close() {
    GATE.close();
}
pub fn fail_cleanup() {
    GATE.fail_cleanup();
}
pub fn cleanup_failed() -> bool {
    GATE.cleanup_failed()
}
pub fn closing() -> bool {
    GATE.closing()
}
pub fn begin(until: Instant) -> Result<Mutation<'static>, Error> {
    GATE.admit_before(until).map_err(|error| match error {
        AdmissionError::Expired => Error::TimedOut,
        AdmissionError::Closed => Error::Closed,
    })
}
fn quiesce() -> bool {
    GATE.quiesce(Duration::from_millis(400))
}
extern "C" fn terminate(_: libc::c_int) {
    close();
}
pub fn worker_entry() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--dtw-native-worker") {
        return false;
    }
    let run = || -> Result<(), Error> {
        let parent = args
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| failed("Missing parent PID"))?;
        if unsafe { libc::getppid() } as u32 != parent {
            return Err(failed("Worker parent changed before admission"));
        }
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = terminate as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) != 0 {
                return Err(failed(std::io::Error::last_os_error()));
            }
        }
        std::thread::spawn(move || loop {
            if unsafe { libc::getppid() } as u32 != parent || closing() {
                std::process::exit(if quiesce() {
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
                    close();
                    break;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(v) => {
                        if send.send(v).is_err() {
                            close();
                            break;
                        }
                    }
                    Err(_) => {
                        close();
                        break;
                    }
                }
            }
        });
        let mut store = native::Store::new();
        for request in receive {
            let result = match &mut store {
                Ok(store) => store.dispatch(&request),
                Err(error) => error_reply(error.clone(), false),
            };
            let bytes = json!({"request_id":request["request_id"],"result":result}).to_string();
            if bytes.len() > process::MAX_RESPONSE_BYTES {
                return Err(failed("Native frame exceeds response budget"));
            }
            let mut out = std::io::stdout().lock();
            out.write_all(bytes.as_bytes()).map_err(failed)?;
            out.write_all(b"\n").map_err(failed)?;
            out.flush().map_err(failed)?;
        }
        Ok(())
    };
    if let Err(error) = run() {
        eprintln!("DTW X11 worker: {error:?}");
        close();
        std::process::exit(if quiesce() {
            1
        } else {
            process::INPUT_QUIESCE_FAILED_EXIT_CODE
        });
    }
    if !quiesce() {
        std::process::exit(process::INPUT_QUIESCE_FAILED_EXIT_CODE);
    }
    true
}
