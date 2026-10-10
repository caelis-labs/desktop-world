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

func TestWorldTextGroupsRoutineStepsButKeepsScriptIDsAndExceptions(t *testing.T) {
	full := map[string]any{
		"execution_id": "input-batch", "state": "completed", "actions": []map[string]any{{
			"outcome": "completed", "restoration": "restored", "seat_health": "ready",
			"steps": []map[string]any{
				{"id": "W1/B2.click#1", "channel": "foreground_transaction", "delivery": "complete", "verification": "not_requested"},
				{"id": "W1/T1.press#2", "channel": "foreground_transaction", "delivery": "complete", "verification": "not_requested"},
				{"id": "W1/T1.typeText#3", "channel": "foreground_transaction", "delivery": "complete", "verification": "not_requested"},
			},
		}},
	}
	result := modelResultText(input{Operation: "exec", ExecutionID: "input-batch"}, full)
	legacy := compactToolText(compactExec(full))
	t.Logf("fixed three-step action text: previous=%d UTF-8 bytes, current=%d UTF-8 bytes; tokenizer and model context not measured", len(legacy), len(result))
	if len(result) >= len(legacy) {
		t.Fatalf("routine action result did not shrink: previous=%q current=%q", legacy, result)
	}
	for _, want := range []string{"input-batch ·", "W1/B2.click#1, W1/T1.press#2, W1/T1.typeText#3: dispatched; effect unverified via foreground", "foreground restoration: restored"} {
		if !strings.Contains(result, want) {
			t.Fatalf("missing %q in %q", want, result)
		}
	}
	if strings.Contains(result, "foreground_transaction") || strings.Count(result, "via foreground") != 1 || strings.Count(result, "restored") != 1 || strings.Contains(result, "seat_health") {
		t.Fatalf("routine status repeated: %q", result)
	}
	partial := modelResultText(input{Operation: "exec", ExecutionID: "partial"}, map[string]any{
		"execution_id": "partial", "state": "completed", "actions": []map[string]any{{
			"outcome": "partial", "restoration": "failed", "seat_health": "user_interrupted",
			"steps": []map[string]any{{"id": "a", "state": "satisfied", "channel": "semantic", "delivery": "complete", "verification": "verified"}, {"id": "b", "state": "unknown", "channel": "foreground_transaction", "delivery": "unknown", "verification": "not_verified", "fault": "provider_timeout"}},
		}},
	})
	for _, want := range []string{"action: partial", "a: verified", "b: unknown", "delivery unknown", "provider_timeout", "foreground restoration: failed", "seat: user_interrupted"} {
		if !strings.Contains(partial, want) {
			t.Fatalf("exception omitted %q in %q", want, partial)
		}
	}
}
