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
