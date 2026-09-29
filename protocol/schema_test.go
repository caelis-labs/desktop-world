package protocol_test

import (
	"encoding/json"
	"github.com/caelis-labs/desktop-world/protocol"
	"strings"
	"testing"
)

func TestToolSchemasExposeArguments(t *testing.T) {
	for _, tool := range protocol.Tools() {
		p := tool.InputSchema["properties"].(map[string]any)
		args := p["args"].(map[string]any)
		fields, ok := args["properties"].(map[string]any)
		if !ok || len(fields) == 0 {
			t.Fatalf("opaque args for %s", tool.Name)
		}
		if args["additionalProperties"] != false {
			t.Fatal("unbounded object")
		}
	}
	b, e := json.Marshal(protocol.ArgumentsSchema("world.act"))
	if e != nil {
		t.Fatal(e)
	}
	for _, needle := range []string{`"timeout_ms"`, `"duration_ms"`, `"oneOf"`, `"require_unique"`, `"primary"`, `"equals_version"`, `"type_text"`} {
		if !strings.Contains(string(b), needle) {
			t.Fatalf("missing %s", needle)
		}
	}
	if strings.Contains(string(b), `"timeout"`) || strings.Contains(string(b), `"duration"`) {
		t.Fatal("schema disagrees with duration wire names")
	}
}
