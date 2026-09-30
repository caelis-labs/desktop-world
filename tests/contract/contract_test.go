package contract_test

import (
	"context"
	"errors"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

var ctx = context.Background()
var allOps = []string{"observe", "read", "sync", "bind", "wait", "focus", "invoke", "set_value", "keyboard.type_text", "keyboard.press", "pointer.move", "pointer.click", "pointer.drag", "pointer.scroll", "resolve_anchor", "capture", "read_asset", "raw_input"}

type host struct {
	w     dw.World
	a     dw.Actor
	f     *dwtest.Fixture
	epoch dw.Epoch
	refs  map[string]dw.Ref
}

func setup(t *testing.T, opts ...dwtest.Options) *host {
	t.Helper()
	var o dwtest.Options
	if len(opts) > 0 {
		o = opts[0]
	}
	w, f, e := dwtest.NewWithOptions(ctx, o)
	if e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() {
		c, cancel := context.WithTimeout(ctx, time.Second)
		defer cancel()
		if e := w.Close(c); e != nil {
			t.Error(e)
		}
	})
	f.Form()
	a, e := w.NewActor(ctx, dw.ActorConfig{ID: "test", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: allOps})
	if e != nil {
		t.Fatal(e)
	}
	env, e := w.Environment(ctx)
	if e != nil {
		t.Fatal(e)
	}
	h := &host{w: w, a: a, f: f, epoch: env.Epoch, refs: map[string]dw.Ref{}}
	ob, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name", "value_preview", "states", "capabilities", "bounds"}})
	if e != nil {
		t.Fatal(e)
	}
	for _, o := range ob.Objects {
		if o.Name.Value != nil {
			h.refs[*o.Name.Value] = o.Ref
		}
	}
	return h
}
func (h *host) plan(id string, steps ...dw.Step) dw.Plan {
	return dw.Plan{Epoch: h.epoch, RequestID: dw.RequestID(string(h.epoch) + ":" + id), Steps: steps}
}
func target(r dw.Ref) dw.Target { return dw.Target{Ref: r} }
func code(e error) string {
	var f *dw.Fault
	if errors.As(e, &f) {
		return f.Code
	}
	if e != nil {
		return e.Error()
	}
	return ""
}
func TestLinearPlanAndDedup(t *testing.T) {
	h := setup(t)
	name := "内容"
	p := h.plan("one", dw.Step{ID: "window", Op: "focus", Target: target(h.refs["Desktop World Fixture"])}, dw.Step{ID: "bind", Op: "bind", Bind: &dw.Bind{Name: "field", RequireUnique: true, Locator: dw.Locator{Within: h.refs["Desktop World Fixture"], NameEquals: &name, Role: "text_field"}}}, dw.Step{ID: "focus", Op: "focus", Target: dw.Target{Bound: "field"}}, dw.Step{ID: "write", Op: "set_value", Target: dw.Target{Bound: "field"}, SetValue: &dw.SetValue{Text: "Hello 世界 🌍"}}, dw.Step{ID: "enter", Op: "keyboard.press", Target: dw.Target{Bound: "field"}, Press: &dw.KeyChord{Key: "Enter"}})
	r, e := h.a.Execute(ctx, p)
	if e != nil || r.Outcome != "completed" {
		t.Fatalf("%+v %v", r, e)
	}
	if r.Steps[3].Verification != dw.VerifyVerified || len(h.f.Events()) != 4 {
		t.Fatalf("receipt=%+v events=%v", r, h.f.Events())
	}
	again, e := h.a.Execute(ctx, p)
	if e != nil || again.RunID != r.RunID || len(h.f.Events()) != 4 {
		t.Fatal("replayed request")
	}
	p.Steps[3].SetValue.Text = "changed"
	if _, e = h.a.Execute(ctx, p); code(e) != "request_conflict" {
		t.Fatal(e)
	}
}

