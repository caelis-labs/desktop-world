package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/caelis-labs/desktop-world/host"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestCompactMCPDefaultAndOriginalResult(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_COMPACT_OUTPUT=1", "DTW_POC_HELPER=", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-compact-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	first, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "compact-original", "code": `print('task fact')`}})
	if err != nil || first.IsError {
		t.Fatalf("compact exec: %v %+v", err, first)
	}
	defaultFacts := first.StructuredContent.(map[string]any)
	if defaultFacts["print"] != nil || defaultFacts["execution_id"] != nil || defaultFacts["state"] != "completed" ||
		first.Content[0].(*mcp.TextContent).Text != "task fact" {
		t.Fatalf("compact default duplicated metadata: %+v %+v", defaultFacts, first.Content)
	}
	original, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "compact-original"}})
	if err != nil || original.IsError {
		t.Fatalf("original result query: %v %+v", err, original)
	}
	originalFacts := original.StructuredContent.(map[string]any)
	if originalFacts["execution_id"] != "compact-original" || originalFacts["print"].([]any)[0] != "task fact" {
		t.Fatalf("original result lost: %+v", originalFacts)
	}
}

func TestCompactExecKeepsOnlyDecisionRelevantDefaultFacts(t *testing.T) {
	full := map[string]any{
		"execution_id": "e1", "state": "completed", "print": []string{"W1 window \"Test\""},
		"native_request_ids": []string{"e1-native-1"},
		"observations":       []map[string]any{{"native_request_id": "e1-native-1", "complete": true, "dirty": false, "truncated": false}},
		"actions":            []map[string]any{{"native_request_id": "e1-native-2", "run_id": "run-long", "outcome": "completed", "seat_health": "ready", "restoration": "not_borrowed", "steps": []map[string]any{{"id": "one", "channel": "semantic", "delivery": "not_applicable", "verification": "verified"}}}},
		"captures":           []map[string]any{{"native_request_id": "e1-native-3", "images": 1}},
	}
	got := compactExec(full)
	for _, key := range []string{"execution_id", "native_request_ids", "observations"} {
		if _, exists := got[key]; exists {
			t.Fatalf("routine %s leaked to default projection: %+v", key, got)
		}
	}
	action := got["actions"].([]map[string]any)[0]
	if action["outcome"] != "completed" || action["restoration"] != nil || action["run_id"] != nil {
		t.Fatalf("routine action not compact: %+v", action)
	}
	step := action["steps"].([]map[string]any)[0]
	if step["verification"] != "verified" || step["delivery"] != nil || got["captures"].([]map[string]any)[0]["images"] != 1 {
		t.Fatalf("necessary result missing: %+v", got)
	}
	if !strings.Contains(compactToolText(got), "W1 window \"Test\"") ||
		!strings.Contains(compactToolText(got), "one: completed, verified via semantic") {
		t.Fatalf("text duplicated ordinary result: %q", compactToolText(got))
	}
	structured := compactStructured(got)
	if structured["print"] != nil || structured["actions"] != nil || structured["captures"] != nil {
		t.Fatalf("ordinary facts duplicated in structured content: %+v", structured)
	}
}

func TestCompactExecPreservesIncompleteAndUncertainRecovery(t *testing.T) {
	ob := compactExec(map[string]any{"execution_id": "e2", "state": "completed", "observations": []map[string]any{{"native_request_id": "e2-native-1", "complete": false, "dirty": true, "truncated": true, "more": true, "unavailable_sources": []string{"ax_partial"}}}})
	if ob["execution_id"] != "e2" || ob["observations"].([]map[string]any)[0]["unavailable_sources"].([]string)[0] != "ax_partial" || !strings.Contains(compactToolText(ob), "observation incomplete (dirty, truncated, more, ax_partial); execution_id: e2") {
		t.Fatalf("incomplete coverage lost: %+v", ob)
	}
	partial := compactExec(map[string]any{"execution_id": "e3", "state": "completed", "actions": []map[string]any{{"native_request_id": "e3-native-1", "run_id": "run-original", "outcome": "partial", "seat_health": "ready", "steps": []map[string]any{{"id": "first", "channel": "semantic", "delivery": "complete", "verification": "verified"}, {"id": "second", "channel": "semantic", "delivery": "unknown", "verification": "not_verified", "fault": "provider_timeout"}}}}})
	action := partial["actions"].([]map[string]any)[0]
	steps := action["steps"].([]map[string]any)
	if partial["execution_id"] != "e3" || action["run_id"] != "run-original" || steps[1]["delivery"] != "unknown" || steps[1]["fault"] != "provider_timeout" || !strings.Contains(compactToolText(partial), "second: partial, not_verified via semantic; delivery unknown") {
		t.Fatalf("partial original recovery lost: %+v", partial)
	}
	unknown := compactExec(map[string]any{"execution_id": "e4", "state": "failed", "native_request_ids": []string{"e4-native-1"}, "native_error": map[string]any{"code": "native_unknown", "retry_class": "never_automatically", "message": "unknown result"}})
	if unknown["native_error"].(map[string]any)["retry_class"] != "never_automatically" || unknown["native_request_ids"].([]string)[0] != "e4-native-1" || !strings.Contains(compactToolText(unknown), "native native_unknown (never_automatically); execution_id: e4") {
		t.Fatalf("unknown original recovery lost: %+v", unknown)
	}
	cancelled := compactExec(map[string]any{"execution_id": "cancel-original", "state": "cancelled", "native_request_ids": []string{"cancel-native-1"}})
	if cancelled["native_request_ids"].([]string)[0] != "cancel-native-1" || !strings.Contains(compactToolText(cancelled), "state: cancelled; execution_id: cancel-original") {
		t.Fatalf("cancelled original recovery lost: %+v", cancelled)
	}
	unsafeRestoration := compactExec(map[string]any{"execution_id": "e5", "state": "completed", "actions": []map[string]any{{"run_id": "run-e5", "outcome": "completed", "seat_health": "ready", "restoration": "failed", "steps": []map[string]any{{"id": "click", "channel": "foreground_transaction", "delivery": "complete", "verification": "verified"}}}}})
	if unsafeRestoration["execution_id"] != "e5" || compactStructured(unsafeRestoration)["actions"] == nil || !strings.Contains(compactToolText(unsafeRestoration), "restoration=failed") || !strings.Contains(compactToolText(unsafeRestoration), "execution_id: e5") {
		t.Fatalf("failed restoration hidden: %+v", unsafeRestoration)
	}
}

