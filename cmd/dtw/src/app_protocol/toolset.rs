//! Selectable MCP tools; atomic calls remain ordinary public-facade scripts.
use super::{app_id, mcp, scene, scripts, wire};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Toolset {
    #[default]
    Script,
    Atomic,
    All,
}

impl Toolset {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "script" => Some(Self::Script),
            "atomic" => Some(Self::Atomic),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub(super) fn allows(self, name: &str) -> bool {
        name == "apps"
            || (name == "exec" && self != Self::Atomic)
            || (self != Self::Script && TOOLS.iter().any(|tool| tool.name == name))
    }

    pub(super) fn tools(self) -> Value {
        let mut tools = mcp::tools();
        let list = tools.as_array_mut().expect("static MCP tool array");
        list.retain(|tool| self.allows(tool["name"].as_str().unwrap()));
        if self != Self::Script {
            for tool in TOOLS {
                let mut schema = (tool.schema)();
                schema["type"] = json!("object");
                schema["additionalProperties"] = json!(false);
                schema["properties"]["app"] = app_id::schema();
                let mut required = vec!["app"];
                required.extend_from_slice(tool.required);
                schema["required"] = json!(required);
                list.push(json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": schema,
                    "annotations": {"readOnlyHint":false,"destructiveHint":true,
                        "idempotentHint":false,"openWorldHint":true}
                }));
            }
        }
        tools
    }

    pub(super) fn atomic_source(
        self,
        name: &str,
        args: &Value,
    ) -> Result<Option<(String, String)>, wire::WireError> {
        let Some(tool) = TOOLS.iter().find(|tool| tool.name == name) else {
            return Ok(None);
        };
        if !self.allows(name) {
            return Ok(None);
        }
        let mut options = args
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("Expected one argument object"))?;
        let app = app_id::parse(
            &options
                .remove("app")
                .ok_or_else(|| invalid("app is required"))?,
        )?;
        let options = Value::Object(options);
        scene::validate(&format!("scene.{}", tool.method), &options)?;
        // Parse serialized JSON instead of a JS object literal: __proto__ stays
        // an own data property, and arbitrary strings never become source code.
        let encoded = serde_json::to_string(&options).expect("JSON arguments");
        let literal = serde_json::to_string(&encoded).expect("JSON string");
        // Actions publish the existing automatic view and effects summary.
        // Only text pagination needs a custom result instead of that view.
        let prefix = if tool.method == "readText" {
            "return "
        } else {
            ""
        };
        let source = format!("{prefix}await app.{}(JSON.parse({literal}));", tool.method);
        if source.len() > scripts::MAX_SOURCE_BYTES {
            return Err(invalid("Generated source exceeds 64 KiB of UTF-8"));
        }
        Ok(Some((app, source)))
    }
}

fn invalid(message: &str) -> wire::WireError {
    wire::WireError {
        code: "invalid_arguments".into(),
        message: message.into(),
        details: None,
    }
}

