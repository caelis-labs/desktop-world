//! Small, explicit observation routes. Native evidence stays in the receipt;
//! only its model-facing presentation is projected, filtered and bounded.
use crate::SavedResult;
use serde_json::{json, Value};

pub fn allowed(name: &str) -> bool {
    matches!(name, "apps" | "windows" | "observe")
}

pub fn description(name: &str) -> Value {
    let properties = match name {
        "apps" => json!({
            "running":{"type":"boolean","default":true,"description":"Only running apps by default. False includes installed apps too."},
            "query":{"type":"string","description":"Case-insensitive substring in name or bundle ID."},
            "offset":{"type":"integer","minimum":0,"default":0},
            "limit":{"type":"integer","minimum":1,"maximum":100,"default":20}
        }),
        "windows" => json!({
            "pid":{"type":"integer","minimum":1},
            "app":{"type":"string","description":"CUA app name/bundle filter; use either app or pid."},
            "title":{"type":"string","description":"Case-insensitive title substring; lists all matches, never selects an arbitrary one."},
            "on_screen_only":{"type":"boolean","default":false},
            "offset":{"type":"integer","minimum":0,"default":0},
            "limit":{"type":"integer","minimum":1,"maximum":100,"default":20}
        }),
        _ => json!({
            "pid":{"type":"integer","minimum":1},
            "window_id":{"type":"integer","minimum":1},
            "query":{"type":"string"},
            "max_elements":{"type":"integer","minimum":1,"maximum":2000,"default":250},
            "max_depth":{"type":"integer","minimum":1,"maximum":100,"default":25},
            "include_screenshot":{"type":"boolean","default":false},
            "max_image_dimension":{"type":"integer","minimum":0,"default":1600},
            "timeout_ms":{"type":"integer","minimum":1,"maximum":30000}
        }),
    };
    let required = if name == "observe" {
        json!(["pid", "window_id"])
    } else {
        json!([])
    };
    json!({"operation":name,"arguments_schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
    "usage":match name {
        "apps" => "Discover running apps with compact records. Filtering is after native enumeration; it does not reduce upstream scan cost. next_call reads a new live page; result,part=structured preserves this receipt's native inventory.",
        "windows" => "Require pid or app. Inspect all candidates before choosing an exact window_id. layer_hint describes OS stacking only, not an AX role or proof of a main window. Pagination re-enumerates live state.",
        _ => "Read one exact window. The summary has one tree plus snapshot/diagnostic metadata; full native evidence and elements are in result,part=structured. Every AX read may invalidate previous element tokens. No readiness retry or foreground input."
    }})
}

pub struct Request {
    name: String,
    args: Value,
}