// Official MCP and actual script child, with only the native boundary injected.
// This validates the compact wire response plus original-ID recovery; it does
// not stand in for a live provider producing unknown or partial delivery.
func TestCompactMCPUnknownAndPartialKeepOriginalReceipts(t *testing.T) {
	t.Setenv("DTW_POC_CHILD", "1")
	t.Setenv("DTW_POC_COMPACT_OUTPUT", "1")
	coordDir := t.TempDir()
	if err := os.Chmod(coordDir, 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("DTW_POC_COORD_DIR", coordDir)
	s := newSupervisor(nil, "")
	defer func() {
		if s.child != nil {
			_ = s.child.cmd.Process.Kill()
		}
	}()
	var calls atomic.Int32
	s.testNative = func(_ context.Context, id, op string, _ json.RawMessage) (host.Reply, error) {
		calls.Add(1)
		if op != "act" {
			return host.Reply{}, errors.New("unexpected operation")
		}
		if strings.HasPrefix(id, "uncertain-") {
			return host.Reply{}, errors.New("transport lost after possible dispatch")
		}
		return host.Reply{ID: id, Result: json.RawMessage(`{"run_id":"original-run","outcome":"partial","seat_health":"ready","steps":[{"id":"first","channel":"semantic","delivery":"complete","verification":"verified"},{"id":"second","channel":"semantic","delivery":"unknown","verification":"not_verified","fault":{"code":"provider_timeout"}}],"input":{"restoration":"not_borrowed"}}`)}, nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	serverTransport, clientTransport := mcp.NewInMemoryTransports()
	serverSession, err := newServerWithSupervisor(s).Connect(ctx, serverTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer serverSession.Close()
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-compact-fault", Version: "1"}, nil)
	session, err := client.Connect(ctx, clientTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(op, id, code string) *mcp.CallToolResult {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		return res
	}
	script := `await dtw.act({steps:[{id:'first',op:'set_value',target:{ref:'owned-synthetic-ref'},set_value:{text:'A'}},{id:'second',op:'set_value',target:{ref:'owned-synthetic-ref'},set_value:{text:'B'}}]});`
	unknown := call("exec", "uncertain-original", script)
	unknownFacts := unknown.StructuredContent.(map[string]any)
	if unknownFacts["state"] != "failed" || unknownFacts["native_error"].(map[string]any)["retry_class"] != "never_automatically" ||
		!strings.Contains(unknown.Content[0].(*mcp.TextContent).Text, "never_automatically") {
		t.Fatalf("unknown compact response: %+v", unknown)
	}
	if len(call("result", "uncertain-original", "").StructuredContent.(map[string]any)["native_receipts"].(map[string]any)) != 1 {
		t.Fatal("original unknown receipt missing")
	}
	_ = call("exec", "uncertain-original", script)
	partial := call("exec", "partial-original", script)
	partialFacts := partial.StructuredContent.(map[string]any)
	partialActions, ok := partialFacts["actions"].([]any)
	if !ok || len(partialActions) != 1 {
		t.Fatalf("partial action absent: facts=%+v text=%+v calls=%d", partialFacts, partial.Content, calls.Load())
	}
	if partialFacts["execution_id"] != "partial-original" || partialActions[0].(map[string]any)["run_id"] != "original-run" ||
		!strings.Contains(partial.Content[0].(*mcp.TextContent).Text, "delivery unknown") {
		t.Fatalf("partial compact response: %+v", partial)
	}
	original := call("result", "partial-original", "").StructuredContent.(map[string]any)
	if original["native_receipts"].(map[string]any)["partial-original-native-1"].(map[string]any)["result"].(map[string]any)["run_id"] != "original-run" || calls.Load() != 2 {
		t.Fatalf("original result or no-replay lost: %+v calls=%d", original, calls.Load())
	}
}
