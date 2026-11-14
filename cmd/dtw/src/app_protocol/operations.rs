//! Canonical public app operations, schemas and static argument validation.
use super::wire;
use serde_json::{json, Value};

pub(super) struct Operation {
    pub name: &'static str,
    pub description: &'static str,
    pub action: bool,
    schema: fn() -> Value,
    pub required: &'static [&'static str],
}

impl Operation {
    pub(super) fn schema(&self) -> Value {
        let mut schema = (self.schema)();
        schema["type"] = json!("object");
        schema["additionalProperties"] = json!(false);
        schema["required"] = json!(self.required);
        schema
    }
}

pub(super) fn find(name: &str) -> Option<&'static Operation> {
    ALL.iter().find(|operation| operation.name == name)
}

pub(super) fn metadata() -> String {
    serde_json::to_string(
        &ALL.iter()
            .map(|operation| json!({"name": operation.name, "action": operation.action}))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

pub(super) const ALL: &[Operation] = &[
    Operation {
        name: "look", action: false,
        description: "Inspect the app scene, optionally filtering by role/text, scope or observed target. DTW starts the app when needed.",
        schema: || json!({"properties":{"full":{"type":"boolean"},"role":{"type":"string","minLength":1,"maxLength":64},"text":{"type":"string","minLength":1,"maxLength":256},"scope":{"enum":["window","menu"]},"target":{"type":"string","minLength":1}},"not":{"required":["target","scope"]}}),
        required: &[],
    },
    Operation {
        name: "capture", action: false,
        description: "Capture the app scene as an image. DTW starts the app when needed.",
        schema: || json!({"properties":{"full":{"type":"boolean"}}}), required: &[],
    },
    Operation {
        name: "readText", action: false,
        description: "Read a bounded text page from an observed target ID.",
        schema: || json!({"properties":{"target":{"type":"string","minLength":1},"offset":{"type":"integer","minimum":0,"maximum":4294967295u64},"limit":{"type":"integer","minimum":2,"maximum":16384}}}),
        required: &["target"],
    },
    Operation {
        name: "wait", action: false,
        description: "Wait 1..10000 ms, or wait for a stable window or matching title/role/text. Do not combine ms with conditions.",
        schema: || json!({"properties":{"ms":{"type":"integer","minimum":1,"maximum":10000},"window":{"enum":["stable"]},"title":{"type":"string","minLength":1,"maxLength":256},"role":{"type":"string","minLength":1,"maxLength":256},"text":{"type":"string","minLength":1,"maxLength":256},"timeout_ms":timeout_schema()},"oneOf":[{"required":["ms"],"not":{"anyOf":[{"required":["window"]},{"required":["title"]},{"required":["role"]},{"required":["text"]},{"required":["timeout_ms"]}]}},{"not":{"required":["ms"]},"anyOf":[{"required":["window"]},{"required":["title"]},{"required":["role"]},{"required":["text"]}]}]}),
        required: &[],
    },
    Operation {
        name: "waitForNewWindow", action: false,
        description: "Wait for a new app window following input, using DTW window routing.",
        schema: || json!({"properties":{"timeout_ms":timeout_schema()}}), required: &[],
    },
    Operation {
        name: "click", action: true,
        description: "Click one observed target, semantic selector, or image x/y pair. Button/count apply only to image coordinates. Standard readback follows input.",
        schema: || {
            let mut schema = json!({"properties":{"target":{"type":"string","minLength":1},"selector":selector_schema(),"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"unit":{"enum":["fraction"]},"button":{"enum":["left","right"]},"count":{"type":"integer","enum":[1,2]}}});
            schema["oneOf"] = json!([
                {"required":["target"],"not":{"anyOf":[{"required":["selector"]},{"required":["x"]},{"required":["y"]},{"required":["unit"]},{"required":["button"]},{"required":["count"]}]}},
                {"required":["selector"],"not":{"anyOf":[{"required":["target"]},{"required":["x"]},{"required":["y"]},{"required":["unit"]},{"required":["button"]},{"required":["count"]}]}},
                {"required":["x","y"],"not":{"anyOf":[{"required":["target"]},{"required":["selector"]}]}}
            ]);
            schema
        }, required: &[],
    },
    Operation {
        name: "setValue", action: true,
        description: "Set a string value (at most 16 KiB UTF-8, no NUL; at most 48 KiB JSON encoded) on one observed target or semantic selector. Schema maxLength counts Unicode characters. Standard readback follows input.",
        schema: || json!({"properties":{"target":{"type":"string","minLength":1},"selector":selector_schema(),"value":{"type":"string","maxLength":16384}},"oneOf":[{"required":["target"],"not":{"required":["selector"]}},{"required":["selector"],"not":{"required":["target"]}}]}),
        required: &["value"],
    },
    Operation {
        name: "move", action: true,
        description: "Move the pointer to image coordinates. unit fraction uses coordinates from 0 inclusive to 1 exclusive.",
        schema: || json!({"properties":{"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"unit":{"enum":["fraction"]}}}), required: &["x","y"],
    },
    Operation {
        name: "drag", action: true,
        description: "Drag along 2..64 image points within one observed window. Standard readback follows input.",
        schema: || json!({"properties":{"path":{"type":"array","minItems":2,"maxItems":64,"items":{"type":"object","properties":{"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0}},"required":["x","y"],"additionalProperties":false}},"unit":{"enum":["fraction"]}}}), required: &["path"],
    },
    Operation {
        name: "typeText", action: true,
        description: "Type nonempty text (at most 16 KiB UTF-8, no NUL; at most 48 KiB JSON encoded) into the current editing surface or an observed target. Schema maxLength counts Unicode characters. Standard readback follows input.",
        schema: || json!({"properties":{"text":{"type":"string","minLength":1,"maxLength":16384},"target":{"type":"string","minLength":1}}}), required: &["text"],
    },
    Operation {
        name: "keypress", action: true,
        description: "Press a chord of 1..8 ASCII key names on the app. Standard readback follows input.",
        schema: || json!({"properties":{"keys":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"string","minLength":1,"maxLength":32,"pattern":"^[ -~]+$"}}}}), required: &["keys"],
    },
    Operation {
        name: "scroll", action: true,
        description: "Scroll by bounded integer deltas at the active window center or an image x/y pair. At least one delta must be nonzero. Standard readback follows input.",
        schema: || json!({"properties":{"delta_y":{"type":"integer","minimum":-100,"maximum":100},"delta_x":{"type":"integer","minimum":-100,"maximum":100},"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"unit":{"enum":["fraction"]}},"dependentRequired":{"x":["y"],"y":["x"],"unit":["x","y"]},"anyOf":[{"required":["delta_y"],"properties":{"delta_y":{"not":{"const":0}}}},{"required":["delta_x"],"properties":{"delta_x":{"not":{"const":0}}}}]}), required: &["delta_y"],
    },
    Operation {
        name: "focus", action: true,
        description: "Focus the app, starting it when required. Standard readback follows the operation.",
        schema: || json!({"properties":{}}), required: &[],
    },
    Operation {
        name: "quit", action: true,
        description: "Quit the authorized application through the public app facade.",
        schema: || json!({"properties":{}}), required: &[],
    },
];

fn timeout_schema() -> Value {
    json!({"type":"integer","minimum":1,"maximum":10000})
}

fn selector_schema() -> Value {
    json!({"type":"object","properties":{"role":{"type":"string","minLength":1,"maxLength":64},"text":{"type":"string","minLength":1,"maxLength":256},"scope":{"enum":["window","menu"]}},"anyOf":[{"required":["role"]},{"required":["text"]}],"additionalProperties":false})
}

pub(super) fn validate(op: &str, args: &Value) -> Result<Value, wire::WireError> {
    let operation = find(op).ok_or_else(|| error("invalid_arguments", "Unknown app operation"))?;
    let schema = operation.schema();
    let mut canonical = args.clone();
    // JSON Schema integers include 1.0/1e0. Match the JS bridge's JSON number
    // representation for declared integer fields before validation and execution.
    if let Some(object) = canonical.as_object_mut() {
        for (key, field) in schema["properties"].as_object().unwrap() {
            if field["type"] != "integer" {
                continue;
            }
            if let Some(value) = object.get_mut(key).filter(|value| value.is_f64()) {
                let number = value.as_f64().unwrap();
                if number.is_finite()
                    && number.fract() == 0.0
                    && number.abs() <= 9_007_199_254_740_991.0
                {
                    *value = json!(number as i64);
                }
            }
        }
    }
    let args = &canonical;
    let object = args
        .as_object()
        .ok_or_else(|| error("invalid_arguments", "Expected one argument object"))?;
    let properties = schema["properties"].as_object().unwrap();
    if object.keys().any(|key| !properties.contains_key(key)) {
        return Err(error(
            "invalid_arguments",
            "Unknown app argument; see Skill API.md",
        ));
    }
    if operation
        .required
        .iter()
        .any(|key| !object.contains_key(*key))
    {
        return Err(error("invalid_arguments", "Missing required app argument"));
    }
    if args.get("unit").is_some_and(|unit| {
        unit != "fraction" || (op != "drag" && !(args["x"].is_number() && args["y"].is_number()))
    }) {
        return Err(error(
            "invalid_arguments",
            "unit:'fraction' requires image coordinates",
        ));
    }
    if op == "setValue" {
        let one_target = usize::from(args.get("target").is_some())
            + usize::from(args.get("selector").is_some())
            == 1;
        if !one_target {
            return Err(error(
                "invalid_arguments",
                "setValue requires one target or selector",
            ));
        }
        native(wire::text_value(object, "value", true))?;
    }

    if op == "typeText"
        && args
            .get("target")
            .is_some_and(|target| target.as_str().is_none_or(str::is_empty))
    {
        return Err(error(
            "invalid_arguments",
            "typeText target must be an observed element ID",
        ));
    }
    if op == "wait" {
        if let Some(ms) = args.get("ms") {
            if object.len() == 1 && ms.as_u64().is_some_and(|ms| (1..=10_000).contains(&ms)) {
                return Ok(canonical);
            }
            return Err(error(
                "invalid_arguments",
                "wait(ms) requires 1..10000 milliseconds",
            ));
        }
        if args.get("window").is_some_and(|v| v != "stable")
            || !["window", "title", "role", "text"]
                .iter()
                .any(|key| args.get(key).is_some())
            || ["title", "role", "text"].iter().any(|key| {
                args.get(key).is_some_and(|v| {
                    v.as_str()
                        .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 256)
                })
            })
        {
            return Err(error(
                "invalid_arguments",
                "wait requires window:'stable', title, role or text",
            ));
        }
    }
    if matches!(op, "wait" | "waitForNewWindow")
        && args
            .get("timeout_ms")
            .is_some_and(|v| v.as_u64().is_none_or(|ms| !(1..=10_000).contains(&ms)))
    {
        return Err(error(
            "invalid_arguments",
            "timeout_ms must be an integer from 1 to 10000",
        ));
    }
    for key in ["full"] {
        if args.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(error(
                "invalid_arguments",
                "Observation flags must be Boolean",
            ));
        }
    }
    if op == "look" {
        if args
            .get("target")
            .is_some_and(|target| target.as_str().is_none_or(str::is_empty))
            || (args.get("target").is_some() && args.get("scope").is_some())
        {
            return Err(error(
                "invalid_arguments",
                "look target must be an observed ID and cannot be combined with scope",
            ));
        }
        if args
            .get("scope")
            .is_some_and(|scope| !matches!(scope.as_str(), Some("window" | "menu")))
        {
            return Err(error(
                "invalid_arguments",
                "look scope must be window or menu",
            ));
        }
        for (key, maximum) in [("role", 64), ("text", 256)] {
            if args.get(key).is_some_and(|value| {
                value
                    .as_str()
                    .is_none_or(|text| text.is_empty() || text.chars().count() > maximum)
            }) {
                return Err(error(
                    "invalid_arguments",
                    "Observation role/text must be bounded nonempty strings",
                ));
            }
        }
    }
    if op == "readText" {
        if args["target"]
            .as_str()
            .is_none_or(|target| target.is_empty())
        {
            return Err(error(
                "invalid_arguments",
                "readText requires an observed target ID",
            ));
        }
        for (key, minimum, maximum) in [("offset", 0, u32::MAX as u64), ("limit", 2, 16384)] {
            if args.get(key).is_some_and(|value| {
                value
                    .as_u64()
                    .is_none_or(|number| number < minimum || number > maximum)
            }) {
                let end_hint = if key == "offset" && args[key].is_null() {
                    "; next_offset:null means the text has ended"
                } else {
                    ""
                };
                return Err(error(
                    "invalid_arguments",
                    &format!(
                        "readText {key} must be an integer from {minimum} to {maximum}{end_hint}"
                    ),
                ));
            }
        }
    }
    if matches!(op, "click" | "setValue")
        && object.contains_key("target")
        && args["target"].as_str().is_none_or(str::is_empty)
    {
        return Err(error(
            "invalid_arguments",
            "target must be a nonempty observed ID",
        ));
    }
    if object.contains_key("selector") {
        native(wire::selection(object))?;
    }
    match op {
        "typeText" => {
            native(wire::text_value(object, "text", false))?;
        }
        "keypress" => {
            native(wire::key_names(object))?;
        }
        "click" => {
            let target = object.contains_key("target");
            let selector = object.contains_key("selector");
            let coordinates = object.contains_key("x") || object.contains_key("y");
            if usize::from(target) + usize::from(selector) + usize::from(coordinates) != 1 {
                return Err(error(
                    "invalid_arguments",
                    "click requires exactly one target, selector or x/y pair",
                ));
            }
            if coordinates {
                point(args, args.get("unit").is_some())?;
                native(wire::click_options(object))?;
            } else if ["unit", "button", "count"]
                .iter()
                .any(|key| object.contains_key(*key))
            {
                return Err(error(
                    "invalid_arguments",
                    "Mouse options require image coordinates",
                ));
            }
        }
        "move" => {
            point(args, args.get("unit").is_some())?;
        }
        "drag" => {
            let points = args["path"]
                .as_array()
                .filter(|points| (2..=64).contains(&points.len()))
                .ok_or_else(|| error("invalid_arguments", "drag requires 2..64 points"))?;
            let point_schema = &schema["properties"]["path"]["items"];
            let keys = point_schema["properties"].as_object().unwrap();
            let mut first = None;
            let mut moved = false;
            for supplied in points {
                if supplied.as_object().is_none_or(|p| {
                    p.len() != keys.len() || p.keys().any(|key| !keys.contains_key(key))
                }) {
                    return Err(error(
                        "invalid_arguments",
                        "Each path point has exactly x and y",
                    ));
                }
                let position = point(supplied, args.get("unit").is_some())?;
                if let Some(origin) = first {
                    moved |= position != origin;
                } else {
                    first = Some(position);
                }
            }
            if !moved {
                return Err(error("invalid_arguments", "A drag must contain movement"));
            }
        }
        "scroll" => {
            native(wire::scroll_deltas(object))?;
            if object.contains_key("x") || object.contains_key("y") {
                point(args, args.get("unit").is_some())?;
            }
        }
        _ => {}
    }
    Ok(canonical)
}

fn error(code: &str, message: &str) -> wire::WireError {
    wire::WireError {
        code: code.into(),
        message: message.into(),
        details: None,
    }
}

fn native<T>(result: Result<T, wire::WireError>) -> Result<T, wire::WireError> {
    result.map_err(|mut error| {
        error.code = "invalid_arguments".into();
        error
    })
}

fn point(args: &Value, fraction: bool) -> Result<(f64, f64), wire::WireError> {
    let coordinate = |key: &str| {
        args[key].as_f64().filter(|value| value.is_finite() && *value >= 0.0 && (!fraction || *value < 1.0))
        .ok_or_else(|| error("invalid_arguments", "Image coordinates must be finite and nonnegative; fractions must be less than 1"))
    };
    Ok((coordinate("x")?, coordinate("y")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_closes_objects_and_enforces_required_fields() {
        let metadata: Value = serde_json::from_str(&metadata()).unwrap();
        assert_eq!(metadata.as_array().unwrap().len(), ALL.len());
        for operation in ALL {
            let schema = operation.schema();
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            assert_eq!(schema["required"], json!(operation.required));
            assert!(schema["properties"].get("app").is_none());
            assert_eq!(find(operation.name).unwrap().name, operation.name);
            assert_eq!(
                validate(operation.name, &json!({"unknown":true}))
                    .unwrap_err()
                    .code,
                "invalid_arguments"
            );
            for required in operation.required {
                let mut args = schema["properties"].as_object().unwrap().clone();
                args.remove(*required);
                assert_eq!(
                    validate(operation.name, &Value::Object(args))
                        .unwrap_err()
                        .message,
                    "Missing required app argument"
                );
            }
        }
        assert!(find("after").is_none());
        assert!(find("read_text").is_none());
    }

    #[test]
    fn rejects_invalid_canonical_shapes_before_routing() {
        for (name, args) in [
            ("click", json!({})),
            ("click", json!({"target":"e1","x":0,"y":0})),
            ("click", json!({"target":"e1","count":2})),
            ("click", json!({"x":0})),
            ("click", json!({"x":0,"y":0,"button":"middle"})),
            (
                "click",
                json!({"selector":{"role":"button","unknown":true}}),
            ),
            ("setValue", json!({"selector":{},"value":""})),
            ("move", json!({"x":-1,"y":0})),
            ("move", json!({"x":1,"y":0,"unit":"fraction"})),
            ("drag", json!({"path":[{"x":0,"y":0},{"x":0,"y":0}]})),
            (
                "drag",
                json!({"path":[{"x":0,"y":0},{"x":1,"y":1,"extra":0}]}),
            ),
            ("keypress", json!({"keys":["é"]})),
            ("typeText", json!({"text":"\u{0}"})),
            ("scroll", json!({"delta_x":1})),
            ("scroll", json!({"delta_y":0})),
            ("scroll", json!({"delta_y":101})),
            ("scroll", json!({"delta_y":1.5})),
            ("scroll", json!({"delta_y":1,"x":0})),
            ("scroll", json!({"delta_y":1,"unit":"fraction"})),
            ("wait", json!({"ms":1,"timeout_ms":2})),
        ] {
            assert_eq!(
                validate(name, &args).unwrap_err().code,
                "invalid_arguments",
                "{name}: {args}"
            );
        }
    }

    #[test]
    fn accepts_canonical_boundaries_and_applies_native_text_limits() {
        for (name, args) in [
            (
                "click",
                json!({"selector":{"role":"button","scope":"menu"}}),
            ),
            ("click", json!({"x":0,"y":0,"button":"right","count":2})),
            (
                "drag",
                json!({"path":[{"x":0,"y":0},{"x":0.9,"y":0.9}],"unit":"fraction"}),
            ),
            ("scroll", json!({"delta_y":0,"delta_x":-100})),
            ("scroll", json!({"delta_y":100,"x":0,"y":0})),
            ("setValue", json!({"target":"e1","value":""})),
            ("typeText", json!({"text":"x".repeat(16_384)})),
            ("wait", json!({"text":"é".repeat(256)})),
        ] {
            validate(name, &args).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
        // Schema maxLength counts characters; native transport additionally bounds UTF-8 bytes.
        assert!(validate("typeText", &json!({"text":"é".repeat(8193)})).is_err());
        assert!(validate("typeText", &json!({"text":"\u{1}".repeat(8192)})).is_err());
    }
}