func TestSummaryFocusedRefUsableWithoutFullOutline(t *testing.T) {
	w, f, err := dwtest.New(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close(ctx)
	f.Form()
	f.Focus("field")
	reader, _ := w.NewActor(ctx, dw.ActorConfig{ID: "inventory", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	ob, err := reader.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary})
	if err != nil {
		t.Fatal(err)
	}
	var window dw.Ref
	for _, o := range ob.Objects {
		if o.Kind == dw.KindWindow {
			window = o.Ref
		}
	}
	if ob.Seat.FocusedObject.Value == nil {
		t.Fatal("missing focus")
	}
	scoped, _ := w.NewActor(ctx, dw.ActorConfig{ID: "scoped", ReadScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, Operations: []string{"observe", "keyboard.press"}})
	env, _ := w.Environment(ctx)
	receipt, err := scoped.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":focused-ref"), Steps: []dw.Step{{ID: "enter", Op: "keyboard.press", Target: target(*ob.Seat.FocusedObject.Value), Press: &dw.KeyChord{Key: "Enter"}}}})
	if err != nil || receipt.Outcome != "completed" {
		t.Fatalf("%+v %v", receipt, err)
	}
}

func TestWindowlessFocusedEditorRequiresAppAuthority(t *testing.T) {
	h := setup(t)
	h.f.Add(dwtest.Node{ID: "inline", App: "app", Parent: "app", Object: dw.Object{Kind: dw.KindUI, Role: "text_field", Name: dw.Known("inline editor"), States: map[string]dw.Fact[bool]{"enabled": dw.Known(true), "focused": dw.Known(true)}}})
	h.f.Focus("inline")
	ob, err := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary})
	if err != nil {
		t.Fatal(err)
	}
	ref := *ob.Seat.FocusedObject.Value
	if ob.Seat.ForegroundWindow.Status != dw.FactUnknown {
		t.Fatal("invented window")
	}
	a, _ := h.w.NewActor(ctx, dw.ActorConfig{ID: "window-only", ReadScopes: []dw.Scope{{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}}, Operations: []string{"keyboard.press"}})
	step := dw.Step{ID: "enter", Op: "keyboard.press", Target: target(ref), Press: &dw.KeyChord{Key: "Enter"}}
	if _, err := a.Execute(ctx, h.plan("denied-inline", step)); code(err) != "permission_denied" {
		t.Fatalf("escaped window: %v", err)
	}
	b, _ := h.w.NewActor(ctx, dw.ActorConfig{ID: "app-only", ReadScopes: []dw.Scope{{Refs: []dw.Ref{h.refs["Fixture"]}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{h.refs["Fixture"]}}}, Operations: []string{"keyboard.press"}})
	if _, err := b.Execute(ctx, h.plan("allowed-inline", step)); err != nil {
		t.Fatal(err)
	}
	h.f.Focus("field")
	if _, err := b.Execute(ctx, h.plan("lost-inline-focus", step)); err == nil {
		t.Fatal("typed into a different focus")
	}
}
func TestReplacementStopsBoundAlias(t *testing.T) {
	h := setup(t)
	h.f.Enqueue("focus", dwtest.Behavior{Apply: true, Before: func(f *dwtest.Fixture) {
		f.Remove("field")
		f.Add(dwtest.Node{ID: "field", App: "app", Parent: "window", Window: "window", Object: dw.Object{Kind: dw.KindUI, Role: "text_field", Name: dw.Known("内容")}})
	}})
	name := "内容"
	p := h.plan("replace", dw.Step{ID: "bind", Op: "bind", Bind: &dw.Bind{Name: "field", RequireUnique: true, Locator: dw.Locator{Within: h.refs["Desktop World Fixture"], NameEquals: &name}}}, dw.Step{ID: "focus", Op: "focus", Target: target(h.refs["Desktop World Fixture"])}, dw.Step{ID: "write", Op: "set_value", Target: dw.Target{Bound: "field"}, SetValue: &dw.SetValue{Text: "must not write"}}, dw.Step{ID: "enter", Op: "keyboard.press", Target: dw.Target{Bound: "field"}, Press: &dw.KeyChord{Key: "Enter"}})
	r, e := h.a.Execute(ctx, p)
	if code(e) != "ref_gone" || r.Outcome != "partial" || r.Steps[3].State != "skipped" || len(h.f.Events()) != 1 {
		t.Fatalf("%+v %v events %v", r, e, h.f.Events())
	}
	ob, e := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}, Projection: dw.ProjectionOutline})
	if e != nil {
		t.Fatal(e)
	}
	for _, o := range ob.Objects {
		if o.Name.Value != nil && *o.Name.Value == "内容" && o.Ref == h.refs["内容"] {
			t.Fatal("rebound old ref")
		}
	}
}
func TestIncompleteBindDoesNotDispatch(t *testing.T) {
	h := setup(t)
	h.f.SetIncomplete(true)
	name := "内容"
	r, e := h.a.Execute(ctx, h.plan("bind", dw.Step{ID: "bind", Op: "bind", Bind: &dw.Bind{Name: "f", RequireUnique: true, Locator: dw.Locator{Within: h.refs["Desktop World Fixture"], NameEquals: &name}}}, dw.Step{ID: "write", Op: "set_value", Target: dw.Target{Bound: "f"}, SetValue: &dw.SetValue{Text: "no"}}))
	if code(e) != "search_incomplete" || r.Outcome != "stopped" || len(h.f.Events()) != 0 {
		t.Fatalf("%+v %v", r, e)
	}
}
func TestUnknownNativeCallFencesAndNeverReplays(t *testing.T) {
	h := setup(t)
	block := make(chan struct{})
	started := make(chan struct{})
	h.f.Enqueue("invoke", dwtest.Behavior{Before: func(*dwtest.Fixture) { close(started) }, Block: block, IgnoreCancellation: true, Apply: true})
	p := h.plan("late", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"]), Timeout: 30 * time.Millisecond})
	r, e := h.a.Execute(ctx, p)
	<-started
	if e == nil || r.Outcome != "unknown" || r.SeatHealth != "fenced" || r.Steps[0].Delivery != dw.DeliveryUnknown {
		close(block)
		t.Fatalf("%+v %v", r, e)
	}
	r2, e := h.a.Execute(ctx, h.plan("blocked", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"])}))
	if code(e) != "seat_fenced" || r2.Outcome != "stopped" || r2.Fault.RetryClass != "never_automatically" {
		t.Fatalf("%+v %v", r2, e)
	}
	close(block)
	deadline := time.Now().Add(time.Second)
	for len(h.f.Events()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	again, _ := h.a.Execute(ctx, p)
	if again.Outcome != "unknown" || len(h.f.Events()) != 1 {
		t.Fatal("late result rewrote or replayed receipt")
	}
}
func TestVerificationUnknownAfterDelivery(t *testing.T) {
	h := setup(t)
	h.f.Enqueue("invoke", dwtest.Behavior{Apply: true, Before: func(f *dwtest.Fixture) {
		f.SetReadFault(dw.NewFault("provider_unavailable", "fixture fault", "read_only"))
	}})
	yes := true
	r, e := h.a.Execute(ctx, h.plan("unknown", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"]), Completion: "verify", Timeout: 30 * time.Millisecond, After: []dw.Predicate{{Target: target(h.refs["提交"]), Property: "enabled", EqualsBool: &yes}}}))
	if e == nil || r.Outcome != "unknown" || r.Steps[0].Delivery != dw.DeliveryComplete || r.Steps[0].Verification != dw.VerifyUnknown || r.SeatHealth != "ready" {
		t.Fatalf("%+v %v", r, e)
	}
}
func TestKnownNotMetIsPartial(t *testing.T) {
	// Take one known sample then expire while waiting, not during another native
	// read (which correctly makes the latest verification unknown).
	h := setup(t, dwtest.Options{VerificationPollInterval: time.Second})
	no := false
	r, e := h.a.Execute(ctx, h.plan("notmet", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"]), Completion: "verify", Timeout: 250 * time.Millisecond, After: []dw.Predicate{{Target: target(h.refs["提交"]), Property: "enabled", EqualsBool: &no}}}))
	if e == nil || r.Outcome != "partial" || r.Steps[0].Verification != dw.VerifyNotMet {
		t.Fatalf("%+v %v", r, e)
	}
}
func TestConcurrentRequestAdmission(t *testing.T) {
	h := setup(t)
	p := h.plan("concurrent", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"])})
	var wg sync.WaitGroup
	ids := make(chan dw.RunID, 20)
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			r, e := h.a.Execute(ctx, p)
			if e != nil {
				t.Error(e)
			}
			ids <- r.RunID
		}()
	}
	wg.Wait()
	close(ids)
	first := dw.RunID("")
	for id := range ids {
		if first == "" {
			first = id
		}
		if id != first {
			t.Fatal("multiple runs")
		}
	}
	if len(h.f.Events()) != 1 {
		t.Fatal(h.f.Events())
	}
}
func TestSnapshotDeltaAndMaterialVersions(t *testing.T) {
	h := setup(t)
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}, Projection: dw.ProjectionOutline, Fields: []string{"name", "value_preview", "states"}}
	ob, e := h.a.Observe(ctx, req)
	if e != nil {
		t.Fatal(e)
	}
	same, e := h.a.Observe(ctx, req)
	if e != nil || ob.Revision != same.Revision {
		t.Fatalf("sampling changed revision %d %d: %v", ob.Revision, same.Revision, e)
	}
	h.f.Update("field", func(n *dwtest.Node) { n.Object.Name = dw.Known("Changed") })
	delta, e := h.a.Changes(ctx, dw.ChangeRequest{Cursor: ob.Cursor})
	if e != nil || delta.ResetRequired || len(delta.Upserts) != 1 {
		t.Fatalf("%+v %v", delta, e)
	}
	rebuilt := map[dw.Ref]dw.Object{}
	for _, o := range ob.Objects {
		rebuilt[o.Ref] = o
	}
	for _, o := range delta.Upserts {
		rebuilt[o.Ref] = o
	}
	for _, r := range delta.Removed {
		delete(rebuilt, r.Ref)
	}
	now, e := h.a.Observe(ctx, req)
	if e != nil {
		t.Fatal(e)
	}
	for _, o := range now.Objects {
		if rebuilt[o.Ref].Version != o.Version || !reflect.DeepEqual(rebuilt[o.Ref].Name.Value, o.Name.Value) {
			t.Fatal("delta did not reconstruct snapshot")
		}
	}
	tiny, e := h.a.Changes(ctx, dw.ChangeRequest{Cursor: ob.Cursor, MaxOutputBytes: 32})
	if e != nil || !tiny.ResetRequired || tiny.Cursor != "" {
		t.Fatalf("%+v %v", tiny, e)
	}
}
func TestPaginationStableAndEnvelopeBounded(t *testing.T) {
	h := setup(t)
	for i := 0; i < 80; i++ {
		h.f.Add(dwtest.Node{ID: fmt.Sprintf("extra-%03d", i), Parent: "window", App: "app", Window: "window", Object: dw.Object{Kind: dw.KindUI, Role: "text", Name: dw.Known(strings.Repeat("语", 100))}})
	}
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}, Projection: dw.ProjectionOutline, Budget: dw.Budget{MaxResults: 10, MaxOutputBytes: 3000}, Fields: []string{"name", "role"}}
	seen := map[dw.Ref]bool{}
	for {
		ob, e := h.a.Observe(ctx, req)
		if e != nil {
			t.Fatal(e)
		}
		b, _ := protocol.Marshal(protocol.Response{Protocol: protocol.Version, World: h.epoch, Result: ob})
		if len(b) > req.Budget.MaxOutputBytes {
			t.Fatalf("budget exceeded %d", len(b))
		}
		for _, o := range ob.Objects {
			if seen[o.Ref] {
				t.Fatal("duplicate page result")
			}
			seen[o.Ref] = true
		}
		if ob.Coverage.Continuation == "" {
			break
		}
		req.Continuation = ob.Coverage.Continuation
	}
	if len(seen) != 83 {
		t.Fatalf("lost objects: %d", len(seen))
	}
}
func TestUnicodeTextContinuationAndProtection(t *testing.T) {
	h := setup(t)
	h.f.Update("field", func(n *dwtest.Node) { n.Text = "A😀中B" })
	one, e := h.a.ReadText(ctx, dw.TextRequest{Target: h.refs["内容"], LimitRunes: 2})
	if e != nil || *one.Text.Value != "A😀" || one.Next == "" {
		t.Fatalf("%+v %v", one, e)
	}
	two, e := h.a.ReadText(ctx, dw.TextRequest{Target: h.refs["内容"], LimitRunes: 2, Continuation: one.Next})
	if e != nil || *two.Text.Value != "中B" {
		t.Fatalf("%+v %v", two, e)
	}
	h.f.Update("field", func(n *dwtest.Node) { n.Text = "changed" })
	if _, e = h.a.ReadText(ctx, dw.TextRequest{Target: h.refs["内容"], Continuation: one.Next}); code(e) != "text_changed" {
		t.Fatal(e)
	}
	h.f.Update("field", func(n *dwtest.Node) {
		n.Object.States["protected"] = dw.Known(true)
		n.Object.ValuePreview = dw.Known("secret")
	})
	red, e := h.a.ReadText(ctx, dw.TextRequest{Target: h.refs["内容"]})
	if e != nil || red.Text.Status != dw.FactRedacted || red.Text.Value != nil {
		t.Fatalf("%+v %v", red, e)
	}
}
func TestScopesCursorIsolationAndCapture(t *testing.T) {
	h := setup(t)
	a, e := h.w.NewActor(ctx, dw.ActorConfig{ID: "scoped", ReadScopes: []dw.Scope{{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}}, Operations: allOps})
	if e != nil {
		t.Fatal(e)
	}
	if _, e = a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}}); code(e) != "permission_denied" {
		t.Fatal(e)
	}
	if _, e = a.Capture(ctx, dw.CaptureRequest{Kind: "visible_region", Target: h.refs["Desktop World Fixture"]}); code(e) != "permission_denied" {
		t.Fatal(e)
	}
	ob, _ := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}})
	delta, e := a.Changes(ctx, dw.ChangeRequest{Cursor: ob.Cursor})
	if e != nil || !delta.ResetRequired {
		t.Fatalf("%+v %v", delta, e)
	}
	image, e := h.a.Capture(ctx, dw.CaptureRequest{Kind: "visible_region"})
	if e != nil {
		t.Fatal(e)
	}
	if _, e = a.ReadAsset(ctx, image.Tiles[0].Asset); code(e) != "asset_expired" {
		t.Fatal(e)
	}
	if _, e = h.a.ReadAsset(ctx, image.Tiles[0].Asset); e != nil {
		t.Fatal(e)
	}
	h.f.SetPermission("screen_capture", "denied")
	if _, e = h.a.ReadAsset(ctx, image.Tiles[0].Asset); code(e) != "asset_expired" {
		t.Fatal(e)
	}
}

