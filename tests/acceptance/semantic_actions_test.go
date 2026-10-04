package acceptance_test

import (
	"encoding/json"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"strings"
	"testing"
	"time"
)

// Every feature runs with fresh apps, a fresh dtw process and app-owned evidence.
// The foreground helper simulates a human typing while the background task runs.
func semanticScenario(t *testing.T, feature, name, state, business, value string, step func(dw.Ref, bool) dw.Step) {
	t.Helper()
	title, logPath := requiredFeatureFixture(t, "DTW_"+feature)
	frontTitle, frontLog := requiredFeatureFixture(t, "DTW_FOREGROUND")
	bg := featureStart(t, dw.InputNoShared)
	window := bg.window(title)
	// Discovery only asks for name/role. State/capability details are requested
	// for the selected target, rather than dumping the whole application's tree.
	item := bg.find(window.Ref, name)
	var detail dw.Observation
	bg.call("observe", dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{item.Ref}}, Projection: dw.ProjectionDetail, Fields: []string{"states", "capabilities"}, Budget: dw.Budget{MaxResults: 1, MaxOutputBytes: 4096}}, &detail)
	if len(detail.Objects) != 1 || detail.Objects[0].States[state].Value == nil {
		t.Fatal("provider desired state not known", detail)
	}
	if *detail.Objects[0].States[state].Value != (feature == "F8") {
		t.Fatal("fixture did not start in the unsatisfied desired state", detail)
	}
	submit := bg.find(window.Ref, "提交")
	unsupported := bg.find(window.Ref, "Label only")
	bg.grant(window.App)
	// Unsupported targets refuse delivery and retain a retrievable receipt.
	bad := step(unsupported.Ref, true)
	rejected, err := bg.c.Call(bg.ctx, "feature", "unsupported", "act", struct{ Steps []dw.Step }{[]dw.Step{bad}})
	bg.calls++
	bg.bytes += len(host.Content(rejected).Content[0].Text)
	if err != nil || rejected.Error == nil || rejected.Error.Code != "capability_unavailable" {
		t.Fatal("unsupported operation did not refuse", rejected, err)
	}
	var refusal dw.Receipt
	if err = protocol.Decode(rejected.Result, &refusal); err != nil || len(refusal.Steps) != 1 || refusal.Steps[0].Delivery != dw.DeliveryNone {
		t.Fatal("missing no-delivery evidence", refusal, err)
	}
	recovered, err := bg.c.Reconcile(bg.ctx, "feature", "unsupported")
	if err != nil || recovered.ID == "" {
		t.Fatal("refusal not recoverable", recovered, err)
	}
	var original dw.Receipt
	if err = protocol.Decode(recovered.Result, &original); err != nil || original.RunID != refusal.RunID {
		t.Fatal("refusal receipt changed", original, err)
	}
	if feature == "F7" {
		mixed := bg.find(window.Ref, "Mixed checkbox", "name", "role", "states", "capabilities")
		if mixed.States["checked"].Status != dw.FactUnknown || mixed.States["checked"].Value != nil {
			t.Fatal("mixed checked state mapped to boolean", mixed)
		}
		rejected, err := bg.c.Call(bg.ctx, "feature", "mixed-checked", "act", struct{ Steps []dw.Step }{[]dw.Step{step(mixed.Ref, false)}})
		bg.calls++
		bg.bytes += len(host.Content(rejected).Content[0].Text)
		if err != nil || rejected.Error == nil || rejected.Error.Code != "capability_unavailable" {
			t.Fatal("mixed toggle not refused", rejected, err)
		}
		if err = protocol.Decode(rejected.Result, &refusal); err != nil || refusal.Steps[0].Delivery != dw.DeliveryNone {
			t.Fatal(refusal, err)
		}
	}
	fg := featureStart(t, dw.InputShared)
	front := fg.window(frontTitle)
	editor := fg.find(front.Ref, "内容")
	fg.grant(front.App)
	fg.act(dw.Step{ID: "window", Op: "focus", Target: dw.Target{Ref: front.Ref}}, dw.Step{ID: "editor", Op: "focus", Target: dw.Target{Ref: editor.Ref}})
	seatRequest := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{front.Ref}}, Projection: dw.ProjectionDetail, Fields: []string{"role"}, Budget: dw.Budget{MaxOutputBytes: 4096}}
	var before, after dw.Observation
	fg.call("observe", seatRequest, &before)
	if before.Seat.ForegroundWindow.Value == nil || *before.Seat.ForegroundWindow.Value != front.Ref {
		t.Fatal("foreground setup", before)
	}
	const humanText = "Human-🙂-input"
	started := make(chan error, 1)
	done := make(chan error, 1)
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		for i, ch := range humanText {
			s := dw.Step{ID: "type", Op: "keyboard.type_text", Target: dw.Target{Ref: editor.Ref}, TypeText: &dw.TypeText{Text: string(ch)}}
			r, err := fg.c.Call(fg.ctx, "feature", fmt.Sprintf("human-%d", i), "act", struct{ Steps []dw.Step }{[]dw.Step{s}})
			if err == nil && r.Error != nil {
				err = r.Error
			}
			var receipt dw.Receipt
			if err == nil {
				err = protocol.Decode(r.Result, &receipt)
			}
			if err == nil && receipt.Outcome != "completed" {
				err = fmt.Errorf("human input failed: %+v", receipt)
			}
			if i == 0 {
				started <- err
			}
			if err != nil {
				done <- err
				return
			}
			time.Sleep(100 * time.Millisecond)
		}
		done <- nil
	}()
	// Avoid calling testing.Fatal from the input goroutine; always join it.
	t.Cleanup(func() {
		select {
		case <-finished:
		case <-time.After(5 * time.Second):
		}
	})
	if err = <-started; err != nil {
		t.Fatal(err)
	}
	first := bg.act(step(item.Ref, true))
	if first.Steps[0].Channel != "semantic" || first.Steps[0].Verification != dw.VerifyVerified || first.Steps[0].Delivery != dw.DeliveryComplete {
		t.Fatal(first)
	}
	noop := bg.act(step(item.Ref, true))
	if noop.Steps[0].Verification != dw.VerifyVerified || noop.Steps[0].Delivery != dw.DeliveryNA {
		t.Fatal("no-op dispatched", noop)
	}
	if feature != "F8" {
		bg.act(step(item.Ref, false))
		bg.act(step(item.Ref, true))
	}
	if feature == "F8" {
		submit = item
	}
	result := bg.act(dw.Step{ID: "submit", Op: "invoke", Target: dw.Target{Ref: submit.Ref}})
	lastRequest := fmt.Sprintf("f-%d", bg.seq)
	recovered, err = bg.c.Reconcile(bg.ctx, "feature", lastRequest)
	if err != nil || recovered.ID == "" {
		t.Fatal("original receipt unavailable", recovered, err)
	}
	if err = protocol.Decode(recovered.Result, &original); err != nil || original.RunID != result.RunID {
		t.Fatal("business receipt changed", original, err)
	}
	if err = <-done; err != nil {
		t.Fatal(err)
	}
	fg.call("observe", seatRequest, &after)
	if after.Seat.ForegroundWindow.Value == nil || *after.Seat.ForegroundWindow.Value != front.Ref {
		t.Fatal("background stole focus")
	}
	bp, ap := before.Seat.Pointer.Value, after.Seat.Pointer.Value
	if bp == nil || ap == nil || *bp != *ap {
		t.Fatal("semantic task moved shared pointer", bp, ap)
	}
	fg.act(dw.Step{ID: "submit", Op: "keyboard.press", Target: dw.Target{Ref: editor.Ref}, Press: &dw.KeyChord{Key: "Enter"}})
	deadline := time.Now().Add(2 * time.Second)
	for {
		ready := false
		for _, ev := range fixtureEvents(t, frontLog) {
			ready = ready || (ev.Event == "submit" && ev.Value == humanText)
		}
		if ready {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("human Unicode text/submit corrupted")
		}
		time.Sleep(10 * time.Millisecond)
	}
	// High precision app-side timestamps prove the task overlapped human input.
	type event struct {
		Event, Value string
		Time         float64
	}
	readEvents := func(path string) []event {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		var events []event
		for _, line := range strings.Split(string(data), "\n") {
			var ev event
			if json.Unmarshal([]byte(line), &ev) == nil {
				events = append(events, ev)
			}
		}
		return events
	}
	start, end := 0.0, 0.0
	for _, ev := range readEvents(frontLog) {
		if ev.Event == "key_down" && ev.Value != "\r" {
			if start == 0 {
				start = ev.Time
			}
			end = ev.Time
		}
	}
	var transitions []string
	count := 0
	for _, ev := range readEvents(logPath) {
		if ev.Event == "key_down" || ev.Event == "pointer" || ev.Event == "label_action" || ev.Event == "mixed" {
			t.Fatal("unexpected background input/fallback", ev)
		}
		if ev.Event == "selected" || ev.Event == "checkbox" || ev.Event == "scrolled" {
			transitions = append(transitions, ev.Value)
		}
		if ev.Event == business {
			count++
			if ev.Value != value || ev.Time <= start || ev.Time >= end {
				t.Fatal("business task or actual input overlap unproven", ev, start, end)
			}
		}
	}
	wantTransitions := map[string]string{"F6": "Order A:true,Order A:false,Order A:true", "F7": "1,0,1", "F8": "true"}[feature]
	if count != 1 || strings.Join(transitions, ",") != wantTransitions {
		t.Fatal("app effects repeated or missing", count, transitions)
	}
	t.Logf("%s native business=%s; human Unicode intact; pointer/focus stable; no-op verified; background SDK calls=%d bytes=%d; original receipt=%s", feature, value, bg.calls, bg.bytes, result.RunID)
}

func TestNativeSetSelected(t *testing.T) {
	semanticScenario(t, "F6", "Order A", "selected", "ordered", "Order A,Order B", func(ref dw.Ref, value bool) dw.Step {
		return dw.Step{ID: "select", Op: "set_selected", Target: dw.Target{Ref: ref}, SetSelected: &dw.SetSelected{Selected: &value}}
	})
}
func TestNativeSetChecked(t *testing.T) {
	semanticScenario(t, "F7", "Approve order", "checked", "approved", "order-42", func(ref dw.Ref, value bool) dw.Step {
		return dw.Step{ID: "check", Op: "set_checked", Target: dw.Target{Ref: ref}, SetChecked: &dw.SetChecked{Checked: &value}}
	})
}
func TestNativeScrollIntoView(t *testing.T) {
	semanticScenario(t, "F8", "Fulfill distant order", "offscreen", "fulfilled", "order-900", func(ref dw.Ref, _ bool) dw.Step {
		return dw.Step{ID: "scroll", Op: "scroll_into_view", Target: dw.Target{Ref: ref}}
	})
}
