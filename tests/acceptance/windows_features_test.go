package acceptance_test

import (
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/protocol"
	"runtime"
	"testing"
	"time"
)

func awaitFeatureSubmit(t *testing.T, path, want string) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for {
		count := 0
		for _, e := range fixtureEvents(t, path) {
			if e.Event == "submit" {
				count++
				if e.Value != want {
					t.Fatal(e)
				}
			}
		}
		if count == 1 {
			return
		}
		if count > 1 || time.Now().After(deadline) {
			t.Fatalf("independent submit count=%d", count)
		}
		time.Sleep(10 * time.Millisecond)
	}
}
func TestNativeWindowsContinuation(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("Windows interactive UIA acceptance")
	}
	title, path := requiredFeatureFixture(t, "DTW_F4")
	s := featureStart(t, dw.InputNoShared)
	window := s.window(title)
	name := "Deep submit"
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window.Ref}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name"}, Match: &dw.Locator{Within: window.Ref, NameEquals: &name}, Budget: dw.Budget{MaxDepth: 6, MaxVisitedNodes: 64, MaxResults: 2, MaxOutputBytes: 4096, ReadDeadline: 3 * time.Second}}
	previous := 0
	var button dw.Ref
	pages := 0
	for ; pages < 32; pages++ {
		var ob dw.Observation
		s.call("observe", req, &ob)
		if ob.Coverage.VisitedNodes <= previous {
			t.Fatal("native traversal failed to advance", ob.Coverage)
		}
		previous = ob.Coverage.VisitedNodes
		if pages == 0 && len(ob.Objects) != 0 {
			t.Fatal("target not beyond initial prefix")
		}
		if pages > 0 && !ob.Coverage.Dirty {
			t.Fatal("live resumed tree claimed stable coverage", ob.Coverage)
		}
		if len(ob.Objects) == 1 {
			button = ob.Objects[0].Ref
			break
		}
		if ob.Coverage.Continuation == "" {
			t.Fatal("native frontier lost", ob.Coverage)
		}
		if ob.Coverage.Complete {
			t.Fatal("unfinished scan marked complete")
		}
		req.Continuation = ob.Coverage.Continuation
	}
	if button == "" {
		t.Fatal("bounded continuation did not find deep target")
	}
	input := s.find(window.Ref, "内容")
	s.grant(window.App)
	want := "Windows resumed order"
	s.act(dw.Step{ID: "fill", Op: "set_value", Target: dw.Target{Ref: input.Ref}, SetValue: &dw.SetValue{Text: want}}, dw.Step{ID: "deep-submit", Op: "invoke", Target: dw.Target{Ref: button}})
	awaitFeatureSubmit(t, path, want)
	t.Logf("F4 actual UIA task complete; scan pages=%d visited=%d calls=%d bytes=%d", pages+1, previous, s.calls, s.bytes)
}
func TestNativeWindowsManagedControl(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("Windows interactive managed transport acceptance")
	}
	title, path := requiredFeatureFixture(t, "DTW_F5")
	s := featureStart(t, dw.InputNoShared)
	window := s.window(title)
	input := s.find(window.Ref, "内容")
	button := s.find(window.Ref, "提交")
	s.grant(window.App)
	want := "Windows managed order"
	receipt := s.act(dw.Step{ID: "fill", Op: "set_value", Target: dw.Target{Ref: input.Ref}, SetValue: &dw.SetValue{Text: want}}, dw.Step{ID: "submit", Op: "invoke", Target: dw.Target{Ref: button.Ref}})
	request := fmt.Sprintf("f-%d", s.seq)
	if err := s.c.EndTurn(s.ctx, "feature"); err != nil {
		t.Fatal(err)
	}
	original, err := s.c.Reconcile(s.ctx, "feature", request)
	if err != nil {
		t.Fatal(err)
	}
	var recovered dw.Receipt
	if err = protocol.Decode(original.Result, &recovered); err != nil || recovered.RunID != receipt.RunID {
		t.Fatal(recovered, err)
	}
	late, err := s.c.Call(s.ctx, "feature", "after-end", "act", struct{ Steps []dw.Step }{[]dw.Step{{ID: "never", Op: "invoke", Target: dw.Target{Ref: button.Ref}}}})
	if err != nil || late.Error == nil || late.Error.Code != "turn_expired" {
		t.Fatal(late, err)
	}
	awaitFeatureSubmit(t, path, want)
	t.Logf("F5 Windows private managed pipes completed task, revoked writes and preserved receipt; calls=%d bytes=%d", s.calls, s.bytes)
}