impl Request {
    pub fn parse(name: &str, args: Option<&Value>) -> Result<Self, String> {
        let args = args.cloned().unwrap_or_else(|| json!({}));
        let object = args.as_object().ok_or("arguments must be an object")?;
        let schema = description(name);
        let properties = &schema["arguments_schema"]["properties"];
        for (key, value) in object {
            let rule = &properties[key];
            let valid = match rule["type"].as_str() {
                Some("boolean") => value.is_boolean(),
                Some("string") => value
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= 512),
                Some("integer") => value.as_u64().is_some_and(|n| {
                    n >= rule["minimum"].as_u64().unwrap_or(0)
                        && n <= rule["maximum"].as_u64().unwrap_or(if key == "pid" {
                            i32::MAX as u64
                        } else {
                            u64::MAX
                        })
                }),
                _ => false,
            };
            if !valid {
                return Err(format!("invalid or unsupported {name} argument: {key}"));
            }
        }
        if name == "windows" && (object.contains_key("pid") == object.contains_key("app")) {
            return Err("windows requires exactly one of pid or app".into());
        }
        if name == "observe" && (!object.contains_key("pid") || !object.contains_key("window_id")) {
            return Err("observe requires exact pid and window_id".into());
        }
        Ok(Self {
            name: name.into(),
            args,
        })
    }

    pub fn tool(&self) -> &'static str {
        match self.name.as_str() {
            "apps" => "list_apps",
            "windows" => "list_windows",
            _ => "get_window_state",
        }
    }

    pub fn native_arguments(&self) -> Value {
        match self.name.as_str() {
            "apps" => json!({}),
            "windows" => {
                let mut args = self.args.clone();
                for key in ["offset", "limit", "title"] {
                    args.as_object_mut().unwrap().remove(key);
                }
                args
            }
            _ => {
                let mut args = self.args.clone();
                let object = args.as_object_mut().unwrap();
                object.insert("tree_format".into(), json!("both"));
                object.insert("verbose".into(), json!(true));
                object.entry("max_elements").or_insert(json!(250));
                object.entry("max_depth").or_insert(json!(25));
                object.entry("include_screenshot").or_insert(json!(false));
                object.entry("max_image_dimension").or_insert(json!(1600));
                args
            }
        }
    }

    pub fn finish(&self, mut result: SavedResult) -> SavedResult {
        if result.is_error {
            return result;
        }
        let Ok(native) = serde_json::from_str::<Value>(&result.structured) else {
            return result;
        };
        if self.name != "observe" {
            let Some(items) = native[&self.name].as_array() else {
                return result;
            };
            let query = self.args[if self.name == "apps" {
                "query"
            } else {
                "title"
            }]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
            let mut filtered: Vec<_> = items
                .iter()
                .filter(|item| {
                    if self.name == "apps" {
                        (!self.args["running"].as_bool().unwrap_or(true) || item["running"] == true)
                            && ["name", "bundle_id"].iter().any(|key| {
                                item[*key]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_lowercase()
                                    .contains(&query)
                            })
                    } else {
                        item["title"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query)
                    }
                })
                .collect();
            // Deterministic ordering makes small live pages useful. Never infer
            // foreground order from this list: window z_index remains explicit.
            filtered.sort_by_key(|v| {
                (
                    v["name"].as_str().unwrap_or("").to_lowercase(),
                    v["pid"].as_u64(),
                    v["window_id"].as_u64(),
                )
            });
            let offset = self.args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = self.args["limit"].as_u64().unwrap_or(20) as usize;
            let mut rows = Vec::new();
            let mut bytes = 0;
            for item in filtered.iter().skip(offset).take(limit) {
                let mut row = json!({});
                let fields: &[&str] = if self.name == "apps" {
                    &["pid", "name", "bundle_id", "running", "active"]
                } else {
                    &[
                        "pid",
                        "window_id",
                        "app_name",
                        "title",
                        "bounds",
                        "layer",
                        "z_index",
                        "is_on_screen",
                        "minimized",
                        "on_current_space",
                    ]
                };
                for field in fields {
                    if let Some(value) = item.get(*field) {
                        row[*field] = value.clone();
                    }
                }
                if self.name == "windows" {
                    row["layer_hint"] = json!(match item["layer"].as_i64() {
                        Some(0) => "ordinary_layer",
                        Some(n) if n > 0 => "elevated_layer",
                        _ => "unknown",
                    });
                }
                // Bound individual labels as well as page size; raw values remain
                // available. Do not shorten identities or actionable IDs.
                for field in ["name", "title", "app_name"] {
                    if let Some(label) = row[field].as_str().filter(|s| s.chars().count() > 160) {
                        let label = format!("{}…", label.chars().take(160).collect::<String>());
                        row[field] = json!(label);
                        row["label_truncated"] = json!(true);
                    }
                }
                bytes += row.to_string().len();
                if bytes > 8000 && !rows.is_empty() {
                    break;
                }
                rows.push(row);
            }
            let next = offset.saturating_add(rows.len());
            let mut page = json!({"items":rows,"total":filtered.len(),"offset":offset,"returned":rows.len(),"pagination":"live_reenumeration"});
            if next < filtered.len() {
                let mut args = self.args.clone();
                args["offset"] = json!(next);
                page["next_call"] = json!({"operation":self.name,"arguments":args});
            }
            result.structured = json!({"page":page,"native":native}).to_string();
        }
        result.native_operation = Some(self.name.clone());
        result
    }
}

