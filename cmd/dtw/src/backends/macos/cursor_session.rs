//! Optional cursor presentation shared by every app in one standard session.
use super::{
    overlay::{OverlayEndpoint, OverlayIdentity, OverlayOwner},
    StopReason,
};
use crate::computer::CallContext;
use std::io::{self, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
struct State {
    operation: u64,
    owner: Option<Arc<OverlayOwner>>,
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
    pub(super) fn begin(&self, call: &CallContext<'_>) -> Option<OverlayEndpoint> {
        let poll = || {
            if self.closed.load(Ordering::Acquire) {
                return Err(StopReason::Cancelled);
            }
            call.remaining().map_err(|_| StopReason::Cancelled)
        };
        self.begin_with_poll(&poll, OverlayOwner::spawn_with_poll)
    }
    fn begin_with_poll(
        &self,
        poll: &dyn Fn() -> Result<std::time::Duration, StopReason>,
        spawn: impl FnOnce(
            &str,
            &dyn Fn() -> Result<std::time::Duration, StopReason>,
        ) -> io::Result<OverlayOwner>,
    ) -> Option<OverlayEndpoint> {
        let mut state = self.state.lock().unwrap();
        if self.closed.load(Ordering::Acquire) || poll().is_err() {
            return None;
        }
        // Never reuse an ordinal after cancellation, failed startup or a helper
        // replacement. An old provider's hide cannot match the next helper.
        state.operation = state.operation.checked_add(1)?;
        if let Some(owner) = state
            .owner
            .as_ref()
            .filter(|owner| owner.needs_replacement())
        {
            if let Err(error) = owner.close() {
                diagnostic("cleanup", &error);
                return None; // Retain ownership; unconfirmed teardown cannot recover.
            }
            state.owner = None;
        }
        // Cleanup may consume time, but never extends the original action budget.
        if self.closed.load(Ordering::Acquire) || poll().is_err() {
            return None;
        }
        if state.owner.is_none() {
            match spawn(&self.session, poll) {
                Ok(owner) => state.owner = Some(Arc::new(owner)),
                Err(error) => {
                    diagnostic("unavailable", &error);
                    return None;
                }
            }
        }
        let owner = state.owner.as_ref()?;
        match owner.begin_with_poll(state.operation, poll) {
            Ok(endpoint) => Some(endpoint),
            Err(error) => {
                diagnostic("unavailable", &error);
                None
            }
        }
    }
    pub(super) fn hide(&self, operation: u64) {
        if operation == 0 {
            return;
        }
        let owner = self.state.lock().unwrap().owner.clone();
        if let Some(owner) = owner {
            if let Err(error) = owner.hide(operation) {
                diagnostic("cleanup", &error);
            }
        }
    }
    pub(super) fn identity(&self) -> Option<OverlayIdentity> {
        let owner = self.state.lock().unwrap().owner.clone()?;
        owner.identity_candidate()
    }
    pub fn close(&self) -> std::io::Result<()> {
        self.closed.store(true, Ordering::Release);
        let owner = self.state.lock().unwrap().owner.clone();
        owner.map_or(Ok(()), |owner| owner.close())
    }
}
fn diagnostic(context: &str, error: &dyn std::fmt::Display) {
    write_diagnostic(&mut io::stderr().lock(), context, error);
}
fn write_diagnostic(writer: &mut impl Write, context: &str, error: &dyn std::fmt::Display) {
    let _ = writeln!(writer, "dtw cursor presentation {context}: {error}");
}
impl Drop for CursorSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, Instant},
    };

    fn log_path() -> PathBuf {
        PathBuf::from(format!(
            "/private/tmp/dtw-cursor-control-{}",
            uuid::Uuid::new_v4()
        ))
    }
    fn records(path: &std::path::Path) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn ready() -> Result<Duration, StopReason> {
        Ok(Duration::from_secs(3))
    }

    #[test]
    fn fixture_cold_start_is_separate_from_the_real_control_budget() {
        let session = CursorSession::default();
        let log = log_path();
        let endpoint = session.begin_with_poll(&ready, |id, _| {
            Ok(OverlayOwner::test_helper_with_startup_delay(
                id,
                "ready",
                &log,
                Duration::from_millis(2100), // Exceeds the real two-second startup exchange.
            ))
        });
        assert_eq!(endpoint.unwrap().operation, 1);
        assert_eq!(records(&log).len(), 2);
        session.close().unwrap();
        fs::remove_file(log).unwrap();
    }

    #[test]
    fn cancelled_or_expired_control_recovers_only_on_new_action_with_stable_identity() {
        for (mode, reason) in [
            ("startup_wait", StopReason::Cancelled),
            ("begin_wait", StopReason::Cancelled),
            ("startup_wait", StopReason::TimedOut),
            ("begin_wait", StopReason::TimedOut),
        ] {
            let session = CursorSession::default();
            let log = log_path();
            let cancelled = || {
                let sent = records(&log).iter().any(|record| {
                    if mode == "startup_wait" {
                        record.get("session").is_some()
                    } else {
                        record["command"] == "begin"
                    }
                });
                if sent {
                    Err(reason)
                } else {
                    ready()
                }
            };
            assert!(session
                .begin_with_poll(&cancelled, |id, _| {
                    Ok(OverlayOwner::test_helper(id, mode, &log))
                })
                .is_none());
            let old = session.state.lock().unwrap().owner.clone().unwrap();
            assert!(old.needs_replacement());
            let cancelled_records = records(&log);
            assert_eq!(
                cancelled_records.len(),
                if mode == "startup_wait" { 1 } else { 2 }
            );
            let old_instance = cancelled_records[0]["instance"].clone();
            let next = session
                .begin_with_poll(&ready, |id, _| {
                    assert_eq!(id, session.session);
                    Ok(OverlayOwner::test_helper(id, "ready", &log))
                })
                .unwrap();
            assert_eq!(next.operation, 2);
            let all = records(&log);
            let startup = &all[cancelled_records.len()];
            assert_eq!(startup["session"], session.session);
            assert_ne!(startup["instance"], old_instance);
            let new_path = PathBuf::from(startup["path"].as_str().unwrap());
            drop(old); // An old Arc's Drop cannot remove the replacement directory.
            assert!(new_path.parent().unwrap().exists());
            session.hide(1); // A late old-app cleanup addresses only the old ordinal.
            let current = session.state.lock().unwrap().owner.clone().unwrap();
            assert!(!current.needs_replacement());
            let third = session
                .begin_with_poll(&ready, |_, _| panic!("healthy owner replaced"))
                .unwrap();
            assert_eq!(third.operation, 3);
            let log_records = records(&log);
            assert_eq!(
                log_records
                    .iter()
                    .filter(|record| record.get("session").is_some())
                    .count(),
                2
            );
            assert_eq!(
                log_records
                    .iter()
                    .filter(|record| record["command"] == "begin" && record["operation"] == 1)
                    .count(),
                usize::from(mode == "begin_wait")
            );
            assert!(log_records
                .iter()
                .any(|record| record["command"] == "hide" && record["operation"] == 1));
            session.close().unwrap();
            fs::remove_file(log).unwrap();
        }
    }

    #[test]
    fn exited_helper_is_reaped_before_first_new_action_and_terminal_close_never_restarts() {
        let session = CursorSession::default();
        let log = log_path();
        assert_eq!(
            session
                .begin_with_poll(&ready, |id, _| Ok(OverlayOwner::test_helper(
                    id, "ready", &log
                )))
                .unwrap()
                .operation,
            1
        );
        let old = session.state.lock().unwrap().owner.clone().unwrap();
        let identity = old.identity_candidate().unwrap();
        assert_eq!(unsafe { libc::kill(identity.pid as i32, libc::SIGTERM) }, 0);
        let until = Instant::now() + Duration::from_secs(2);
        while !old.needs_replacement() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(old.needs_replacement()); // Includes an exited but unreaped zombie.
        let next = session
            .begin_with_poll(&ready, |id, _| {
                Ok(OverlayOwner::test_helper(id, "ready", &log))
            })
            .unwrap();
        assert_eq!(next.operation, 2);
        let replacement = session.identity().unwrap();
        assert_ne!(replacement.pid, identity.pid);
        let bootstraps: Vec<_> = records(&log)
            .into_iter()
            .filter(|record| record.get("session").is_some())
            .collect();
        assert_eq!(bootstraps[0]["session"], bootstraps[1]["session"]);
        assert_ne!(bootstraps[0]["instance"], bootstraps[1]["instance"]);
        session.close().unwrap();
        assert!(session
            .begin_with_poll(&ready, |_, _| panic!("closed session restarted"))
            .is_none());
        fs::remove_file(log).unwrap();
    }

    #[test]
    fn unconfirmed_cleanup_blocks_replacement_and_operation_ids_never_wrap() {
        let session = CursorSession::default();
        let log = log_path();
        session
            .begin_with_poll(&ready, |id, _| {
                Ok(OverlayOwner::test_helper(id, "unclean", &log))
            })
            .unwrap();
        let owner = session.state.lock().unwrap().owner.clone().unwrap();
        assert!(owner.close().is_err());
        assert!(session
            .begin_with_poll(&ready, |_, _| panic!("unconfirmed owner replaced"))
            .is_none());
        assert!(Arc::ptr_eq(
            &owner,
            session.state.lock().unwrap().owner.as_ref().unwrap()
        ));
        assert!(session.close().is_err());
        fs::remove_file(log).unwrap();
        let exhausted = CursorSession::default();
        exhausted.state.lock().unwrap().operation = u64::MAX;
        assert!(exhausted
            .begin_with_poll(&ready, |_, _| panic!("ordinal wrapped"))
            .is_none());
    }

    #[test]
    fn failed_diagnostic_write_is_best_effort() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
        write_diagnostic(&mut Closed, "unavailable", &"helper failed");
    }
}
