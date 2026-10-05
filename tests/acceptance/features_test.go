package acceptance_test

import (
	"context"
	"encoding/json"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"strings"
	"sync"
	"testing"
	"time"
)

type featureSession struct {
	t                 *testing.T
	ctx               context.Context
	c                 *host.Client
	seq, calls, bytes int
}

func featureStart(t *testing.T, policy dw.InputPolicy) *featureSession {
	t.Helper()
	path := os.Getenv("DTW_NATIVE_HELPER")
	if path == "" {
		t.Skip("set DTW_NATIVE_HELPER and per-feature fixture title/log")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	t.Cleanup(cancel)
	c, err := host.Start(ctx, host.Options{Executable: path, InputPolicy: policy, AssetsDir: os.Getenv("DTW_CAPTURE_ASSETS"), Stderr: os.Stderr})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { c.Close() })
	if err = c.BeginTurn(ctx, "feature"); err != nil {
		t.Fatal(err)
	}
	return &featureSession{t: t, ctx: ctx, c: c}
}
func (s *featureSession) call(op string, args any, out any) host.Reply {
	s.t.Helper()
	s.seq++
	r, err := s.c.Call(s.ctx, "feature", fmt.Sprintf("f-%d", s.seq), op, args)
	if err != nil {
		s.t.Fatal(err)
	}
	s.calls++
	projected := host.Content(r)
	s.bytes += len(projected.Content[0].Text)
	if r.Error != nil {
		s.t.Fatalf("%s: %v result=%s", op, r.Error, r.Result)
	}
	if out != nil {
		if err = protocol.Decode(r.Result, out); err != nil {
			s.t.Fatal(err)
		}
	}
	return r
}
func (s *featureSession) window(title string) dw.Object {
	s.t.Helper()
	req := dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Fields: []string{"name", "role", "app"}, Budget: dw.Budget{MaxResults: 32, MaxOutputBytes: 8192, MaxVisitedNodes: 512, ReadDeadline: 5 * time.Second}}
	for i := 0; i < 12; i++ {
		var ob dw.Observation
		s.call("observe", req, &ob)
		for _, o := range ob.Objects {
			if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == title {
				return o
			}
		}
		if ob.Coverage.Continuation == "" {
			break
		}
		req.Continuation = ob.Coverage.Continuation
	}
	s.t.Fatalf("explicit fixture %q unavailable", title)
	return dw.Object{}
}
func (s *featureSession) find(window dw.Ref, name string, fields ...string) dw.Object {
	return s.findRole(window, name, "", fields...)
}
func (s *featureSession) findRole(window dw.Ref, name, role string, fields ...string) dw.Object {
	s.t.Helper()
	if len(fields) == 0 {
		fields = []string{"role", "name"}
	}
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: fields, Match: &dw.Locator{Within: window, Role: role, NameEquals: &name}, Budget: dw.Budget{MaxDepth: 12, MaxVisitedNodes: 128, MaxResults: 4, MaxOutputBytes: 4096, ReadDeadline: 3 * time.Second}}
	for i := 0; i < 8; i++ {
		var ob dw.Observation
		s.call("observe", req, &ob)
		if len(ob.Objects) == 1 {
			return ob.Objects[0]
		}
		if len(ob.Objects) > 1 {
			s.t.Fatal("ambiguous fixture name")
		}
		if ob.Coverage.Continuation == "" {
			// WebKit publishes AX children asynchronously after the first read.
			// Repeat this same bounded query without widening scope or input.
			time.Sleep(50 * time.Millisecond)
		}
		req.Continuation = ob.Coverage.Continuation
	}
	s.t.Fatalf("explicit fixture control %q unavailable", name)
	return dw.Object{}
}
func (s *featureSession) act(steps ...dw.Step) dw.Receipt {
	s.t.Helper()
	var r dw.Receipt
	s.call("act", struct{ Steps []dw.Step }{steps}, &r)
	if r.Outcome != "completed" {
		s.t.Fatal(r)
	}
	return r
}
func (s *featureSession) grant(app dw.Ref) {
	s.t.Helper()
	if err := s.c.Grant(s.ctx, "feature", app); err != nil {
		s.t.Fatal(err)
	}
}
func fixtureEvents(t *testing.T, path string) []struct{ Event, Value string } {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var out []struct{ Event, Value string }
	for _, line := range strings.Split(string(data), "\n") {
		var row struct{ Event, Value string }
		if json.Unmarshal([]byte(line), &row) == nil {
			out = append(out, row)
		}
	}
	return out
}
func requiredFeatureFixture(t *testing.T, prefix string) (string, string) {
	t.Helper()
	title, path := os.Getenv(prefix+"_TITLE"), os.Getenv(prefix+"_LOG")
	if title == "" || path == "" {
		t.Skip("explicit per-feature native fixture required")
	}
	return title, path
}