pub fn presentation(name: &str, result: &SavedResult, id: &str) -> Value {
    let native = serde_json::from_str::<Value>(&result.structured).unwrap_or(Value::Null);
    let mut view = if name == "observe" {
        native
    } else {
        native["page"].clone()
    };
    let more = json!({"operation":"result","id":id,"part":"structured"});
    if let Some(object) = view.as_object_mut() {
        if name == "observe" {
            // This is a known native get_window_state receipt, never arbitrary
            // user-returned JSON. Keep one human-readable tree and diagnostics.
            object.remove("text");
            object.remove("elements");
            object.remove("_note");
            if let Some(tree) = object
                .get("tree_markdown")
                .and_then(Value::as_str)
                .filter(|s| s.len() > 6000)
            {
                let mut end = 6000;
                while !tree.is_char_boundary(end) {
                    end -= 1;
                }
                let tree = format!("{}…", &tree[..end]);
                object.insert("tree_markdown".into(), json!(tree));
                object.insert("dtw_tree_truncated".into(), json!(true));
            }
        }
        object.insert("more".into(), more.clone());
    }
    if view.to_string().len() > 12 * 1024 {
        // Unknown large diagnostic fields are not silently discarded.
        return json!({"pid":view["pid"],"window_id":view["window_id"],"snapshot_id":view["snapshot_id"],"dtw_truncated":true,"more":more});
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result(value: Value) -> SavedResult {
        SavedResult {
            text: "native evidence".into(),
            structured: value.to_string(),
            images: vec![],
            is_error: false,
            error_code: None,
            native_operation: None,
        }
    }
    #[test]
    fn discovery_filters_and_pages_without_losing_native_inventory() {
        let apps: Vec<_> = (1..=200)
            .map(|n| json!({"pid":n,"name":format!("App {n:03}"),"running":n%2==0,"active":false}))
            .collect();
        let request = Request::parse("apps", Some(&json!({"query":"app", "limit":3}))).unwrap();
        let saved = request.finish(result(json!({"apps":apps})));
        let page = presentation("apps", &saved, "receipt");
        assert_eq!(page["total"], 100);
        assert_eq!(page["items"][0]["pid"], 2);
        assert_eq!(page["returned"], 3);
        assert_eq!(page["next_call"]["arguments"]["offset"], 3);
        let raw: Value = serde_json::from_str(&saved.structured).unwrap();
        assert_eq!(raw["native"]["apps"].as_array().unwrap().len(), 200);
        let next = Request::parse("apps", Some(&page["next_call"]["arguments"]))
            .unwrap()
            .finish(result(raw["native"].clone()));
        assert_eq!(presentation("apps", &next, "next")["items"][0]["pid"], 8);
    }
    #[test]
    fn observation_preserves_large_window_ids_and_bounded_native_walk() {
        let request =
            Request::parse("observe", Some(&json!({"pid":1,"window_id":4294967296u64}))).unwrap();
        let args = request.native_arguments();
        assert_eq!(args["window_id"], 4294967296u64);
        assert_eq!(args["tree_format"], "both");
        assert_eq!(args["max_elements"], 250);
        assert_eq!(args["max_depth"], 25);
        assert_eq!(args["include_screenshot"], false);
        assert!(args.get("full_output").is_none());
    }
    #[test]
    fn windows_preserve_ambiguity_and_raw_titles() {
        let request =
            Request::parse("windows", Some(&json!({"pid":1,"title":"settings"}))).unwrap();
        let saved=request.finish(result(json!({"windows":[{"pid":1,"window_id":20,"title":"Settings","layer":0},{"pid":1,"window_id":21,"title":"Settings","layer":8}]})));
        let view = presentation("windows", &saved, "r");
        assert_eq!(view["total"], 2);
        assert_eq!(view["items"][1]["layer_hint"], "elevated_layer");
        assert_eq!(request.native_arguments(), json!({"pid":1}));
    }
    #[test]
    fn observation_summary_deduplicates_only_explicit_route_and_keeps_evidence() {
        let native = json!({"pid":1,"window_id":2,"snapshot_id":"abc","text":"same tree","tree_markdown":"same tree","elements":[{"element_token":"abc:1"}],"degraded_reason":"ax_app_launching","truncated":true});
        let saved = Request::parse("observe", Some(&json!({"pid":1,"window_id":2})))
            .unwrap()
            .finish(result(native.clone()));
        let view = presentation("observe", &saved, "r");
        assert!(view.get("text").is_none() && view.get("elements").is_none());
        assert_eq!(view["tree_markdown"], "same tree");
        assert_eq!(view["degraded_reason"], native["degraded_reason"]);
        assert_eq!(
            serde_json::from_str::<Value>(&saved.structured).unwrap(),
            native
        );
    }
    #[test]
    fn invalid_inputs_never_become_unscoped_native_reads() {
        for (name, args) in [
            ("windows", json!({})),
            ("windows", json!({"pid":1,"app":"Example"})),
            ("observe", json!({"pid":1})),
            ("apps", json!({"running":"false"})),
            ("apps", json!({"limit":0})),
            ("apps", json!({"session":"other"})),
            ("observe", json!({"pid":1,"window_id":0})),
        ] {
            assert!(Request::parse(name, Some(&args)).is_err(), "{name}: {args}");
        }
    }
    #[test]
    fn large_labels_and_trees_stay_bounded_and_utf8_safe() {
        let apps: Vec<_> = (1..=100)
            .map(|pid| json!({"pid":pid,"name":"中".repeat(400),"running":true}))
            .collect();
        let saved = Request::parse("apps", Some(&json!({"limit":100})))
            .unwrap()
            .finish(result(json!({"apps":apps})));
        let view = presentation("apps", &saved, "r");
        assert!(view.to_string().len() < 12000);
        assert!(view["returned"].as_u64().unwrap() < 100);
        assert_eq!(view["next_call"]["arguments"]["offset"], view["returned"]);
        let saved = result(json!({"pid":1,"window_id":2,"tree_markdown":"中".repeat(10000)}));
        let view = presentation("observe", &saved, "r");
        assert_eq!(view["dtw_tree_truncated"], true);
        assert!(view.to_string().len() < 12000);
    }
}
