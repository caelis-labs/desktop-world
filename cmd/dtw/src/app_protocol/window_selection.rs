use super::*;

impl<B: Backend + 'static> Coordinator<B> {
    pub(super) fn select_window(
        &self,
        submitted: &Submit,
        title: Option<&str>,
        reference: Option<&str>,
        timeout_ms: u64,
        deadline: Instant,
    ) -> Work {
        if let Some(reference) = reference {
            return if self.state.lock().unwrap().windows.contains_key(reference) {
                Work {
                    completed: Completed {
                        value: Some(json!({"window_ref":reference})),
                        image: None,
                        receipt: None,
                        error: None,
                        payload_bytes: 0,
                        payload_expired: false,
                    },
                    publication: Publication::None,
                    state_update: None,
                }
            } else {
                Work::read_error(Error::TargetChanged)
            };
        }
        let until = (Instant::now() + Duration::from_millis(timeout_ms)).min(deadline);
        loop {
            let mut query = submitted.clone();
            query.command = Command::Windows;
            let work = self.execute_atomic(&query, deadline);
            if work.completed.error.is_some() {
                return work;
            }
            let inventory = work.completed.value.as_ref().unwrap();
            let matches = inventory["windows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| title.is_none_or(|title| row["title"] == title))
                .cloned()
                .collect::<Vec<_>>();
            let coverage = inventory["coverage"].clone();
            // Candidates returned in an ambiguity diagnostic must themselves
            // be usable refs. Publishing read inventory does not select one.
            apply_publication(&mut self.state.lock().unwrap(), work.publication);
            self.record_resolution(
                &submitted.request_id,
                json!({"kind":"window_selection","coverage":coverage}),
            );
            if coverage["complete_within_scope"] != true {
                return Work::read_error(Error::NativeFailure {
                    code: "window_selection_unproven".into(),
                    message:
                        "Partial inventory cannot establish a unique window; inspect app.windows()"
                            .into(),
                });
            }
            if matches.len() > 1 {
                let mut rejected = Work::read_error(Error::NativeFailure {
                    code: "ambiguous_window".into(),
                    message: "More than one window matches; select an observed window_ref".into(),
                });
                if let Some(error) = &mut rejected.completed.error {
                    error.details = Some(json!({"candidates":matches}));
                }
                return rejected;
            }
            if let Some(row) = matches.first() {
                let row = row.clone();
                return Work {
                    completed: Completed {
                        value: Some(row),
                        image: None,
                        receipt: None,
                        error: None,
                        payload_bytes: 0,
                        payload_expired: false,
                    },
                    publication: Publication::None,
                    state_update: None,
                };
            }
            if Instant::now() >= until {
                return Work::read_error(Error::NativeFailure {
                    code: "window_not_found".into(),
                    message: "No matching window appeared within the read deadline".into(),
                });
            }
            std::thread::sleep(
                Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())),
            );
        }
    }
}
