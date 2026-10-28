//! Platform-free ordering and held-input accounting used by native providers.
//! A segment is one Unicode scalar, complete chord, click pair or wheel step.
#[cfg(any(target_os = "linux", test))]
use std::path::PathBuf;

// Only native mutation/paired cleanup owns the active bit. Readiness reads and
// waiting for the cross-process lock are cancellable without a cleanup claim.
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Expired,
    Closed,
}

pub struct MutationGate(AtomicU8);
impl MutationGate {
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }
    pub fn close(&self) {
        self.0.fetch_or(2, Ordering::SeqCst);
    }
    pub fn closing(&self) -> bool {
        self.0.load(Ordering::SeqCst) & 2 != 0
    }
    #[cfg(test)]
    pub fn active(&self) -> bool {
        self.0.load(Ordering::SeqCst) & 1 != 0
    }
    pub fn fail_cleanup(&self) {
        self.0.fetch_or(4, Ordering::SeqCst);
    }
    pub fn cleanup_failed(&self) -> bool {
        self.0.load(Ordering::SeqCst) & 4 != 0
    }
    pub fn quiesce(&self, budget: Duration) -> bool {
        self.close();
        let until = Instant::now() + budget;
        loop {
            let state = self.0.load(Ordering::SeqCst);
            let left = until.saturating_duration_since(Instant::now());
            if let Some(clean) = self.quiesce_decision(state, left.is_zero()) {
                return clean;
            }
            if !left.is_zero() {
                std::thread::sleep(left.min(Duration::from_millis(2)));
            }
        }
    }
    fn quiesce_decision(&self, state: u8, expired: bool) -> Option<bool> {
        if state & 1 == 0 {
            if state & 4 != 0 {
                return Some(false);
            }
            // Cleanup can fail independently after active was cleared. Confirm
            // this clean snapshot before returning success to another closer.
            return self
                .0
                .compare_exchange(state, state, Ordering::SeqCst, Ordering::SeqCst)
                .ok()
                .map(|_| true);
        }
        // A concurrent release wins if it clears active before this CAS. Never
        // turn an obsolete active snapshot into a late cleanup failure.
        if expired
            && self
                .0
                .compare_exchange(state, state | 4, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Some(false);
        }
        None
    }
    pub fn admit_before(&self, until: Instant) -> Result<Mutation<'_>, AdmissionError> {
        if Instant::now() >= until {
            return Err(AdmissionError::Expired);
        }
        let mutation = self.admit().ok_or(AdmissionError::Closed)?;
        // Scheduling may cross the deadline between the first check and CAS.
        if Instant::now() >= until {
            return Err(AdmissionError::Expired);
        }
        Ok(mutation)
    }
    fn admit(&self) -> Option<Mutation<'_>> {
        self.0
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Mutation { gate: self })
    }
}
pub struct Mutation<'a> {
    gate: &'a MutationGate,
}
impl Drop for Mutation<'_> {
    fn drop(&mut self) {
        self.gate.0.fetch_and(!1, Ordering::SeqCst);
    }
}

pub enum Step<'a, T> {
    Verify,
    Dispatch(&'a T),
}