func TestNativeNoSharedInput(t *testing.T) {
	title, logPath := requiredFeatureFixture(t, "DTW_F1")
	frontTitle, frontLog := requiredFeatureFixture(t, "DTW_FOREGROUND")
	bg := featureStart(t, dw.InputNoShared)
	window := bg.window(title)
	input := bg.find(window.Ref, "内容")
	submit := bg.find(window.Ref, "提交")
	bg.grant(window.App)
	// A mixed plan is rejected before the preceding field write.
	steps := []dw.Step{{ID: "write", Op: "set_value", Target: dw.Target{Ref: input.Ref}, SetValue: &dw.SetValue{Text: "must-not-submit"}}, {ID: "enter", Op: "keyboard.press", Target: dw.Target{Ref: input.Ref}, Press: &dw.KeyChord{Key: "Enter"}}}
	rejected, err := bg.c.Call(bg.ctx, "feature", "mixed", "act", struct{ Steps []dw.Step }{steps})
	bg.calls++
	bg.bytes += len(host.Content(rejected).Content[0].Text)
	if err != nil || rejected.Error == nil || rejected.Error.Code != "requires_shared_input" {
		t.Fatal(rejected, err)
	}
	fg := featureStart(t, dw.InputShared)
	front := fg.window(frontTitle)
	editor := fg.find(front.Ref, "内容")
	fg.grant(front.App)
	fg.act(dw.Step{ID: "focus-window", Op: "focus", Target: dw.Target{Ref: front.Ref}}, dw.Step{ID: "focus-field", Op: "focus", Target: dw.Target{Ref: editor.Ref}})
	var seatBefore dw.Observation
	fg.call("observe", dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{front.Ref}}, Projection: dw.ProjectionDetail, Fields: []string{"role"}, Budget: dw.Budget{MaxOutputBytes: 4096}}, &seatBefore)
	if seatBefore.Seat.ForegroundWindow.Value == nil || *seatBefore.Seat.ForegroundWindow.Value != front.Ref {
		t.Fatal("foreground setup not verified")
	}
	// Independent helper processes use the actual shared seat while B stays background.
	const text = "Human-🙂-input"
	var wg sync.WaitGroup
	wg.Add(1)
	started := make(chan struct{})
	go func() {
		defer wg.Done()
		close(started)
		for _, ch := range text {
			fg.act(dw.Step{ID: "type", Op: "keyboard.type_text", Target: dw.Target{Ref: editor.Ref}, TypeText: &dw.TypeText{Text: string(ch)}})
			time.Sleep(15 * time.Millisecond)
		}
	}()
	<-started
	want := "Background order 🌍"
	r := bg.act(dw.Step{ID: "fill", Op: "set_value", Target: dw.Target{Ref: input.Ref}, SetValue: &dw.SetValue{Text: want}}, dw.Step{ID: "submit", Op: "invoke", Target: dw.Target{Ref: submit.Ref}})
	for _, step := range r.Steps {
		if step.Channel != "semantic" {
			t.Fatal("unexpected dispatch channel", step)
		}
	}
	wg.Wait()
	var seatAfter dw.Observation
	fg.call("observe", dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{front.Ref}}, Projection: dw.ProjectionDetail, Fields: []string{"role"}, Budget: dw.Budget{MaxOutputBytes: 4096}}, &seatAfter)
	if seatAfter.Seat.ForegroundWindow.Value == nil || *seatAfter.Seat.ForegroundWindow.Value != front.Ref {
		t.Fatal("background operation activated another window")
	}
	beforePoint, afterPoint := seatBefore.Seat.Pointer.Value, seatAfter.Seat.Pointer.Value
	if beforePoint == nil || afterPoint == nil || beforePoint.Frame != afterPoint.Frame || beforePoint.Topology != afterPoint.Topology || beforePoint.X != afterPoint.X || beforePoint.Y != afterPoint.Y {
		t.Fatal("system pointer changed during semantic task")
	}
	fg.act(dw.Step{ID: "submit", Op: "keyboard.press", Target: dw.Target{Ref: editor.Ref}, Press: &dw.KeyChord{Key: "Enter"}})
	// Dispatch and application callbacks are separate; await only the fixture's own evidence.
	for _, path := range []string{logPath, frontLog} {
		deadline := time.Now().Add(2 * time.Second)
		for {
			ready := false
			for _, ev := range fixtureEvents(t, path) {
				ready = ready || ev.Event == "submit"
			}
			if ready {
				break
			}
			if time.Now().After(deadline) {
				t.Fatal("fixture submit callback unavailable")
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	for path, expected := range map[string]string{logPath: want, frontLog: text} {
		events := fixtureEvents(t, path)
		count := 0
		for _, ev := range events {
			if ev.Event == "submit" {
				count++
				if ev.Value != expected {
					t.Fatalf("wrong recipient or value: %q want %q", ev.Value, expected)
				}
			}
			if path == logPath && (ev.Event == "key_down" || ev.Event == "pointer") {
				t.Fatal("background received physical input", ev)
			}
		}
		if count != 1 {
			t.Fatalf("submit count=%d", count)
		}
	}
	recovered, err := bg.c.Reconcile(bg.ctx, "feature", fmt.Sprintf("f-%d", bg.seq))
	if err != nil || recovered.ID == "" {
		t.Fatal(recovered, err)
	}
	t.Logf("F1 native background completed; foreground text intact; background model calls=%d bytes=%d; original receipt=%s", bg.calls, bg.bytes, r.RunID)
}

func TestNativeSetExpanded(t *testing.T) {
	title, logPath := requiredFeatureFixture(t, "DTW_F2")
	s := featureStart(t, dw.InputNoShared)
	window := s.window(title)
	item := s.find(window.Ref, "Shipping details", "role", "name", "states", "capabilities")
	s.grant(window.App)
	yes, no := true, false
	step := dw.Step{ID: "expand", Op: "set_expanded", Target: dw.Target{Ref: item.Ref}, SetExpanded: &dw.SetExpanded{Expanded: &yes}}
	r := s.act(step)
	if r.Steps[0].Verification != dw.VerifyVerified {
		t.Fatal(r)
	}
	again := s.act(step)
	if again.Steps[0].Delivery != dw.DeliveryNA {
		t.Fatal("already-expanded repeated native write", again)
	}
	step.ID = "collapse"
	step.SetExpanded = &dw.SetExpanded{Expanded: &no}
	s.act(step)
	events := fixtureEvents(t, logPath)
	var states, visible []string
	for _, ev := range events {
		if ev.Event == "expanded" {
			states = append(states, ev.Value)
		}
		if ev.Event == "details_visible" {
			visible = append(visible, ev.Value)
		}
		if ev.Event == "pointer" || ev.Event == "key_down" {
			t.Fatal("physical input used", ev)
		}
	}
	if strings.Join(states, ",") != "true,false" || strings.Join(visible, ",") != "true,false" {
		t.Fatal("app-side desired states and visible details", states, visible)
	}
	t.Logf("F2 native desired states verified, already-expanded no-op; model calls=%d bytes=%d", s.calls, s.bytes)
}

func TestNativeFieldPlan(t *testing.T) {
	title, logPath := requiredFeatureFixture(t, "DTW_F3")
	s := featureStart(t, dw.InputNoShared)
	window := s.window(title)
	count := func() int {
		n := 0
		for _, ev := range fixtureEvents(t, logPath) {
			if ev.Event == "value_read" {
				n++
			}
		}
		return n
	}
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window.Ref}}, Projection: dw.ProjectionOutline, Fields: []string{"name", "role", "value_preview"}, Budget: dw.Budget{MaxDepth: 6, MaxVisitedNodes: 128, MaxResults: 64, MaxOutputBytes: 16000, ReadDeadline: 5 * time.Second}}
	var full dw.Observation
	before := count()
	diagnosticCalls, diagnosticBytes := s.calls, s.bytes
	s.call("observe", req, &full)
	fullReads := count() - before
	diagnosticCalls, diagnosticBytes = s.calls-diagnosticCalls, s.bytes-diagnosticBytes
	req.Fields = []string{"role", "name"}
	req.Budget.MaxOutputBytes = 8192
	before = count()
	var narrow dw.Observation
	s.call("observe", req, &narrow)
	narrowReads := count() - before
	if fullReads < 8 || narrowReads != 0 {
		t.Fatalf("native value getters full=%d narrow=%d", fullReads, narrowReads)
	}
	input := s.find(window.Ref, "内容")
	submit := s.find(window.Ref, "提交")
	s.grant(window.App)
	want := "Field-plan order completed"
	s.act(dw.Step{ID: "fill", Op: "set_value", Target: dw.Target{Ref: input.Ref}, SetValue: &dw.SetValue{Text: want}}, dw.Step{ID: "submit", Op: "invoke", Target: dw.Target{Ref: submit.Ref}})
	deadline := time.Now().Add(2 * time.Second)
	for {
		submits := 0
		for _, ev := range fixtureEvents(t, logPath) {
			if ev.Event == "submit" {
				submits++
				if ev.Value != want {
					t.Fatal(ev)
				}
			}
		}
		if submits == 1 {
			break
		}
		if submits > 1 || time.Now().After(deadline) {
			t.Fatalf("submit count=%d", submits)
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Logf("F3 native task completed; value getters full=%d narrow=%d; task calls=%d projected response bytes=%d; explicit comparison calls=%d bytes=%d", fullReads, narrowReads, s.calls-diagnosticCalls, s.bytes-diagnosticBytes, diagnosticCalls, diagnosticBytes)
}
