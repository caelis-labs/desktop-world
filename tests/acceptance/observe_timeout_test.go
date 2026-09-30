package acceptance_test

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"strings"
	"testing"
	"time"
)

// Real AppKit AX traversal through Bot's managed-helper SDK. The host controls
// the turn and exact application grant; model data never selects authority.
func TestNativeObserveTimeoutFixture(t *testing.T) {
	title, executable := os.Getenv("DW_NATIVE_FIXTURE_TITLE"), os.Getenv("DW_NATIVE_HELPER_PATH")
	if title == "" || executable == "" {
		t.Skip("explicit slow fixture and helper required")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	c, e := host.Start(ctx, host.Options{Executable: executable})
	if e != nil {
		t.Fatal(e)
	}
	defer c.Close()
	turn := "timeout-fixture"
	if e = c.BeginTurn(ctx, turn); e != nil {
		t.Fatal(e)
	}
	reply, e := c.Call(ctx, turn, "inventory", "observe", dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{ReadDeadline: 5 * time.Second, MaxResults: 1024, MaxVisitedNodes: 2048, MaxOutputBytes: 1 << 20}})
	if e != nil || reply.Error != nil {
		t.Fatalf("inventory %v %+v", e, reply.Error)
	}
	var inventory dw.Observation
	if e = protocol.Decode(reply.Result, &inventory); e != nil {
		t.Fatal(e)
	}
	var window, app dw.Ref
	for _, o := range inventory.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == title {
			if window != "" {
				t.Fatal("ambiguous fixture")
			}
			window = o.Ref
			app = o.App
		}
	}
	if window == "" || app == "" {
		t.Fatal("fixture not found")
	}
	if e = c.Grant(ctx, turn, app); e != nil {
		t.Fatal(e)
	}
	request := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name", "value_preview", "parent"}, Budget: dw.Budget{ReadDeadline: 200 * time.Millisecond, MaxDepth: 6, MaxVisitedNodes: 300, MaxResults: 300, MaxOutputBytes: 12000}}
	started := time.Now()
	reply, e = c.Call(ctx, turn, "slow-observe", "observe", request)
	elapsed := time.Since(started)
	if e != nil {
		t.Fatal(e)
	}
	var partial dw.Observation
	if e = protocol.Decode(reply.Result, &partial); e != nil {
		t.Fatal(e)
	}
	if partial.Coverage.Complete || partial.Coverage.VisitedNodes == 0 || !strings.Contains(strings.Join(partial.Coverage.UnavailableSources, ","), "ax_timeout") || elapsed > time.Second {
		t.Fatalf("timeout diagnostics missing: elapsed=%v coverage=%+v fault=%+v", elapsed, partial.Coverage, reply.Error)
	}
	content := host.Content(reply)
	if !strings.Contains(content.Content[0].Text, "ax_timeout") || !strings.Contains(content.Content[0].Text, "visited_nodes") || !strings.Contains(content.Content[0].Text, "sample_start") || !strings.Contains(content.Content[0].Text, "sample_end") {
		t.Fatalf("model projection lost diagnostics: %+v", content)
	}
	t.Logf("native timeout elapsed=%v visited=%d complete=%t sources=%v model_bytes=%d", elapsed, partial.Coverage.VisitedNodes, partial.Coverage.Complete, partial.Coverage.UnavailableSources, len(content.Content[0].Text))
	var control dw.Ref
	for _, o := range partial.Objects {
		if o.Name.Value != nil && *o.Name.Value == "内容" {
			control = o.Ref
		}
	}
	if control == "" {
		t.Fatal("initial bounded sample omitted fixture control")
	}
	narrow := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{control}}, Projection: dw.ProjectionDetail, Fields: []string{"name", "role"}, Budget: dw.Budget{ReadDeadline: time.Second, MaxOutputBytes: 4096}}
	reply, e = c.Call(ctx, turn, "narrow-after-timeout", "observe", narrow)
	var recovered dw.Observation
	_ = protocol.Decode(reply.Result, &recovered)
	if e != nil || reply.Error != nil || !recovered.Coverage.Complete || len(recovered.Objects) != 1 {
		t.Fatalf("narrow recovery: %+v %v coverage=%+v", reply.Error, e, recovered.Coverage)
	}
	request.Budget.ReadDeadline = 3 * time.Second
	request.Budget.MaxVisitedNodes = 8
	reply, e = c.Call(ctx, turn, "node-budget", "observe", request)
	var bounded dw.Observation
	_ = protocol.Decode(reply.Result, &bounded)
	if e != nil || bounded.Coverage.Complete || bounded.Coverage.VisitedNodes != 8 {
		t.Fatalf("node budget %+v %v", bounded.Coverage, e)
	}
	request.Budget.MaxOutputBytes = 2400
	reply, e = c.Call(ctx, turn, "output-budget", "observe", request)
	var output dw.Observation
	_ = protocol.Decode(reply.Result, &output)
	if e != nil || reply.Error != nil || output.Coverage.VisitedNodes != bounded.Coverage.VisitedNodes {
		t.Fatalf("output altered traversal %+v %v %v", output.Coverage, e, reply.Error)
	}
	t.Logf("node allowance visited=%d; smaller output budget visited=%d, next=%t", bounded.Coverage.VisitedNodes, output.Coverage.VisitedNodes, output.Coverage.Continuation != "")
	if e = c.EndTurn(ctx, turn); e != nil {
		t.Fatal(e)
	}
	if e = c.BeginTurn(ctx, "revoked-fixture"); e != nil {
		t.Fatal(e)
	}
	reply, e = c.Call(ctx, turn, "ended-turn", "observe", narrow)
	if e != nil || reply.Error == nil || reply.Error.Code != "turn_expired" {
		t.Fatalf("ended turn admitted Ref use: fault=%+v err=%v", reply.Error, e)
	}
	reply, e = c.Call(ctx, "revoked-fixture", "old-ref", "act", map[string]any{"steps": []dw.Step{{ID: "denied", Op: "set_value", Target: dw.Target{Ref: control}, SetValue: &dw.SetValue{Text: "MUST NOT ARRIVE"}}}})
	if e != nil || reply.Error == nil || reply.Error.Code != "permission_denied" {
		t.Fatalf("EndTurn retained control authority: fault=%+v err=%v", reply.Error, e)
	}
	t.Log("EndTurn independently expired old-turn Ref use and revoked the application write grant; new-turn input denied")
}
