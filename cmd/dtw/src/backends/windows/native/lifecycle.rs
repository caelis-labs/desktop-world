//! Native proofs and dispatch within the existing supervised Windows store.
use super::super::applications;
use super::*;

impl Store {
    pub(super) fn discover_apps(&mut self, until: Instant) -> Result<Value, Error> {
        let mut inventory = applications::installed(&|| check(until))?;
        let running = running_apps(until, &mut inventory.gaps)?;
        inventory.gaps.sort();
        inventory.gaps.dedup();
        let mut targets: BTreeMap<String, (String, usize)> = inventory
            .targets
            .into_iter()
            .map(|target| (target.app_id, (target.name, 0)))
            .collect();
        for running in running {
            let id = string(&running, "app_id")?.to_owned();
            let name = string(&running, "name")?.to_owned();
            targets.entry(id).or_insert((name, 0)).1 += 1;
        }
        let apps = targets
            .into_iter()
            .map(|(app, (name, count))| {
                let mut row = json!({"app":app,"name":name,"running":count > 0});
                if count > 1 {
                    row["instances"] = json!(count);
                }
                row
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"apps":apps,"coverage":{"scope":"start_menu_app_paths_apps_folder_and_running","installed":true,"complete":inventory.gaps.is_empty(),
            "locations":["user_start_menu","common_start_menu","app_paths_hkcu_hklm","apps_folder","running_user_windows"],"read_error_codes":inventory.gaps}}),
        )
    }
    pub(super) fn launch_app(&mut self, r: &Value, until: Instant) -> Value {
        let mut dispatched = false;
        let result = (|| {
            let _lease = input::Lease::acquire(until)?;
            let target = applications::resolve(string(r, "app_id")?, &|| check(until))?;
            if !target.instances.is_empty() {
                return Ok(
                    json!({"dispatch":"not_dispatched","evidence":"confirmed","postcondition":"application_running", "apps":target.instances.iter().map(|app|json!({"app":app_json(app)})).collect::<Vec<_>>()}),
                );
            }
            {
                let _mutation = input::mutation(until)?;
                check(until)?;
                dispatched = true;
                applications::launch_once(&target)?;
            }
            let settle = until.min(Instant::now() + Duration::from_secs(5));
            let apps = loop {
                check(until)?;
                let apps = applications::running_instances(&target.app_id, &|| check(until))?;
                if !apps.is_empty() || Instant::now() >= settle {
                    break apps;
                }
                std::thread::sleep(Duration::from_millis(30));
            };
            let confirmed = !apps.is_empty();
            Ok(
                json!({"dispatch":"dispatched","evidence":if confirmed {"confirmed"} else {"unverified"},"postcondition":if confirmed {Some("application_running")} else {None},
                "apps":apps.iter().map(|app|json!({"app":app_json(app)})).collect::<Vec<_>>()}),
            )
        })();
        match result {
            Ok(value) => value,
            Err(error) => {
                let mut reply = windows_error_reply(error, true);
                reply["apps"] = json!([]);
                if dispatched {
                    reply["dispatch"] = json!("unknown");
                }
                reply
            }
        }
    }
    pub(super) fn close_application(&mut self, r: &Value, until: Instant) -> Value {
        let mut dispatched = false;
        let result = (|| {
            let _lease = input::Lease::acquire(until)?;
            input::no_held_keys()?;
            let close_window = r["operation"] == "close_window";
            let (app, windows) = if close_window {
                let target = parse_target(&r["target"])?;
                self.validate(&target)?;
                (target.app.clone(), vec![target])
            } else {
                let app = parse_app(&r["target"]["app"])?;
                if self.instance_exited(&app)? {
                    return Err(Error::TargetChanged);
                }
                let mut windows = Vec::new();
                for window in all_windows()? {
                    check(until)?;
                    let mut pid = 0;
                    unsafe {
                        GetWindowThreadProcessId(window, Some(&mut pid));
                    }
                    if pid == app.pid
                        && user_application_window(window)
                        && unsafe { GetWindow(window, GW_OWNER) }.is_err()
                    {
                        let target = WindowTarget {
                            app: app.clone(),
                            native_id: id(window),
                            generation: marker(window, true)?,
                        };
                        self.windows.insert(target.native_id, target.clone());
                        self.validate(&target)?;
                        windows.push(target);
                    }
                }
                (app, windows)
            };
            if windows.is_empty() {
                return Err(Error::NativeFailure {
                    code: "app_has_no_closeable_windows".into(),
                    message: "The selected instance has no normal application window to close"
                        .into(),
                });
            }
            let inserted = self.retain_quit(&app)?;
            let posted = (|| {
                for target in &windows {
                    check(until)?;
                    let window = self.validate(target)?;
                    if !unsafe { IsWindowEnabled(window) }.as_bool() {
                        if dispatched {
                            break;
                        }
                        return Err(Error::NativeFailure {code:"modal_state_unproven".into(),message:"Close the selected instance's existing modal before requesting quit".into()});
                    }
                    let _mutation = input::mutation(until)?;
                    self.validate(target)?;
                    check(until)?;
                    unsafe { PostMessageW(window, WM_CLOSE, WPARAM(0), LPARAM(0)) }
                        .map_err(failed)?;
                    dispatched = true;
                }
                let settle = until.min(Instant::now() + Duration::from_millis(250));
                loop {
                    check(until)?;
                    let exited = self.pending_quit(&app).unwrap().process.exited()?;
                    let closed = exited
                        || windows.iter().all(|target| {
                            let window = hwnd(target.native_id);
                            let mut pid = 0;
                            unsafe {
                                GetWindowThreadProcessId(window, Some(&mut pid));
                            }
                            !unsafe { IsWindow(window) }.as_bool() || pid != target.app.pid
                        });
                    if (close_window && closed)
                        || (!close_window && exited)
                        || Instant::now() >= settle
                    {
                        let confirmed = if close_window { closed } else { exited };
                        return Ok(
                            json!({"dispatch":"dispatched","evidence":if confirmed {"confirmed"} else {"unverified"},
                            "postcondition":if confirmed {Some(if close_window {"window_closed"} else {"application_exited"})} else {None}}),
                        );
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            })();
            let confirmed = posted.as_ref().is_ok_and(|reply| {
                reply["postcondition"] == "application_exited" && reply["evidence"] == "confirmed"
            });
            if confirmed || (inserted && (!dispatched || close_window)) {
                // Retain only an unconfirmed application quit. A rejected close
                // and a confirmed exit need no new persistent native resource.
                self.pending_quits.retain(|pending| pending.app != app);
            }
            posted
        })();
        match result {
            Ok(value) => value,
            Err(error) => {
                let mut reply = windows_error_reply(error, true);
                if dispatched {
                    reply["dispatch"] = json!("unknown");
                }
                reply
            }
        }
    }
}
