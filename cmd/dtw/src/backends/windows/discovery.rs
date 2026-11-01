//! Bounded traversal scheduling, independent of any one UIA provider. A node
//! with unreadable identity/ownership is never published, but must not erase
//! independent branches which can prove their own ownership and runtime IDs.
use crate::computer::Error;
use std::collections::HashSet;

pub(super) struct Branches<'a, E> {
    pub frontier: &'a mut Vec<(E, u32)>,
    pub visited: &'a mut HashSet<Vec<i32>>,
    pub gap: &'a mut Option<String>,
}
impl<E> Branches<'_, E> {
    pub fn admit(
        &mut self,
        depth: u32,
        next_sibling: impl FnOnce() -> Result<Option<E>, Error>,
        first_child: impl FnOnce() -> Result<Option<E>, Error>,
        prove_owned: impl FnOnce() -> Result<(), Error>,
        runtime_id: impl FnOnce() -> Result<Vec<i32>, Error>,
    ) -> Result<bool, Error> {
        // Siblings have independent ownership proofs. Reading this node's
        // parent/identity may fail while later siblings remain perfectly valid.
        if depth > 0 {
            self.queue(next_sibling(), depth, false)?;
        }
        if let Err(error) = prove_owned() {
            let reason = match error {
                Error::TimedOut | Error::Cancelled | Error::Closed => return Err(error),
                Error::TargetChanged => "tree_changed",
                _ => "native_read_gap",
            };
            self.gap.get_or_insert(reason.into());
            // No refs are retained for this node. Children will run their own
            // fresh root proof; neither its existence nor traversal grants it.
            self.queue(first_child(), depth + 1, depth >= 64)?;
            return Ok(false);
        }
        let identified = match runtime_id() {
            Ok(runtime) => {
                if !self.visited.insert(runtime) {
                    self.gap.get_or_insert("tree_changed".into());
                    // A recognized repeated node must not recursively expand again.
                    return Ok(false);
                }
                true
            }
            Err(_) => {
                self.gap.get_or_insert("native_read_gap".into());
                false
            }
        };
        // Runtime IDs can be unavailable on structural nodes (including empty
        // SAFEARRAYs). Their independently verifiable children must survive.
        self.queue(first_child(), depth + 1, depth >= 64)?;
        Ok(identified)
    }
    fn queue(
        &mut self,
        read: Result<Option<E>, Error>,
        depth: u32,
        limit: bool,
    ) -> Result<(), Error> {
        match read {
            Ok(Some(_)) if limit => {
                self.gap.get_or_insert("depth_limit".into());
            }
            Ok(Some(element)) => self.frontier.push((element, depth)),
            Ok(None) => (),
            Err(error @ (Error::TimedOut | Error::Cancelled | Error::Closed)) => return Err(error),
            Err(_) => {
                self.gap.get_or_insert("native_read_gap".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Fault injection uses a small tree and the actual production frontier
    // scheduler. 0 -> [1 -> [2], 3]. Nodes 2 and 3 are independently readable.
    fn scan(bad_ownership: bool) -> (Vec<usize>, Option<String>) {
        let mut frontier = vec![(0usize, 0)];
        let mut visited = HashSet::new();
        let mut gap = None;
        let mut published = Vec::new();
        while let Some((node, depth)) = frontier.pop() {
            let owned = if node == 1 && bad_ownership {
                Err(Error::BackendFailed("parent read failed".into()))
            } else {
                Ok(())
            };
            let identity = if node == 1 {
                Err(Error::InvalidObservation)
            } else {
                Ok(vec![node as i32])
            };
            if (Branches {
                frontier: &mut frontier,
                visited: &mut visited,
                gap: &mut gap,
            })
            .admit(
                depth,
                || Ok((node == 1).then_some(3)),
                || {
                    Ok(match node {
                        0 => Some(1),
                        1 => Some(2),
                        _ => None,
                    })
                },
                || owned,
                || identity,
            )
            .unwrap()
            {
                published.push(node);
            }
        }
        (published, gap)
    }
    #[test]
    fn missing_runtime_id_does_not_erase_readable_children_or_siblings() {
        let (nodes, gap) = scan(false);
        assert_eq!(nodes, [0, 2, 3]);
        assert_eq!(gap.as_deref(), Some("native_read_gap"));
    }
    #[test]
    fn failed_parent_proof_does_not_publish_node_or_erase_independent_branches() {
        let (nodes, gap) = scan(true);
        assert_eq!(nodes, [0, 2, 3]);
        assert_eq!(gap.as_deref(), Some("native_read_gap"));
    }
    #[test]
    fn repeated_runtime_id_never_expands_children_again() {
        let mut frontier = Vec::new();
        let mut visited = HashSet::from([vec![1]]);
        let mut gap = None;
        assert!(!(Branches {
            frontier: &mut frontier,
            visited: &mut visited,
            gap: &mut gap
        })
        .admit(
            1,
            || Ok(Some(3)),
            || panic!("cycle must not expand"),
            || Ok(()),
            || Ok(vec![1])
        )
        .unwrap());
        assert_eq!(frontier, [(3, 1)]);
        assert_eq!(gap.as_deref(), Some("tree_changed"));
    }
    #[test]
    fn depth_limit_and_control_stop_preserve_truthful_coverage() {
        let mut frontier = Vec::new();
        let mut visited = HashSet::new();
        let mut gap = None;
        assert!((Branches {
            frontier: &mut frontier,
            visited: &mut visited,
            gap: &mut gap
        })
        .admit(64, || Ok(None), || Ok(Some(2)), || Ok(()), || Ok(vec![1]))
        .unwrap());
        assert!(frontier.is_empty());
        assert_eq!(gap.as_deref(), Some("depth_limit"));
        for error in [Error::Cancelled, Error::TimedOut, Error::Closed] {
            assert_eq!(
                (Branches {
                    frontier: &mut frontier,
                    visited: &mut visited,
                    gap: &mut gap
                })
                .admit(
                    0,
                    || panic!("root has no sibling"),
                    || panic!("must stop before descent"),
                    || Err(error.clone()),
                    || panic!("must stop before identity")
                ),
                Err(error)
            );
        }
    }
}
