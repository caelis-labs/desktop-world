//! Fresh resident execution-focus proof before each new child input payload.
//! Paired cleanup never queries this guard or reopens admission.
use super::scroll_guard::Endpoint;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::CString,
    time::{Duration, Instant},
};

struct Client {
    scoped: bool,
    endpoint: Option<Endpoint>,
    deadline: Instant,
    stopping: fn() -> bool,
    error: Option<Value>,
}
thread_local! { static ACTIVE: RefCell<Option<Client>> = const { RefCell::new(None) }; }
pub(super) struct Scope(Option<Client>);
impl Scope {
    pub fn new(request: &Value, remaining: Duration, stopping: fn() -> bool) -> Self {
        let raw = request.get("_execution_guard");
        let endpoint = raw.and_then(|value| serde_json::from_value(value.clone()).ok());
        let client = Client {
            scoped: request["_execution_foreground"] == true || raw.is_some(),
            error: (raw.is_some() && endpoint.is_none())
                .then(|| unavailable("Invalid execution foreground guard")),
            endpoint,
            deadline: Instant::now() + remaining,
            stopping,
        };
        Self(ACTIVE.with(|active| active.replace(Some(client))))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.replace(self.0.take());
        });
    }
}
fn unavailable(message: &str) -> Value {
    json!({"error":"execution_focus_unavailable", "message":message})
}
impl Client {
    fn check(&mut self) -> Result<(), Value> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let result = self
            .endpoint
            .as_ref()
            .ok_or_else(|| unavailable("Missing execution foreground guard"))
            .and_then(|endpoint| endpoint.check(self.deadline, self.stopping))
            .map_err(|mut error| {
                if error["error"] == "scroll_anchor_unavailable" {
                    error["error"] = json!("execution_focus_unavailable");
                }
                error
            });
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }
}
pub(super) fn active() -> bool {
    ACTIVE.with(|active| active.borrow().as_ref().is_some_and(|client| client.scoped))
}
pub(super) fn check() -> Result<(), Value> {
    ACTIVE.with(|active| {
        active.borrow_mut().as_mut().map_or_else(
            || Err(unavailable("No execution foreground guard is installed")),
            Client::check,
        )
    })
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
pub extern "C" fn dtw_execution_scope_active() -> bool {
    active()
}
#[no_mangle]
pub extern "C" fn dtw_execution_scope_check() -> *mut libc::c_char {
    match check() {
        Ok(()) => std::ptr::null_mut(),
        Err(error) => CString::new(error.to_string()).unwrap().into_raw(),
    }
}
#[no_mangle]
pub unsafe extern "C" fn dtw_execution_scope_free(value: *mut libc::c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests {
    use super::super::scroll_guard::{Owner, Request};
    use super::*;
    use std::sync::Arc;

    fn never_stopping() -> bool {
        false
    }
    fn wait_request(owner: &Arc<Owner>) -> Request {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(request) = owner.pending().unwrap() {
                return request;
            }
            assert!(
                Instant::now() < until,
                "execution guard request was not received"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn every_payload_gets_a_new_proof_and_denial_cannot_be_rearmed() {
        let owner = Owner::new().unwrap();
        let request = json!({"_execution_foreground":true,"_execution_guard":owner.endpoint()});
        let child = std::thread::spawn(move || {
            let _scope = Scope::new(&request, Duration::from_secs(2), never_stopping);
            assert!(active());
            assert!(dtw_execution_scope_check().is_null());
            check().unwrap();
            let error = check().unwrap_err();
            assert_eq!(error["error"], "focus_changed");
            assert_eq!(failure(), Some(error.clone()));
            assert_eq!(check().unwrap_err(), error);
            assert!(active());
        });
        wait_request(&owner).respond(json!({"ok":true})).unwrap();
        wait_request(&owner).respond(json!({"ok":true})).unwrap();
        wait_request(&owner)
            .respond(json!({"error":"focus_changed","message":"User left and returned"}))
            .unwrap();
        child.join().unwrap();
        assert!(owner.pending().unwrap().is_none());
    }
    #[test]
    fn missing_and_malformed_endpoints_never_disable_scoped_admission() {
        assert!(!active());
        assert_eq!(check().unwrap_err()["error"], "execution_focus_unavailable");
        for (request, expected_active) in [
            (json!({}), false),
            (json!({"_execution_foreground":true}), true),
            (json!({"_execution_guard":null}), true),
            (json!({"_execution_guard":{"path":9,"token":false}}), true),
        ] {
            let _scope = Scope::new(&request, Duration::from_secs(1), never_stopping);
            assert_eq!(dtw_execution_scope_active(), expected_active);
            let pointer = dtw_execution_scope_check();
            assert!(!pointer.is_null());
            let error: Value = unsafe {
                serde_json::from_slice(std::ffi::CStr::from_ptr(pointer).to_bytes()).unwrap()
            };
            unsafe { dtw_execution_scope_free(pointer) };
            assert_eq!(error["error"], "execution_focus_unavailable");
            assert_eq!(failure(), Some(error.clone()));
            assert_eq!(check().unwrap_err(), error);
        }
        assert!(!active());
        unsafe { dtw_execution_scope_free(std::ptr::null_mut()) };
    }
    #[test]
    fn cancellation_and_expiry_fail_before_querying_the_resident() {
        for (remaining, stopping, expected) in [
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
            let request = json!({"_execution_guard":owner.endpoint()});
            let _scope = Scope::new(&request, remaining, stopping);
            let error = check().unwrap_err();
            assert_eq!(error["error"], expected);
            assert_eq!(failure(), Some(error.clone()));
            assert_eq!(check().unwrap_err(), error);
            assert!(owner.pending().unwrap().is_none());
        }
    }
    #[test]
    fn invalidating_a_claimed_query_prevents_late_ack_and_later_queries() {
        let owner = Owner::new().unwrap();
        let request = json!({"_execution_guard":owner.endpoint()});
        let child = std::thread::spawn(move || {
            let _scope = Scope::new(&request, Duration::from_secs(2), never_stopping);
            let error = check().unwrap_err();
            assert_eq!(error["error"], "execution_focus_unavailable");
            assert_eq!(check().unwrap_err(), error);
        });
        let pending = wait_request(&owner);
        owner.invalidate();
        assert!(pending.respond(json!({"ok":true})).is_err());
        child.join().unwrap();
        assert!(owner.pending().unwrap().is_none());
    }
}
