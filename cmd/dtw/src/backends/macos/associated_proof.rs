//! Association screenshots obey the same short-lived, globally leased Scene
//! lifetime as captures. A resident AX worker never creates an SCK client.
use super::super::process::{StopReason, TransportError, TransportErrorKind, Worker};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::{c_char, CStr, CString},
    time::{Duration, Instant},
};

// One sequential native dispatch can inspect several candidate surfaces. Reuse
// its proof child, then reap it before publishing the reply or acquiring a
// different native resource. Errors never replay a request in a new child.
struct Owner<C> {
    child: Option<C>,
    failure: Option<Value>,
}
impl<C> Default for Owner<C> {
    fn default() -> Self {
        Self {
            child: None,
            failure: None,
        }
    }
}
impl<C> Owner<C> {
    fn request(
        &mut self,
        spawn: impl FnOnce() -> Result<C, Value>,
        call: impl FnOnce(&C) -> Result<Value, Value>,
    ) -> Value {
        if let Some(error) = &self.failure {
            return error.clone();
        }
        let result = (|| {
            if self.child.is_none() {
                self.child = Some(spawn()?);
            }
            call(self.child.as_ref().unwrap())
        })();
        match result {
            Ok(value) => value,
            Err(error) => {
                self.failure = Some(error.clone());
                error
            }
        }
    }

    fn close(&mut self, stop: impl FnOnce(&C) -> (Result<(), Value>, bool)) -> Result<(), Value> {
        let Some(child) = &self.child else {
            return Ok(());
        };
        let (result, reaped) = stop(child);
        if reaped {
            self.child = None;
        }
        let result = result.and_then(|()| {
            if reaped {
                Ok(())
            } else {
                Err(unavailable("Association proof child exit is unconfirmed"))
            }
        });
        if let Err(error) = &result {
            self.failure.get_or_insert_with(|| error.clone());
        }
        result
    }
}

struct Session {
    owner: Owner<Worker>,
    sequence: u64,
    deadline: Instant,
    local: bool,
}
thread_local! { static ACTIVE: RefCell<Option<Session>> = const { RefCell::new(None) }; }

pub(super) struct Scope;
impl Scope {
    pub fn new(local: bool, budget: Duration) -> Self {
        ACTIVE.with(|slot| {
            debug_assert!(
                slot.borrow().is_none(),
                "Only a worker dispatch owns association proof"
            );
            slot.replace(Some(Session {
                owner: Owner::default(),
                sequence: 0,
                deadline: Instant::now() + budget,
                local,
            }));
        });
        Self
    }

    pub fn finish(&self, result: &mut Value) {
        if let Err(error) = flush() {
            append_cleanup(result, error);
        }
    }
}

fn append_cleanup(result: &mut Value, error: Value) {
    // The original input dispatch/error remains authoritative. The existing
    // provider cleanup channel records this later, independent failure.
    if result.get("cleanup_error").is_none() {
        result["cleanup_error"] = error;
    } else {
        result["association_cleanup_error"] = error;
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        let _ = flush();
        ACTIVE.with(|slot| {
            slot.take();
        });
    }
}

/// Release the helper's Scene lease before a parent attempts input lock
/// handover or takes its own Scene lease. Never continue after an unproven reap.
pub(super) fn flush() -> Result<(), Value> {
    ACTIVE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return Ok(());
        };
        session.owner.close(|child| {
            let result = child.stop().map_err(transport_error);
            (result, child.is_reaped())
        })
    })
}

/// Only the actual Scene process may set this after acquiring its global lease.
pub(super) fn use_local() {
    ACTIVE.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            debug_assert!(session.owner.child.is_none());
            session.local = true;
        }
    });
}

fn unavailable(message: impl std::fmt::Display) -> Value {
    json!({"error":"association_unavailable","message":message.to_string()})
}
fn transport_error(error: TransportError) -> Value {
    let code = match error.kind {
        TransportErrorKind::Control(StopReason::Cancelled) => "native_stopping",
        TransportErrorKind::Control(StopReason::TimedOut) => "deadline_exceeded",
        _ => "association_unavailable",
    };
    json!({"error":code,"message":format!("Association proof transport: {error}"),
        "details":{"proof_request_may_have_been_sent":error.request_may_have_been_sent}})
}