struct Tool {
    name: &'static str,
    method: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    required: &'static [&'static str],
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "look", method: "look",
        description: "Inspect the app scene, optionally filtering by role/text, scope or observed target. DTW starts the app when needed.",
        schema: || json!({"properties":{"full":{"type":"boolean"},"role":{"type":"string","minLength":1,"maxLength":64},"text":{"type":"string","minLength":1,"maxLength":256},"scope":{"enum":["window","menu"]},"target":{"type":"string","minLength":1}},"not":{"required":["target","scope"]}}),
        required: &[],
    },
    Tool {
        name: "capture", method: "capture",
        description: "Capture the app scene as an image. DTW starts the app when needed.",
        schema: || json!({"properties":{"full":{"type":"boolean"}}}), required: &[],
    },
    Tool {
        name: "read_text", method: "readText",
        description: "Read a bounded text page from an observed target ID.",
        schema: || json!({"properties":{"target":{"type":"string","minLength":1},"offset":{"type":"integer","minimum":0,"maximum":4294967295u64},"limit":{"type":"integer","minimum":2,"maximum":16384}}}),
        required: &["target"],
    },
    Tool {
        name: "wait", method: "wait",
        description: "Wait 1..10000 ms, or wait for a stable window or matching title/role/text. Do not combine ms with conditions.",
        schema: || json!({"properties":{"ms":{"type":"integer","minimum":1,"maximum":10000},"window":{"enum":["stable"]},"title":{"type":"string","minLength":1,"maxLength":256},"role":{"type":"string","minLength":1,"maxLength":256},"text":{"type":"string","minLength":1,"maxLength":256},"timeout_ms":timeout_schema()},"oneOf":[{"required":["ms"],"not":{"anyOf":[{"required":["window"]},{"required":["title"]},{"required":["role"]},{"required":["text"]},{"required":["timeout_ms"]}]}},{"not":{"required":["ms"]},"anyOf":[{"required":["window"]},{"required":["title"]},{"required":["role"]},{"required":["text"]}]}]}),
        required: &[],
    },
    Tool {
        name: "wait_for_new_window", method: "waitForNewWindow",
        description: "Wait for a new app window following input, using DTW window routing.",
        schema: || json!({"properties":{"timeout_ms":timeout_schema()}}), required: &[],
    },
    Tool {
        name: "click", method: "click",
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
    Tool {
        name: "set_value", method: "setValue",
        description: "Set a string value on one observed target or semantic selector. Standard readback follows input.",
        schema: || json!({"properties":{"target":{"type":"string","minLength":1},"selector":selector_schema(),"value":{"type":"string","maxLength":16384}},"oneOf":[{"required":["target"],"not":{"required":["selector"]}},{"required":["selector"],"not":{"required":["target"]}}]}),
        required: &["value"],
    },
    Tool {
        name: "move", method: "move",
        description: "Move the pointer to image coordinates. unit fraction uses coordinates from 0 inclusive to 1 exclusive.",
        schema: || json!({"properties":{"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"unit":{"enum":["fraction"]}}}), required: &["x","y"],
    },
    Tool {
        name: "drag", method: "drag",
        description: "Drag along 2..64 image points within one observed window. Standard readback follows input.",
        schema: || json!({"properties":{"path":{"type":"array","minItems":2,"maxItems":64,"items":{"type":"object","properties":{"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0}},"required":["x","y"],"additionalProperties":false}},"unit":{"enum":["fraction"]}}}), required: &["path"],
    },
    Tool {
        name: "type_text", method: "typeText",
        description: "Type nonempty text into the current editing surface or an observed target. Standard readback follows input.",
        schema: || json!({"properties":{"text":{"type":"string","minLength":1,"maxLength":16384},"target":{"type":"string","minLength":1}}}), required: &["text"],
    },
    Tool {
        name: "keypress", method: "keypress",
        description: "Press a chord of 1..8 ASCII key names on the app. Standard readback follows input.",
        schema: || json!({"properties":{"keys":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"string","minLength":1,"maxLength":32,"pattern":"^[ -~]+$"}}}}), required: &["keys"],
    },
    Tool {
        name: "scroll", method: "scroll",
        description: "Scroll by bounded integer deltas at the active window center or an image x/y pair. At least one delta must be nonzero. Standard readback follows input.",
        schema: || json!({"properties":{"delta_y":{"type":"integer","minimum":-100,"maximum":100},"delta_x":{"type":"integer","minimum":-100,"maximum":100},"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"unit":{"enum":["fraction"]}},"dependentRequired":{"x":["y"],"y":["x"],"unit":["x","y"]},"anyOf":[{"required":["delta_y"],"properties":{"delta_y":{"not":{"const":0}}}},{"required":["delta_x"],"properties":{"delta_x":{"not":{"const":0}}}}]}), required: &["delta_y"],
    },
    Tool {
        name: "focus", method: "focus",
        description: "Focus the app, starting it when required. Standard readback follows the operation.",
        schema: || json!({"properties":{}}), required: &[],
    },
    Tool {
        name: "quit", method: "quit",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_list_only_allowed_tools() {
        assert_eq!(Toolset::default(), Toolset::Script);
        assert_eq!(Toolset::Script.tools(), mcp::tools());
        for (mode, count) in [
            (Toolset::Script, 2),
            (Toolset::Atomic, 15),
            (Toolset::All, 16),
        ] {
            let tools = mode.tools();
            assert_eq!(tools.as_array().unwrap().len(), count);
            for tool in tools.as_array().unwrap() {
                assert!(mode.allows(tool["name"].as_str().unwrap()));
            }
        }
        assert_eq!(Toolset::parse("atomic"), Some(Toolset::Atomic));
        assert_eq!(Toolset::parse("Atomic"), None);
        assert!(!Toolset::All.allows("scene.click"));
        assert!(Toolset::Script
            .atomic_source("click", &Value::Null)
            .unwrap()
            .is_none());
    }

    #[test]
    fn source_keeps_untrusted_text_inside_serialized_json() {
        let text = "\"); app.quit(); //\n\\\"__proto__\"\u{2028}";
        let (_, source) = Toolset::Atomic
            .atomic_source("type_text", &json!({"app":"example.app","text":text}))
            .unwrap()
            .unwrap();
        let literal = source
            .strip_prefix("await app.typeText(JSON.parse(")
            .unwrap()
            .strip_suffix("));")
            .unwrap();
        let encoded: String = serde_json::from_str(literal).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&encoded).unwrap(),
            json!({"text":text})
        );
        for args in [
            json!({"app":"example.app","unknown":true}),
            json!({"app":"example.app","__proto__":{}}),
        ] {
            assert!(Toolset::Atomic.atomic_source("look", &args).is_err());
        }
    }
}