pub fn run<T, E>(
    segments: &[T],
    mut step: impl FnMut(Step<'_, T>) -> Result<(), E>,
) -> Result<(), E> {
    for segment in segments {
        step(Step::Verify)?;
        step(Step::Dispatch(segment))?;
    }
    step(Step::Verify)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldInput {
    Key(u8),
    Button(u8),
}
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
pub enum Cleanup {
    Releases(Vec<HeldInput>),
    Unconfirmed(bool),
}
#[cfg(any(target_os = "linux", test))]
#[derive(Default)]
pub struct HeldInputs(Vec<HeldInput>);
#[cfg(any(target_os = "linux", test))]
impl HeldInputs {
    pub fn pressed(&mut self, input: HeldInput) {
        if !self.0.contains(&input) {
            self.0.push(input);
        }
    }
    pub fn released(&mut self, input: HeldInput) {
        self.0.retain(|held| *held != input);
    }
    pub fn releases(&self) -> Vec<HeldInput> {
        self.0.iter().rev().copied().collect()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn after_failed_event(
        &self,
        acknowledgement_unknown: bool,
        attempted_down: bool,
    ) -> Cleanup {
        if acknowledgement_unknown {
            Cleanup::Unconfirmed(!self.is_empty() || attempted_down)
        } else {
            Cleanup::Releases(self.releases())
        }
    }
}

pub fn valid_text(text: &str, allow_empty: bool) -> bool {
    if (!allow_empty && text.is_empty()) || text.len() > 16 * 1024 || text.contains('\0') {
        return false;
    }
    let encoded = 2 + text
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\x08' | '\x0c' | '\n' | '\r' | '\t' => 2,
            '\x00'..='\x1f' => 6,
            _ => c.len_utf8(),
        })
        .sum::<usize>();
    encoded <= 48 * 1024
}

pub fn normalized_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(any(target_os = "linux", test))]
pub fn linux_input_idle(keys: &[u8], buttons: &[u32]) -> bool {
    keys.iter().all(|key| *key == 0) && buttons.iter().all(|buttons| *buttons == 0)
}
#[cfg(any(target_os = "linux", test))]
pub fn linux_text_group_supported(modifiers: u32, group: u8) -> bool {
    modifiers & 0xef == 0 && group == 0
}

