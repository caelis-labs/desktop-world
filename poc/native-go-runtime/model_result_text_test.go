package main

import (
	"strings"
	"testing"
)

func TestWorldTextKeepsUncertainRecoveryAndOmitsRoutineFlags(t *testing.T) {
	unknown := modelResultText(input{Operation: "exec", ExecutionID: "e7"}, map[string]any{
		"execution_id": "e7", "state": "failed", "native_request_ids": []string{"e7-native-1"},
		"native_error": map[string]any{"code": "native_unknown", "message": "transport lost after possible dispatch", "retry_class": "never_automatically"},
	})
	for _, part := range []string{"e7 ·", "native_unknown", "transport lost after possible dispatch", "query result(e7)", "do not repeat"} {
		if !strings.Contains(unknown, part) {
			t.Fatalf("unknown outcome lost %q: %q", part, unknown)
		}
	}
	if strings.Contains(unknown, "native_request_ids") || strings.Count(unknown, "e7 ·") != 1 {
		t.Fatalf("routine ID duplication: %q", unknown)
	}
	incomplete := modelResultText(input{Operation: "exec", ExecutionID: "e8"}, map[string]any{
		"execution_id": "e8", "state": "completed", "observations": []map[string]any{{
			"native_request_id": "e8-native-1", "complete": false, "dirty": true, "truncated": true, "more": true,
		}},
	})
	if !strings.HasPrefix(incomplete, "e8 · observation incomplete") || !strings.Contains(incomplete, "more") ||
		strings.Contains(incomplete, "e8-native-1") {
		t.Fatalf("incomplete observation not decision-ready: %q", incomplete)
	}
	status := modelResultText(input{Operation: "status", ExecutionID: "e9"}, map[string]any{
		"execution_id": "e9", "state": "running",
	})
	if status != "e9 · running" {
		t.Fatalf("status should be concise: %q", status)
	}
}