fn reply(bytes: &[u8], request_id: &Value) -> Result<Value, Value> {
    let envelope: Value = serde_json::from_slice(bytes)
        .map_err(|_| unavailable("Malformed association proof reply"))?;
    if &envelope["request_id"] != request_id {
        return Err(unavailable(
            "Association proof reply identity differs from its request",
        ));
    }
    let result = &envelope["result"];
    if result["error"].is_string()
        || (result.as_object().is_some_and(|object| object.len() == 1)
            && result["proven"].is_boolean())
    {
        Ok(result.clone())
    } else {
        Err(unavailable("Incomplete association proof reply"))
    }
}

fn prove(request: Value) -> Value {
    if request["operation"] != "prove_associated_surface" {
        return unavailable("Invalid association proof operation");
    }
    let local = ACTIVE.with(|slot| slot.borrow().as_ref().map(|session| session.local));
    match local {
        None => return unavailable("Association proof has no native request owner"),
        Some(true) => return super::worker::native(&request, true),
        Some(false) => {}
    }
    ACTIVE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let session = slot.as_mut().unwrap();
        let until = session
            .deadline
            .min(Instant::now() + super::worker::remaining(&request));
        let control = || {
            if super::worker::stopping() {
                Err(StopReason::Cancelled)
            } else {
                Ok(until.saturating_duration_since(Instant::now()))
            }
        };
        if let Err(reason) = control().and_then(|left| {
            if left.is_zero() {
                Err(StopReason::TimedOut)
            } else {
                Ok(left)
            }
        }) {
            return transport_error(TransportError {
                kind: TransportErrorKind::Control(reason),
                request_may_have_been_sent: false,
            });
        }
        session.sequence += 1;
        let mut request = request;
        request["request_id"] = json!(session.sequence);
        let request_id = request["request_id"].clone();
        session.owner.request(
            || {
                let exe = std::env::current_exe().map_err(unavailable)?;
                Worker::spawn(&exe, &["--dtw-scene-worker"]).map_err(transport_error)
            },
            |child| {
                let bytes = child
                    .call(request.to_string().as_bytes(), control)
                    .map_err(transport_error)?;
                reply(&bytes, &request_id)
            },
        )
    })
}