// Never use TMPDIR: different Agent hosts must coordinate the same X11 input.
#[cfg(any(target_os = "linux", test))]
pub fn linux_input_lock_path(uid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/dev.caelis.dtw-input-{uid}.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_during_preflight_needs_no_mutation_cleanup() {
        let gate = MutationGate::new();
        gate.close();
        assert!(gate.closing());
        assert!(!gate.active());
        assert!(!gate.cleanup_failed());
        assert!(gate.admit().is_none());
    }
    #[test]
    fn cancellation_preserves_the_admitted_pair_until_its_guard_releases() {
        let gate = MutationGate::new();
        let pair = gate.admit().unwrap();
        gate.close();
        assert!(gate.active());
        assert!(gate.admit().is_none());
        drop(pair);
        assert!(!gate.active());
        assert!(!gate.cleanup_failed());
        assert!(gate.admit().is_none());
    }
    #[test]
    fn cleanup_failure_remains_sticky_after_mutation_releases() {
        let gate = MutationGate::new();
        let pair = gate.admit().unwrap();
        gate.fail_cleanup();
        drop(pair);
        assert!(!gate.active());
        assert!(gate.cleanup_failed());
        assert!(gate.admit().is_none());
    }
    #[test]
    fn quiesce_timeout_stays_failed_after_the_active_pair_finishes() {
        let gate = MutationGate::new();
        let pair = gate.admit().unwrap();
        assert!(!gate.quiesce(Duration::ZERO));
        assert!(gate.cleanup_failed());
        drop(pair);
        assert!(!gate.quiesce(Duration::from_secs(1)));
        assert!(matches!(
            gate.admit_before(Instant::now() + Duration::from_secs(1)),
            Err(AdmissionError::Closed)
        ));
    }
    #[test]
    fn release_winning_the_timeout_cas_does_not_create_a_late_failure() {
        let gate = MutationGate::new();
        let pair = gate.admit().unwrap();
        gate.close();
        let active_snapshot = gate.0.load(Ordering::SeqCst);
        drop(pair);
        // Another closer has already observed the successful release.
        assert!(gate.quiesce(Duration::ZERO));
        assert_eq!(gate.quiesce_decision(active_snapshot, true), None);
        assert!(gate.quiesce(Duration::ZERO));
        assert!(!gate.cleanup_failed());
    }
    #[test]
    fn stale_inactive_snapshot_cannot_hide_concurrent_cleanup_failure() {
        let gate = MutationGate::new();
        gate.close();
        let inactive_snapshot = gate.0.load(Ordering::SeqCst);
        std::thread::scope(|scope| {
            scope.spawn(|| gate.fail_cleanup()).join().unwrap();
        });
        assert_eq!(gate.quiesce_decision(inactive_snapshot, false), None);
        assert!(!gate.quiesce(Duration::ZERO));
        assert!(gate.cleanup_failed());
    }
    #[test]
    fn timeout_winning_the_cas_cannot_be_overwritten_by_release() {
        let gate = MutationGate::new();
        let pair = gate.admit().unwrap();
        gate.close();
        let active_snapshot = gate.0.load(Ordering::SeqCst);
        assert_eq!(gate.quiesce_decision(active_snapshot, true), Some(false));
        drop(pair);
        assert!(!gate.quiesce(Duration::ZERO));
    }
    #[test]
    fn concurrent_closers_keep_the_first_recorded_timeout() {
        let gate = MutationGate::new();
        let pair = gate.admit().unwrap();
        let failed = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                let result = gate.quiesce(Duration::ZERO);
                failed.wait();
                result
            });
            failed.wait();
            drop(pair);
            let second = scope.spawn(|| gate.quiesce(Duration::from_secs(1)));
            assert!(!first.join().unwrap());
            assert!(!second.join().unwrap());
        });
    }
    #[test]
    fn deadline_crossed_during_preflight_admits_no_mutation() {
        let gate = MutationGate::new();
        let mut deadline = Instant::now() + Duration::from_secs(1);
        let mut sends = 0;
        let result = run(&[()], |step| match step {
            Step::Verify => {
                assert!(!gate.active());
                // Model a slow successful provider read without sleeping.
                deadline = Instant::now();
                Ok(())
            }
            Step::Dispatch(_) => {
                let _mutation = gate.admit_before(deadline)?;
                sends += 1;
                Ok(())
            }
        });
        assert_eq!(result, Err(AdmissionError::Expired));
        assert_eq!(sends, 0);
        assert!(!gate.active());
        assert!(!gate.cleanup_failed());
    }
    #[test]
    fn closing_during_preflight_does_not_admit_or_claim_active_cleanup() {
        let gate = MutationGate::new();
        let mut sends = 0;
        let result = run(&[()], |step| match step {
            Step::Verify => {
                assert!(gate.quiesce(Duration::ZERO));
                Ok(())
            }
            Step::Dispatch(_) => {
                let _mutation = gate.admit_before(Instant::now() + Duration::from_secs(1))?;
                sends += 1;
                Ok(())
            }
        });
        assert_eq!(result, Err(AdmissionError::Closed));
        assert_eq!(sends, 0);
        assert!(!gate.cleanup_failed());
    }
    #[test]
    fn expired_suffix_keeps_the_original_completed_pair_without_replay() {
        let gate = MutationGate::new();
        let mut deadline = Instant::now() + Duration::from_secs(1);
        let mut sent = Vec::new();
        let result = run(
            &[["down", "up"], ["next down", "next up"]],
            |step| match step {
                Step::Verify => {
                    if !sent.is_empty() {
                        deadline = Instant::now();
                    }
                    Ok(())
                }
                Step::Dispatch(pair) => {
                    let _mutation = gate.admit_before(deadline)?;
                    sent.extend_from_slice(pair);
                    Ok(())
                }
            },
        );
        assert_eq!(result, Err(AdmissionError::Expired));
        assert_eq!(sent, ["down", "up"]);
        assert!(gate.quiesce(Duration::ZERO));
    }
    #[test]
    fn closing_after_admission_keeps_the_existing_release_interval() {
        let gate = MutationGate::new();
        let pair = gate
            .admit_before(Instant::now() + Duration::from_secs(1))
            .unwrap();
        let mut sent = vec!["down"];
        gate.close();
        assert!(matches!(
            gate.admit_before(Instant::now()),
            Err(AdmissionError::Expired)
        ));
        assert!(gate.active());
        sent.push("up");
        drop(pair);
        assert_eq!(sent, ["down", "up"]);
        assert!(gate.quiesce(Duration::ZERO));
    }
    #[test]
    fn target_change_after_a_complete_pair_blocks_all_suffix_segments() {
        let segments = [vec!["down", "up"], vec!["down", "up"]];
        let mut target = true;
        let mut sent = Vec::new();
        let result = run(&segments, |step| match step {
            Step::Verify => {
                if target {
                    Ok(())
                } else {
                    Err("target changed")
                }
            }
            Step::Dispatch(pair) => {
                sent.extend_from_slice(pair);
                target = false;
                Ok(())
            }
        });
        assert_eq!(result, Err("target changed"));
        assert_eq!(sent, ["down", "up"]);
    }
    #[test]
    fn failed_first_post_focus_validation_never_dispatches_old_coordinates() {
        let mut sends = 0;
        let result = run(&[1, 2], |step| match step {
            Step::Verify => Err("geometry changed on focus"),
            Step::Dispatch(_) => {
                sends += 1;
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(sends, 0);
    }
    #[test]
    fn partial_dispatch_error_does_not_replay_or_run_the_next_segment() {
        let mut sends = 0;
        let result = run(&[1, 2], |step| match step {
            Step::Verify => Ok(()),
            Step::Dispatch(_) => {
                sends += 1;
                Err("unknown")
            }
        });
        assert!(result.is_err());
        assert_eq!(sends, 1);
    }
    #[test]
    fn cleanup_contains_only_confirmed_still_held_prefix_in_reverse_order() {
        let mut held = HeldInputs::default();
        held.pressed(HeldInput::Key(37));
        held.pressed(HeldInput::Key(38));
        held.released(HeldInput::Key(38));
        assert_eq!(held.releases(), [HeldInput::Key(37)]);
        held.released(HeldInput::Key(37));
        assert!(held.is_empty());
        // A future key/button was never confirmed; no synthetic release for it.
        assert!(held.releases().is_empty());
        held.pressed(HeldInput::Key(50));
        held.pressed(HeldInput::Button(1));
        assert_eq!(held.releases(), [HeldInput::Button(1), HeldInput::Key(50)]);
        held.released(HeldInput::Button(1));
        held.released(HeldInput::Button(1));
        assert_eq!(held.releases(), [HeldInput::Key(50)]);
    }
    #[test]
    fn missing_up_ack_never_replays_that_up_and_rejected_down_does_not_add_a_future_release() {
        let mut held = HeldInputs::default();
        held.pressed(HeldInput::Key(37));
        assert_eq!(
            held.after_failed_event(true, false),
            Cleanup::Unconfirmed(true)
        );
        assert_eq!(
            held.after_failed_event(false, true),
            Cleanup::Releases(vec![HeldInput::Key(37)])
        );
        held.released(HeldInput::Key(37));
        assert_eq!(
            held.after_failed_event(true, false),
            Cleanup::Unconfirmed(false)
        );
        assert_eq!(
            held.after_failed_event(true, true),
            Cleanup::Unconfirmed(true)
        );
        assert_eq!(
            held.after_failed_event(false, true),
            Cleanup::Releases(vec![])
        );
    }
    #[test]
    fn text_contract_is_bytes_and_encoded_bytes_not_an_extra_character_limit() {
        assert!(valid_text(&"a".repeat(16384), false));
        assert!(!valid_text(&"a".repeat(16385), false));
        assert!(valid_text(&"🐦".repeat(4096), false));
        assert!(!valid_text(&"🐦".repeat(4097), false));
        assert!(!valid_text(&"\x01".repeat(8192), false));
        assert!(valid_text(&"\x01".repeat(8191), false));
        assert!(!valid_text("\0", true));
        assert!(valid_text("", true));
        assert!(!valid_text("", false));
        assert_eq!(normalized_text("a\r\nb"), "a\nb");
    }
    #[test]
    fn full_xi_button_mask_covers_horizontal_wheel_and_text_rejects_other_groups() {
        assert!(linux_input_idle(&[0; 32], &[0]));
        assert!(!linux_input_idle(&[0; 32], &[1 << 6]));
        assert!(!linux_input_idle(&[0; 32], &[0, 1]));
        assert!(!linux_input_idle(&[1], &[0]));
        assert!(linux_text_group_supported(16, 0));
        assert!(!linux_text_group_supported(2, 0));
        assert!(!linux_text_group_supported(0, 1));
    }
    #[test]
    fn lock_name_is_stable_for_the_uid_and_not_host_tmpdir() {
        assert_eq!(
            linux_input_lock_path(501),
            PathBuf::from("/tmp/dev.caelis.dtw-input-501.lock")
        );
        assert_ne!(linux_input_lock_path(501), linux_input_lock_path(502));
    }
}
