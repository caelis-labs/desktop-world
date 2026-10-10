package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/caelis-labs/desktop-world/host"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// This runs the official MCP tool and actual QuickJS subprocess while replacing
// only the native OS boundary. It proves projection and original-ID behavior;
// it does not claim that a real provider produced these fault outcomes.
func TestSyntheticUnknownPartialAndLateReceiptsThroughMCP(t *testing.T) {
	t.Setenv("DTW_POC_CHILD", "1")
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
	var unknownCalls, partialCalls, lateCalls atomic.Int32
	lateEntered := make(chan struct{}, 1)
	lateRelease := make(chan struct{})
	s.testNative = func(_ context.Context, id, op string, _ json.RawMessage) (host.Reply, error) {
		if op != "act" {
			return host.Reply{}, errors.New("unexpected operation")
		}
		switch {
		case strings.HasPrefix(id, "unknown-"):
			unknownCalls.Add(1)
			return host.Reply{}, errors.New("synthetic transport loss after possible dispatch")
		case strings.HasPrefix(id, "partial-"):
			partialCalls.Add(1)
			return host.Reply{ID: id, Result: json.RawMessage(`{"run_id":"run-partial-original","outcome":"partial","seat_health":"healthy","steps":[{"id":"first","channel":"semantic_background","delivery":"complete","verification":"verified"},{"id":"second","channel":"semantic_background","delivery":"unknown","verification":"not_verified","fault":{"code":"provider_timeout"}}],"input":{"restoration":"not_borrowed"}}`)}, nil
		case strings.HasPrefix(id, "late-"):
			lateCalls.Add(1)
			lateEntered <- struct{}{}
			<-lateRelease
			return host.Reply{ID: id, Result: json.RawMessage(`{"run_id":"run-late-original","outcome":"partial","seat_health":"healthy","steps":[{"id":"first","channel":"semantic_background","delivery":"complete","verification":"verified"},{"id":"second","channel":"semantic_background","delivery":"unknown","verification":"not_verified"}],"input":{"restoration":"not_borrowed"}}`)}, nil
		}
		return host.Reply{}, errors.New("unexpected native ID")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	serverTransport, clientTransport := mcp.NewInMemoryTransports()
	serverSession, err := newServerWithSupervisor(s).Connect(ctx, serverTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer serverSession.Close()
	client := mcp.NewClient(&mcp.Implementation{Name: "receipt-fault-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, clientTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(op, id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		return res.StructuredContent.(map[string]any)
	}
	script := `const r=await dtw.act({steps:[{id:'first',op:'set_value',target:{ref:'owned-synthetic-ref'},set_value:{text:'A'}},{id:'second',op:'set_value',target:{ref:'owned-synthetic-ref'},set_value:{text:'B'}}]});print(r.outcome);`
	unknown := call("exec", "unknown-original", script)
	if unknown["state"] != "failed" {
		t.Fatalf("unknown state: %+v", unknown)
	}
	if got := unknown["native_error"].(map[string]any); got["code"] != "native_unknown" || got["retry_class"] != "never_automatically" {
		t.Fatalf("unknown projection: %+v", got)
	}
	originalUnknown := call("result", "unknown-original", "")
	if len(originalUnknown["native_receipts"].(map[string]any)) != 1 {
		t.Fatalf("unknown original receipt: %+v", originalUnknown)
	}
	_ = call("exec", "unknown-original", script)
	if unknownCalls.Load() != 1 {
		t.Fatalf("unknown replayed: %d", unknownCalls.Load())
	}
	partial := call("exec", "partial-original", script)
	if partial["state"] != "completed" || partial["actions"].([]any)[0].(map[string]any)["outcome"] != "partial" {
		t.Fatalf("partial projection: %+v", partial)
	}
	steps := partial["actions"].([]any)[0].(map[string]any)["steps"].([]any)
	if steps[0].(map[string]any)["delivery"] != "complete" || steps[1].(map[string]any)["delivery"] != "unknown" {
		t.Fatalf("step coverage: %+v", steps)
	}
	originalPartial := call("result", "partial-original", "")
	rawPartial := originalPartial["native_receipts"].(map[string]any)["partial-original-native-1"].(map[string]any)
	if rawPartial["result"].(map[string]any)["run_id"] != "run-partial-original" {
		t.Fatalf("original run lost: %+v", rawPartial)
	}
	_ = call("exec", "partial-original", script)
	if partialCalls.Load() != 1 {
		t.Fatalf("partial replayed: %d", partialCalls.Load())
	}
	done := make(chan map[string]any, 1)
	go func() { done <- call("exec", "late-original", script) }()
	select {
	case <-lateEntered:
	case <-ctx.Done():
		t.Fatal("late native never entered")
	}
	status := call("status", "late-original", "")
	if status["state"] != "running" {
		t.Fatalf("status while native blocked: %+v", status)
	}
	cancelled := call("cancel", "late-original", "")
	if cancelled["state"] != "cancelling" && cancelled["state"] != "cancelled" {
		t.Fatalf("cancel: %+v", cancelled)
	}
	close(lateRelease)
	select {
	case <-done:
	case <-ctx.Done():
		t.Fatal("cancelled exec did not return")
	}
	var late map[string]any
	for ctx.Err() == nil {
		late = call("result", "late-original", "")
		if receipts, ok := late["native_receipts"].(map[string]any); ok && len(receipts) == 1 {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	receipt := late["native_receipts"].(map[string]any)["late-original-native-1"].(map[string]any)
	if receipt["result"].(map[string]any)["run_id"] != "run-late-original" || lateCalls.Load() != 1 {
		t.Fatalf("late original was lost or replayed: %+v calls=%d", late, lateCalls.Load())
	}
	t.Logf("unknown=%v partial=%v late=%v; native calls=%d/%d/%d", unknown["native_error"], partial["actions"], late["state"], unknownCalls.Load(), partialCalls.Load(), lateCalls.Load())
}