pub(super) extern "C" fn callback(input: *const c_char) -> *mut c_char {
    let result = if input.is_null() {
        unavailable("Empty association proof request")
    } else {
        match serde_json::from_slice(unsafe { CStr::from_ptr(input) }.to_bytes()) {
            Ok(request) => prove(request),
            Err(_) => unavailable("Malformed association proof request"),
        }
    };
    CString::new(result.to_string())
        .expect("JSON has no literal NUL")
        .into_raw()
}
pub(super) extern "C" fn free(reply: *mut c_char) {
    if !reply.is_null() {
        unsafe {
            drop(CString::from_raw(reply));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn proof_child_is_lazy_reused_and_reaped_before_local_handover() {
        let mut owner = Owner::<u8>::default();
        owner.close(|_| panic!("No proof means no child")).unwrap();
        assert_eq!(
            owner.request(|| Ok(1), |_| Ok(json!({"proven":true}))),
            json!({"proven":true})
        );
        assert_eq!(
            owner.request(
                || panic!("Reuse this dispatch's child"),
                |child| {
                    assert_eq!(*child, 1);
                    Ok(json!({"proven":false}))
                }
            ),
            json!({"proven":false})
        );
        let stopped = Cell::new(false);
        owner
            .close(|child| {
                assert_eq!(*child, 1);
                stopped.set(true);
                (Ok(()), true)
            })
            .unwrap();
        assert!(stopped.get());
        assert!(owner.child.is_none());
        assert_eq!(
            owner.request(|| Ok(2), |_| Ok(json!({"proven":true}))),
            json!({"proven":true})
        );
    }

    #[test]
    fn failed_proof_is_not_replayed_and_cleanup_preserves_first_error() {
        let mut owner = Owner::<u8>::default();
        let original = json!({"error":"deadline_exceeded","message":"original"});
        assert_eq!(owner.request(|| Ok(1), |_| Err(original.clone())), original);
        assert_eq!(
            owner.request(|| panic!("No replacement"), |_| panic!("No replay")),
            original
        );
        let cleanup = unavailable("reap failed");
        assert_eq!(owner.close(|_| (Err(cleanup.clone()), false)), Err(cleanup));
        assert!(owner.child.is_some());
        assert_eq!(owner.failure, Some(original));
        owner.close(|_| (Ok(()), true)).unwrap();
        assert!(owner.child.is_none());
    }

    #[test]
    fn successful_reply_does_not_release_scene_until_the_child_is_reaped() {
        let path = std::env::temp_dir().join(format!("dtw-proof-lease-{}", uuid::Uuid::new_v4()));
        let path_text = path.to_str().unwrap();
        let code = "import fcntl,json,sys\nf=open(sys.argv[1],'w')\nfcntl.flock(f,fcntl.LOCK_EX)\nfor line in sys.stdin:\n r=json.loads(line);print(json.dumps({'request_id':r['request_id'],'result':{'proven':True}}),flush=True)";
        let mut owner = Owner::<Worker>::default();
        let first = owner.request(
            || {
                Worker::spawn(
                    std::path::Path::new("python3"),
                    &["-u", "-c", code, path_text],
                )
                .map_err(transport_error)
            },
            |child| {
                let bytes = child
                    .call(b"{\"request_id\":1}", || Ok(Duration::from_secs(2)))
                    .map_err(transport_error)?;
                reply(&bytes, &json!(1))
            },
        );
        assert_eq!(first, json!({"proven":true}));
        // A completed proof is not SCK-client teardown. A competing process
        // cannot acquire the same resource before this exact owned child exits.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(super::super::worker::scene_lease::lock_scene_at(&path, Duration::ZERO).is_err());
        owner
            .close(|child| (child.stop().map_err(transport_error), child.is_reaped()))
            .unwrap();
        drop(
            super::super::worker::scene_lease::lock_scene_at(&path, Duration::from_secs(1))
                .unwrap(),
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unconfirmed_exit_does_not_admit_a_replacement_proof_child() {
        let mut owner = Owner::<u8>::default();
        owner.request(|| Ok(1), |_| Ok(json!({"proven":true})));
        let error = owner.close(|_| (Ok(()), false)).unwrap_err();
        assert_eq!(
            owner.request(
                || panic!("Unreaped child cannot be replaced"),
                |_| panic!("Cannot reuse after cleanup failed")
            ),
            error
        );
        assert!(owner.child.is_some());
    }

    #[test]
    fn proof_success_requires_exact_reply_and_typed_boolean() {
        for proven in [true, false] {
            let bytes = json!({"request_id":7,"result":{"proven":proven}}).to_string();
            assert_eq!(
                reply(bytes.as_bytes(), &json!(7)).unwrap(),
                json!({"proven":proven})
            );
        }
        let original = json!({"error":"target_changed","message":"window replaced"});
        assert_eq!(
            reply(
                json!({"request_id":7,"result":original})
                    .to_string()
                    .as_bytes(),
                &json!(7)
            )
            .unwrap(),
            original
        );
        for value in [
            json!({"request_id":8,"result":{"proven":true}}),
            json!({"request_id":7,"result":{"proven":1}}),
            json!({"request_id":7,"result":{"proven":true,"cleanup_error":{}}}),
            json!({"request_id":7,"result":{}}),
        ] {
            assert!(reply(value.to_string().as_bytes(), &json!(7)).is_err());
        }
    }

    #[test]
    fn missing_owner_or_invalid_operation_cannot_spawn_or_run_sck() {
        assert_eq!(
            prove(json!({"operation":"prove_associated_surface"}))["error"],
            "association_unavailable"
        );
        assert_eq!(
            prove(json!({"operation":"click"}))["error"],
            "association_unavailable"
        );
    }

    #[test]
    fn later_proof_cleanup_never_rewrites_the_original_input_outcome() {
        for dispatch in ["not_dispatched", "dispatched", "unknown"] {
            let mut result = json!({"dispatch":dispatch,"error":"original_error","message":"original receipt",
                "delivery":{"route":"foreground","foreground_borrowed":true}});
            let original = result.clone();
            let cleanup = unavailable("proof child shutdown failed");
            append_cleanup(&mut result, cleanup.clone());
            assert_eq!(result["cleanup_error"], cleanup);
            result.as_object_mut().unwrap().remove("cleanup_error");
            assert_eq!(result, original);
        }
        let original_cleanup = json!({"error":"focus_restore_failed"});
        let mut result = json!({"cleanup_error":original_cleanup});
        let proof_cleanup = unavailable("proof cleanup failed");
        append_cleanup(&mut result, proof_cleanup.clone());
        assert_eq!(result["cleanup_error"], original_cleanup);
        assert_eq!(result["association_cleanup_error"], proof_cleanup);
    }
}
