package protocol_test

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/caelis-labs/desktop-world/protocol"
)

func TestCompactPreservesUncertaintyAndFalseValues(t *testing.T) {
	in := []byte(`{"coverage":{"complete":false,"continuation":"next","sample_start":"a","sample_end":"b"},"objects":[{"ref":"r1","kind":"ui","lifecycle":"stale","version":"4","geometry_version":"2","sample_start":"a","sample_end":"b","name":{"status":"known","source":"ui_content","value":""},"states":{"checked":{"status":"known","value":false},"enabled":{"status":"unknown"},"protected":{"status":"redacted"}},"bounds":{"status":"unsupported"}}],"receipt":{"delivery":"unknown","run_id":"run1","fault":{"code":"timeout"}}}`)
	out, err := protocol.CompactJSON(in)
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{`"known":false`, `"known":""`, `"status":"unknown"`, `"status":"redacted"`, `"status":"unsupported"`, `"complete":false`, `"continuation":"next"`, `"version":"4"`, `"lifecycle":"stale"`, `"delivery":"unknown"`, `"run_id":"run1"`} {
		if !strings.Contains(string(out), want) {
			t.Fatalf("lost %s: %s", want, out)
		}
	}
	var decoded map[string]any
	_ = json.Unmarshal(out, &decoded)
	if len(out) >= len(in) || decoded["coverage"].(map[string]any)["sample_start"] != "a" {
		t.Fatal("not compact or lost coverage sample")
	}
	obj := decoded["objects"].([]any)[0].(map[string]any)
	if _, exists := obj["sample_start"]; exists {
		t.Fatal("repeated object timestamp")
	}
}