type authFunc func(context.Context, dw.Intent) (dw.Decision, error)

func (f authFunc) Check(c context.Context, i dw.Intent) (dw.Decision, error) { return f(c, i) }
func TestAuthorizationCheckedBeforeEachWrite(t *testing.T) {
	h := setup(t)
	var allowed atomic.Bool
	allowed.Store(true)
	a, e := h.w.NewActor(ctx, dw.ActorConfig{ID: "revoked", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: allOps, Authorizer: authFunc(func(_ context.Context, _ dw.Intent) (dw.Decision, error) {
		return dw.Decision{Allow: allowed.Load()}, nil
	})})
	if e != nil {
		t.Fatal(e)
	}
	h.f.Enqueue("invoke", dwtest.Behavior{Apply: true, Before: func(*dwtest.Fixture) { allowed.Store(false) }})
	r, e := a.Execute(ctx, h.plan("revoke", dw.Step{ID: "first", Op: "invoke", Target: target(h.refs["提交"])}, dw.Step{ID: "second", Op: "invoke", Target: target(h.refs["提交"])}))
	if code(e) != "permission_denied" || r.Outcome != "partial" || len(h.f.Events()) != 1 {
		t.Fatalf("%+v %v", r, e)
	}
}
func TestReceiptTombstoneSurvivesExpiry(t *testing.T) {
	h := setup(t, dwtest.Options{ReceiptTTL: time.Millisecond, RequestLimit: 1})
	p := h.plan("expire", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"])})
	r, firstErr := h.a.Execute(ctx, p)
	if firstErr != nil && code(firstErr) != "receipt_expired" {
		t.Fatal(firstErr)
	}
	// Clock resolution differs across platforms. Wait for the read-only expiry
	// condition before proving that a duplicate cannot replay the native input.
	deadline := time.Now().Add(time.Second)
	for {
		_, err := h.a.GetReceipt(ctx, r.RunID)
		if code(err) == "receipt_expired" {
			break
		}
		if err != nil || time.Now().After(deadline) {
			t.Fatalf("receipt did not expire: %v", err)
		}
		time.Sleep(time.Millisecond)
	}
	_, e := h.a.Execute(ctx, p)
	if code(e) != "receipt_expired" || len(h.f.Events()) != 1 {
		t.Fatal(e)
	}
	_, e = h.a.Execute(ctx, h.plan("new", p.Steps...))
	if code(e) != "resource_exhausted" {
		t.Fatal(e)
	}
}
func TestAnchorTopologyAndFocusIntervention(t *testing.T) {
	h := setup(t)
	a, e := h.a.ResolveAnchor(ctx, dw.Anchor{Target: h.refs["内容"], U: .5, V: 0})
	if e != nil || !a.Valid || a.Point.X != 200 {
		t.Fatalf("%+v %v", a, e)
	}
	h.f.SetTopology(2)
	r, e := h.a.Execute(ctx, h.plan("topology", dw.Step{ID: "move", Op: "pointer.move", Target: dw.Target{Point: &a.Point}}))
	if code(e) != "topology_changed" || r.Outcome != "stopped" {
		t.Fatalf("%+v %v", r, e)
	}
	h.f.SetFocus("submit")
	r, e = h.a.Execute(ctx, h.plan("wrongfocus", dw.Step{ID: "type", Op: "keyboard.type_text", Target: target(h.refs["内容"]), TypeText: &dw.TypeText{Text: "do not send"}}))
	if code(e) != "user_interrupted" || len(h.f.Events()) != 0 {
		t.Fatalf("%+v %v", r, e)
	}
}
func TestCloseWithPendingNativeCall(t *testing.T) {
	h := setup(t)
	block := make(chan struct{})
	h.f.Enqueue("invoke", dwtest.Behavior{Block: block, IgnoreCancellation: true, Apply: true})
	_, _ = h.a.Execute(ctx, h.plan("close", dw.Step{ID: "invoke", Op: "invoke", Target: target(h.refs["提交"]), Timeout: 20 * time.Millisecond}))
	c, cancel := context.WithTimeout(ctx, 10*time.Millisecond)
	defer cancel()
	if e := h.w.Close(c); code(e) != "close_incomplete" {
		t.Fatal(e)
	}
	close(block)
}

func TestEmptyViewDiscoversNewObject(t *testing.T) {
	h := setup(t)
	name := "new field"
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}, Projection: dw.ProjectionOutline, Match: &dw.Locator{Within: h.refs["Desktop World Fixture"], NameEquals: &name}}
	ob, e := h.a.Observe(ctx, req)
	if e != nil || len(ob.Objects) != 0 {
		t.Fatal(e)
	}
	h.f.Add(dwtest.Node{ID: "new", Parent: "window", Window: "window", App: "app", Object: dw.Object{Kind: dw.KindUI, Name: dw.Known(name)}})
	delta, e := h.a.Changes(ctx, dw.ChangeRequest{Cursor: ob.Cursor})
	if e != nil || len(delta.Upserts) != 1 {
		t.Fatalf("%+v %v", delta, e)
	}
}
func TestContinuationRechecksOSPermission(t *testing.T) {
	h := setup(t)
	req := dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline, Budget: dw.Budget{MaxResults: 1}}
	ob, e := h.a.Observe(ctx, req)
	if e != nil || ob.Coverage.Continuation == "" {
		t.Fatal(e)
	}
	h.f.SetPermission("accessibility", "denied")
	req.Continuation = ob.Coverage.Continuation
	if _, e = h.a.Observe(ctx, req); code(e) != "continuation_expired" {
		t.Fatalf("revoked page remained readable: %v", e)
	}
}
func TestTopologyInvalidatesCursor(t *testing.T) {
	h := setup(t)
	ob, e := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}})
	if e != nil {
		t.Fatal(e)
	}
	h.f.SetTopology(2)
	delta, e := h.a.Changes(ctx, dw.ChangeRequest{Cursor: ob.Cursor})
	if e != nil || !delta.ResetRequired || delta.ResetReason != "topology_changed" {
		t.Fatalf("%+v %v", delta, e)
	}
}
func TestCanonicalDefaultsDeduplicate(t *testing.T) {
	h := setup(t)
	h.f.SetFocus("field")
	p := h.plan("canonical", dw.Step{ID: "key", Op: "keyboard.press", Target: target(h.refs["内容"]), Press: &dw.KeyChord{Key: "Enter"}})
	one, e := h.a.Execute(ctx, p)
	if e != nil {
		t.Fatal(e)
	}
	p.Timeout = 10 * time.Second
	p.Steps[0].Timeout = 2 * time.Second
	p.Steps[0].Completion = "dispatch"
	p.Steps[0].Press.Modifiers = []string{}
	p.Steps[0].After = []dw.Predicate{}
	two, e := h.a.Execute(ctx, p)
	if e != nil || two.RunID != one.RunID || len(h.f.Events()) != 1 {
		t.Fatalf("%+v %v", two, e)
	}
}

