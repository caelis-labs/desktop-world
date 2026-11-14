//! Host-selected MCP surface. Public operations share the script facade contract.
use super::{app_id, mcp, operations, scripts, wire};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Toolset {
    #[default]
    Script,
    Atomic,
}

impl Toolset {
    pub(super) fn allows(self, name: &str) -> bool {
        name == "apps"
            || match self {
                Self::Script => name == "exec",
                Self::Atomic => operations::find(name).is_some(),
            }
    }

    pub(super) fn tools(self) -> Value {
        let mut tools = mcp::tools();
        if self == Self::Atomic {
            let list = tools.as_array_mut().expect("static MCP tool array");
            list.retain(|tool| tool["name"] == "apps");
            for operation in operations::ALL {
                let mut schema = operation.schema();
                schema["properties"]["app"] = app_id::schema();
                schema["required"]
                    .as_array_mut()
                    .unwrap()
                    .insert(0, json!("app"));
                list.push(json!({
                    "name":operation.name,
                    "description":operation.description,
                    "inputSchema":schema,
                    // Reads can start a stopped app; these are conservative hints.
                    "annotations":{"readOnlyHint":false,"destructiveHint":true,
                        "idempotentHint":false,"openWorldHint":true}
                }));
            }
        }
        tools
    }

    pub(super) fn atomic_call(
        self,
        name: &str,
        args: &Value,
    ) -> Result<Option<(String, wire::Command)>, wire::WireError> {
        if self != Self::Atomic {
            return Ok(None);
        }
        let Some(operation) = operations::find(name) else {
            return Ok(None);
        };
        let mut options = args
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("Expected one argument object"))?;
        let app = app_id::parse(
            &options
                .remove("app")
                .ok_or_else(|| invalid("app is required"))?,
        )?;
        let options = operations::validate(operation.name, &Value::Object(options))?;
        Ok(Some((
            app,
            wire::Command::App {
                method: operation.name.into(),
                args: options,
                timeout_ms: scripts::DEFAULT_TIMEOUT_MS,
            },
        )))
    }
}

fn invalid(message: &str) -> wire::WireError {
    wire::WireError {
        code: "invalid_arguments".into(),
        message: message.into(),
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_list_only_allowed_tools() {
        assert_eq!(Toolset::default(), Toolset::Script);
        assert_eq!(Toolset::Script.tools(), mcp::tools());
        for (mode, count) in [(Toolset::Script, 2), (Toolset::Atomic, 15)] {
            let tools = mode.tools();
            assert_eq!(tools.as_array().unwrap().len(), count);
            for tool in tools.as_array().unwrap() {
                assert!(mode.allows(tool["name"].as_str().unwrap()));
            }
        }
        assert!(!Toolset::Atomic.allows("scene.click"));
        assert!(!Toolset::Atomic.allows("type_text"));
        assert!(Toolset::Script
            .atomic_call("click", &Value::Null)
            .unwrap()
            .is_none());
    }

    #[test]
    fn atomic_command_keeps_untrusted_text_as_data() {
        let text = "\"); app.quit(); //\n\\\"__proto__\"\u{2028}";
        let (_, command) = Toolset::Atomic
            .atomic_call("typeText", &json!({"app":"example.app","text":text}))
            .unwrap()
            .unwrap();
        let wire::Command::App {
            method,
            args,
            timeout_ms,
        } = command
        else {
            panic!("atomic tool must produce a native app command");
        };
        assert_eq!(method, "typeText");
        assert_eq!(args, json!({"text":text}));
        assert_eq!(timeout_ms, scripts::DEFAULT_TIMEOUT_MS);
        for args in [
            json!({"app":"example.app","unknown":true}),
            json!({"app":"example.app","__proto__":{}}),
        ] {
            assert!(Toolset::Atomic.atomic_call("look", &args).is_err());
        }
    }
}
