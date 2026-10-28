//! A complete inventory requires proof for every candidate's process owner.
//! Preserve native failures instead of turning unknown ownership into absence.
pub(crate) fn owned_windows<E>(
    windows: impl IntoIterator<Item = u32>,
    app_pid: u32,
    mut owner: impl FnMut(u32) -> Result<u32, E>,
) -> Result<Vec<u32>, E> {
    let mut owned = Vec::new();
    for window in windows {
        if owner(window)? == app_pid {
            owned.push(window);
        }
    }
    Ok(owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct NativeError {
        window: u32,
        code: &'static str,
    }

    #[test]
    fn only_proven_foreign_windows_are_excluded() {
        let result: Result<_, NativeError> = owned_windows([11, 12, 13], 100, |window| {
            Ok(if window == 12 { 200 } else { 100 })
        });
        assert_eq!(result, Ok(vec![11, 13]));
    }

    #[test]
    fn unreadable_known_window_cannot_become_successful_empty_inventory() {
        let result = owned_windows([11], 100, |_| {
            Err(NativeError {
                window: 11,
                code: "xres_read_failed",
            })
        });
        assert_eq!(
            result,
            Err(NativeError {
                window: 11,
                code: "xres_read_failed",
            })
        );
    }

    #[test]
    fn unknown_new_window_invalidates_a_previously_proven_partial_list() {
        let result = owned_windows([11, 99], 100, |window| match window {
            11 => Ok(100),
            _ => Err(NativeError {
                window,
                code: "owner_unproven",
            }),
        });
        assert_eq!(
            result,
            Err(NativeError {
                window: 99,
                code: "owner_unproven",
            })
        );
    }

    #[test]
    fn preserves_first_native_error_without_retry_or_reading_suffix() {
        let mut calls = Vec::new();
        let result = owned_windows([11, 12, 13], 100, |window| {
            calls.push(window);
            Err(NativeError {
                window,
                code: "connection_failed",
            })
        });
        assert_eq!(calls, [11]);
        assert_eq!(
            result,
            Err(NativeError {
                window: 11,
                code: "connection_failed",
            })
        );
    }

    #[test]
    fn proven_empty_candidate_list_needs_no_owner_queries() {
        let result: Result<_, NativeError> =
            owned_windows([], 100, |_| panic!("No candidate means no ownership query"));
        assert_eq!(result, Ok(vec![]));
    }
}