func TestControlStepPreconditionsAreEnforced(t *testing.T) {
	h := setup(t)
	name := "内容"
	wrong := "not this window"
	r, e := h.a.Execute(ctx, h.plan("guarded-bind", dw.Step{ID: "bind", Op: "bind", Bind: &dw.Bind{Name: "f", RequireUnique: true, Locator: dw.Locator{Within: h.refs["Desktop World Fixture"], NameEquals: &name}}, Before: []dw.Predicate{{Target: target(h.refs["Desktop World Fixture"]), Property: "name", EqualsString: &wrong}}}))
	if code(e) != "precondition_failed" || r.Outcome != "stopped" || len(r.Bindings) != 0 {
		t.Fatalf("%+v %v", r, e)
	}
}
func TestTextVersionDoesNotExposeContentHash(t *testing.T) {
	h := setup(t)
	h.f.Update("field", func(n *dwtest.Node) { n.Text = "low entropy secret" })
	one, e := h.a.ReadText(ctx, dw.TextRequest{Target: h.refs["内容"]})
	if e != nil || one.TextVersion != 1 {
		t.Fatalf("%+v %v", one, e)
	}
	h.f.Update("field", func(n *dwtest.Node) { n.Text = "changed" })
	two, e := h.a.ReadText(ctx, dw.TextRequest{Target: h.refs["内容"]})
	if e != nil || two.TextVersion != 2 {
		t.Fatalf("%+v %v", two, e)
	}
}

