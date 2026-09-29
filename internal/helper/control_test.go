package helper

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
)

func managedCall(s *Server, turn, id, op string, args any) Response {
	b, _ := json.Marshal(args)
	return s.Handle(context.Background(), Request{ID: id, Op: op, Turn: turn, Args: b})
}
func managedRefs(t *testing.T, s *Server, turn string) map[string]dw.Ref {
	t.Helper()
	out := managedCall(s, turn, "list", "observe", map[string]any{"scope": map[string]bool{"desktop": true}, "projection": "outline", "fields": []string{"name"}, "budget": map[string]int{"max_output_bytes": 65536}})
	if out.Error != nil {
		t.Fatal(out.Error)
	}
	var ob dw.Observation
	if err := protocol.Decode(out.Result.(json.RawMessage), &ob); err != nil {
		t.Fatal(err)
	}
	refs := map[string]dw.Ref{}
	for _, o := range ob.Objects {
		if o.Name.Value != nil {
			refs[*o.Name.Value] = o.Ref
		}
	}
	return refs
}
func control(t *testing.T, s *Server, op, turn string, app dw.Ref) {
	t.Helper()
	r := s.Control(context.Background(), ControlRequest{ID: op, Op: op, Turn: turn, Application: app})
	if r.Error != nil {
		t.Fatal(r.Error)
	}
}
func TestManagedApplicationTurnAndSeparateControl(t *testing.T) {
	s, f := setup(t, Config{Managed: true})
	control(t, s, "begin_turn", "turn1", "")
	f.Add(dwtest.Node{ID: "other", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Other")}})
	f.Add(dwtest.Node{ID: "other-button", App: "other", Parent: "other", Object: dw.Object{Kind: dw.KindUI, Role: "button", Name: dw.Known("Other button")}})
	refs := managedRefs(t, s, "turn1")
	for _, out := range []Response{
		managedCall(s, "turn1", "before", "act", invoke(refs["提交"])),
		managedCall(s, "turn1", "forged", "grant", map[string]any{"application": refs["Fixture"]}),
		s.Control(context.Background(), ControlRequest{ID: "wrong", Op: "grant", Turn: "turn1", Application: refs["Desktop World Fixture"]}),
	} {
		if out.Error == nil {
			t.Fatal("unapproved control/input accepted")
		}
	}
	control(t, s, "grant", "turn1", refs["Fixture"])
	if out := managedCall(s, "turn1", "other", "act", invoke(refs["Other button"])); out.Error == nil {
		t.Fatal("cross-app write")
	}
	one := managedCall(s, "turn1", "one", "act", invoke(refs["提交"]))
	if one.Error != nil {
		t.Fatal(one.Error)
	}
	if out := managedCall(s, "turn1", "one", "act", invoke(refs["提交"])); out.Error != nil {
		t.Fatal(out.Error)
	}
	if len(f.Events()) != 1 {
		t.Fatal("duplicated input")
	}
	control(t, s, "end_turn", "turn1", "")
	if out := managedCall(s, "turn1", "late", "act", invoke(refs["提交"])); out.Error == nil {
		t.Fatal("ended turn accepted")
	}
	control(t, s, "begin_turn", "turn2", "")
	if out := managedCall(s, "turn2", "unapproved", "act", invoke(refs["提交"])); out.Error == nil {
		t.Fatal("grant inherited")
	}
	control(t, s, "grant", "turn2", refs["Fixture"])
	if out := managedCall(s, "turn2", "one", "act", invoke(refs["提交"])); out.Error != nil {
		t.Fatal(out.Error)
	}
	if len(f.Events()) != 2 {
		t.Fatal("IDs not isolated across turns")
	}
	var receipt dw.Receipt
	_ = protocol.Decode(one.Result.(json.RawMessage), &receipt)
	if out := call(s, "receipt", "get", map[string]any{"run_id": receipt.RunID}); out.Error != nil {
		t.Fatal("lost prior receipt", out.Error)
	}
	control(t, s, "end_turn", "turn2", "")
	if s.Control(context.Background(), ControlRequest{ID: "reuse", Op: "begin_turn", Turn: "turn1"}).Error == nil {
		t.Fatal("reused ended turn")
	}
}
func TestEndTurnCancelsNativePlanAndDoesNotWaitOnDataLane(t *testing.T) {
	s, f := setup(t, Config{Managed: true})
	control(t, s, "begin_turn", "turn1", "")
	refs := managedRefs(t, s, "turn1")
	control(t, s, "grant", "turn1", refs["Fixture"])
	entered := make(chan struct{})
	release := make(chan struct{})
	defer close(release)
	f.Enqueue("invoke", dwtest.Behavior{Before: func(*dwtest.Fixture) { close(entered) }, Block: release, Apply: true})
	done := make(chan Response, 1)
	go func() {
		done <- managedCall(s, "turn1", "slow", "act", map[string]any{"steps": []any{map[string]any{"id": "first", "op": "invoke", "target": map[string]any{"ref": refs["提交"]}}, map[string]any{"id": "never", "op": "invoke", "target": map[string]any{"ref": refs["提交"]}}}})
	}()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("native input never started")
	}
	stopped := make(chan struct{})
	go func() { control(t, s, "end_turn", "turn1", ""); close(stopped) }()
	select {
	case <-stopped:
	case <-time.After(time.Second):
		t.Fatal("control blocked by input")
	}
	select {
	case out := <-done:
		if out.Error == nil {
			t.Fatal("cancel claimed success")
		}
	case <-time.After(time.Second):
		t.Fatal("plan did not cancel")
	}
	if len(f.Events()) > 1 {
		t.Fatal("executed queued input after end")
	}
}
func TestControlEOFFailsClosed(t *testing.T) {
	s, _ := setup(t, Config{Managed: true})
	control(t, s, "begin_turn", "turn1", "")
	if err := s.ServeControl(context.Background(), strings.NewReader(""), &strings.Builder{}); err != nil {
		t.Fatal(err)
	}
	if out := managedCall(s, "turn1", "read", "observe", map[string]any{}); out.Error == nil {
		t.Fatal("owner EOF retained turn")
	}
}

func TestUnresolvedTargetOwnerCannotPiggybackOnGrantedApp(t *testing.T) {
	g := &turnGrants{used: map[string]bool{}}
	if err := g.begin("turn1"); err != nil {
		t.Fatal(err)
	}
	defer g.stop()
	g.apps["approved"] = true
	ctx, done, err := g.bind(context.Background(), "turn1")
	if err != nil {
		t.Fatal(err)
	}
	defer done()
	d, err := g.Check(ctx, dw.Intent{Operation: "pointer.drag", Targets: []dw.Ref{"known", "unknown"}, Applications: []dw.Ref{"approved", ""}})
	if err != nil || d.Allow {
		t.Fatal("unresolved target owner was allowed", err)
	}
}
