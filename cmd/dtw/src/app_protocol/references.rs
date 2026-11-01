//! Public lookup keys, not native authentication tokens or reusable slot numbers.
use crate::computer::Error;
use base64::Engine;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

struct References {
    namespace: String,
    issued: AtomicU64,
}

impl References {
    fn new() -> Self {
        let random = uuid::Uuid::new_v4();
        let bytes = random.as_bytes();
        // Keep 64 random bits, excluding the UUID version and variant fields.
        // These local lookup keys are not authentication tokens. A shared counter
        // prevents all in-process aliases; randomness isolates process restarts.
        let mut namespace = [0; 8];
        namespace[..6].copy_from_slice(&bytes[..6]);
        namespace[6..].copy_from_slice(&bytes[14..]);
        Self {
            namespace: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(namespace),
            issued: AtomicU64::new(0),
        }
    }

    fn next(&self, prefix: &str) -> Result<String, Error> {
        let previous = self
            .issued
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |issued| {
                issued.checked_add(1)
            })
            .map_err(|_| Error::NativeFailure {
                code: "reference_exhausted".into(),
                message: "Public reference space exhausted; start a new DTW process".into(),
            })?;
        // The successful checked update above proves this addition cannot wrap.
        Ok(format!(
            "{prefix}_{}_{}",
            self.namespace,
            base36(previous + 1)
        ))
    }
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buffer = [0; 13]; // ceil(log36(u64::MAX))
    let mut start = buffer.len();
    loop {
        start -= 1;
        buffer[start] = DIGITS[(value % 36) as usize];
        value /= 36;
        if value == 0 {
            return String::from_utf8(buffer[start..].to_vec()).unwrap();
        }
    }
}

pub(super) fn reference(prefix: &str) -> Result<String, Error> {
    // Shared by every server, app, and replacement Coordinator in this process.
    // Dropping a context or evicting history must never reset the counter.
    static REFERENCES: OnceLock<References> = OnceLock::new();
    REFERENCES.get_or_init(References::new).next(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, sync::Arc};

    #[test]
    fn compact_references_share_a_counter_across_kinds() {
        let references = References::new();
        assert_eq!(references.namespace.len(), 11);
        assert!(references
            .namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte)));
        for (index, prefix) in ["app", "win", "obs", "el", "cur", "exe"]
            .into_iter()
            .enumerate()
        {
            let value = references.next(prefix).unwrap();
            assert_eq!(
                value,
                format!("{prefix}_{}_{}", references.namespace, index + 1)
            );
            assert!(value.len() <= 17);
        }
    }

    #[test]
    fn parallel_allocation_never_reuses_a_reference() {
        let references = Arc::new(References::new());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let references = references.clone();
                std::thread::spawn(move || {
                    (0..1000)
                        .map(|_| references.next("win").unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let values: HashSet<_> = threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(values.len(), 8000);
        assert_eq!(references.issued.load(Ordering::Relaxed), 8000);
    }

    #[test]
    fn counter_boundaries_are_compact_and_exhaustion_is_permanent() {
        for (counter, expected) in [
            (0, "0"),
            (35, "z"),
            (36, "10"),
            (1295, "zz"),
            (1296, "100"),
            (u64::MAX, "3w5e11264sgsf"),
        ] {
            assert_eq!(base36(counter), expected);
        }
        let references = References {
            namespace: "boundary".into(),
            issued: AtomicU64::new(u64::MAX - 1),
        };
        assert_eq!(
            references.next("win").unwrap(),
            "win_boundary_3w5e11264sgsf"
        );
        for prefix in ["win", "obs", "exe"] {
            assert!(
                matches!(references.next(prefix), Err(Error::NativeFailure { code, .. }) if code == "reference_exhausted")
            );
        }
        assert_eq!(references.issued.load(Ordering::Relaxed), u64::MAX);
    }
}