func TestNativeTraversalRootSurvivesFirstPageAndURIIsOptIn(t *testing.T) {
	h := setup(t)
	for i := 0; i < 30; i++ {
		h.f.Add(dwtest.Node{ID: fmt.Sprintf("extra-%d", i), Parent: "window", App: "app", Window: "window", Object: dw.Object{Kind: dw.KindUI, Role: "text", Name: dw.Known(fmt.Sprint(i))}})
	}
	u := "file:///isolated/workspace/" + strings.Repeat("long-name-", 30) + ".txt"
	h.f.Update("window", func(n *dwtest.Node) { n.Object.URI = dw.Known(u) })
	r := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}, Projection: dw.ProjectionOutline, Fields: []string{"name", "uri"}, Budget: dw.Budget{MaxResults: 1, MaxTextRunes: 10}}
	ob, err := h.a.Observe(ctx, r)
	if err != nil || len(ob.Objects) != 1 || ob.Objects[0].Ref != h.refs["Desktop World Fixture"] {
		t.Fatalf("root must be first: %+v %v", ob, err)
	}
	if ob.Objects[0].URI.Value == nil || *ob.Objects[0].URI.Value != u {
		t.Fatal("URI was cropped or lost")
	}
	seen := map[dw.Ref]bool{ob.Objects[0].Ref: true}
	for ob.Coverage.Continuation != "" {
		r.Continuation = ob.Coverage.Continuation
		ob, err = h.a.Observe(ctx, r)
		if err != nil {
			t.Fatal(err)
		}
		for _, o := range ob.Objects {
			if seen[o.Ref] {
				t.Fatal("duplicate page ref")
			}
			seen[o.Ref] = true
		}
	}
	if len(seen) < 31 {
		t.Fatal("lost descendants", len(seen))
	}
	r.Continuation = ""
	r.Fields = []string{"name"}
	ob, err = h.a.Observe(ctx, r)
	if err != nil || ob.Objects[0].URI.Status != dw.FactUnrequested {
		t.Fatal("URI must be opt-in", err)
	}
}

