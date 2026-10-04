package helper

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestActionSchemaDisclosesOneArmAndPreservesSafetyConstraints(t *testing.T) {
	full, _ := json.Marshal(Schema("act"))
	s := ActionSchema("set_expanded")
	data, _ := json.Marshal(s)
	if len(data) >= len(full)/2 {
		t.Fatalf("selected action still loads most of act: %d/%d", len(data), len(full))
	}
	args := s["properties"].(map[string]any)["args"].(map[string]any)
	steps := args["properties"].(map[string]any)["steps"].(map[string]any)
	item := steps["items"].(map[string]any)
	properties := item["properties"].(map[string]any)
	if len(item["oneOf"].([]any)) != 1 || properties["type_text"] != nil || properties["press"] != nil || properties["bind"] != nil || properties["set_value"] != nil {
		t.Fatalf("unrelated actions disclosed: %s", data)
	}
	for _, key := range []string{"target", "before", "after", "completion", "timeout_ms", "set_expanded"} {
		if properties[key] == nil {
			t.Fatalf("safety constraint omitted: %s", key)
		}
	}
	if properties["set_expanded"].(map[string]any)["additionalProperties"] != false || item["additionalProperties"] != false || steps["maxItems"] != 16 {
		t.Fatal("selected schema weakened validation bounds", steps)
	}
	// Narrowing one call must not mutate later full schemas.
	again, _ := json.Marshal(Schema("act"))
	if string(full) != string(again) || ActionSchema("raw_input") != nil || ActionSchema("native-call") != nil {
		t.Fatal("schema mutability or unsupported action")
	}
	index, _ := json.Marshal(SchemaIndex())
	if len(index) > 512 || strings.Contains(string(index), "properties") {
		t.Fatalf("schema entry point leaked full parameter definitions: %s", index)
	}
}

func TestSemanticActionSchemasDiscloseOnlyTheirArgument(t *testing.T) {
	for op, arm := range map[string]string{"set_checked": "set_checked", "set_selected": "set_selected", "scroll_into_view": ""} {
		t.Run(op, func(t *testing.T) {
			args := ActionSchema(op)["properties"].(map[string]any)["args"].(map[string]any)
			item := args["properties"].(map[string]any)["steps"].(map[string]any)["items"].(map[string]any)
			p := item["properties"].(map[string]any)
			if len(item["oneOf"].([]any)) != 1 {
				t.Fatal("action alternatives leaked", item)
			}
			for _, key := range []string{"set_checked", "set_selected", "set_expanded", "set_value", "press", "scroll"} {
				if (p[key] != nil) != (key == arm) {
					t.Fatal("wrong argument arm", key, p)
				}
			}
			if arm != "" {
				arg := p[arm].(map[string]any)
				property := map[string]string{"set_checked": "checked", "set_selected": "selected"}[arm]
				if len(arg["required"].([]string)) != 1 || arg["required"].([]string)[0] != property || arg["properties"].(map[string]any)[property].(map[string]any)["type"] != "boolean" {
					t.Fatal("explicit desired boolean lost", arg)
				}
			}
		})
	}
}