func TestValuePredicateDoesNotVerifyLabel(t *testing.T) {
	h := setup(t)
	h.f.Update("submit", func(n *dwtest.Node) { n.Text = "提交"; n.TextSource = "label" })
	want := "提交"
	p := h.plan("label-is-not-value", dw.Step{ID: "label", Op: "invoke", Target: target(h.refs["提交"]), Completion: "verify", Timeout: 80 * time.Millisecond, After: []dw.Predicate{{Target: target(h.refs["提交"]), Property: "value", EqualsString: &want}}})
	r, e := h.a.Execute(ctx, p)
	if code(e) != "verification_timeout" || r.Steps[0].Verification != dw.VerifyUnknown || r.Steps[0].Delivery != dw.DeliveryComplete {
		t.Fatalf("label proved value: %+v %v", r, e)
	}
	again, _ := h.a.Execute(ctx, p)
	if again.RunID != r.RunID || len(h.f.Events()) != 1 {
		t.Fatal("failed verification replayed input")
	}
}
func TestIncompleteChangesDoNotInferRemovals(t *testing.T) {
	h := setup(t)
	ob, e := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline})
	if e != nil {
		t.Fatal(e)
	}
	h.f.Remove("field")
	h.f.SetIncomplete(true)
	delta, e := h.a.Changes(ctx, dw.ChangeRequest{Cursor: ob.Cursor})
	if e != nil || !delta.ResetRequired || delta.Coverage.Complete || len(delta.Removed) != 0 {
		t.Fatalf("partial query inferred removal: %+v %v", delta, e)
	}
}
