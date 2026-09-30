package acceptance_test

import (
	"context"
	"encoding/json"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
	"os"
	"strings"
	"testing"
	"time"
)

// Opt-in native AX test. Each checkbox action is issued once; receipt lookup
// and identical request recovery must not produce a second app-side event.
func TestNativeValueFixture(t *testing.T) {
	title, logPath := os.Getenv("DW_NATIVE_FIXTURE_TITLE"), os.Getenv("DW_NATIVE_FIXTURE_LOG")
	if title == "" || logPath == "" {
		t.Skip("explicit native fixture required")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	w, err := local.Open(ctx, local.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close(context.Background())
	discovery, err := w.NewActor(ctx, dw.ActorConfig{ID: "value-discovery", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if err != nil {
		t.Fatal(err)
	}
	ob, err := discovery.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{ReadDeadline: 10 * time.Second, MaxResults: 1024, MaxVisitedNodes: 2048, MaxOutputBytes: 1 << 20}})
	if err != nil {
		t.Fatal(err)
	}
	var window dw.Ref
	for _, o := range ob.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == title {
			if window != "" {
				t.Fatal("ambiguous fixture")
			}
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatalf("fixture absent: %+v", ob.Coverage)
	}
	discovery.Close()
	a, err := w.NewActor(ctx, dw.ActorConfig{ID: "value-fixture", ReadScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, Operations: []string{"observe", "read", "invoke", "set_value", "wait"}})
	if err != nil {
		t.Fatal(err)
	}
	ob, err = a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"name", "role", "value_preview", "states"}, Budget: dw.Budget{MaxDepth: 6, ReadDeadline: 5 * time.Second, MaxTextRunes: 384, MaxOutputBytes: 1 << 20}})
	if err != nil {
		t.Fatal(err)
	}
	refs := map[string]dw.Ref{}
	previews := map[string]dw.Fact[string]{}
	for _, o := range ob.Objects {
		if o.Name.Value != nil {
			refs[*o.Name.Value] = o.Ref
			previews[*o.Name.Value] = o.ValuePreview
		}
	}
	read := func(name, want string) {
		t.Helper()
		ref := refs[name]
		if ref == "" {
			t.Fatalf("missing %q", name)
		}
		r, e := a.ReadText(ctx, dw.TextRequest{Target: ref})
		if e != nil || r.Source != "value" || r.Text.Value == nil || *r.Text.Value != want {
			t.Fatalf("%s read=%+v err=%v want=%q", name, r, e, want)
		}
		if name == "Unicode preview boundary" {
			t.Logf("Unicode boundary full value retained (%d runes); preview does not split emoji", len([]rune(*r.Text.Value)))
		} else {
			t.Logf("%s text=%q source=%s preview_status=%s", name, *r.Text.Value, r.Source, previews[name].Status)
		}
	}
	read("Value checkbox", "0")
	read("Mixed checkbox", "2")
	read("Numeric slider", "37")
	read("内容", "")
	read("Unicode preview boundary", strings.Repeat("x", 383)+"🌍tail")
	if previews["Unicode preview boundary"].Value == nil || *previews["Unicode preview boundary"].Value != strings.Repeat("x", 383) {
		t.Fatal("preview split a surrogate or exceeded native bound")
	}
	if previews["Label only"].Status != dw.FactUnknown || previews["Protected field"].Status != dw.FactRedacted {
		t.Fatalf("unknown/redacted previews changed: label=%+v protected=%+v", previews["Label only"], previews["Protected field"])
	}
	label, e := a.ReadText(ctx, dw.TextRequest{Target: refs["Label only"]})
	if e != nil || label.Source != "label" || label.Text.Value == nil || *label.Text.Value != "Label only" {
		t.Fatalf("label read=%+v %v", label, e)
	}
	protected, e := a.ReadText(ctx, dw.TextRequest{Target: refs["Protected field"]})
	if e != nil || protected.Text.Status != dw.FactRedacted || protected.Text.Value != nil {
		t.Fatalf("protected leaked: %+v %v", protected, e)
	}
	env, e := w.Environment(ctx)
	if e != nil {
		t.Fatal(e)
	}
	act := func(id, name, want string, success bool) {
		t.Helper()
		p := dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":" + id), Steps: []dw.Step{{ID: id, Op: "invoke", Completion: "verify", Target: dw.Target{Ref: refs[name]}, Timeout: 500 * time.Millisecond, After: []dw.Predicate{{Target: dw.Target{Ref: refs[name]}, Property: "value", EqualsString: &want}}}}}
		r, e := a.Execute(ctx, p)
		if success && (e != nil || r.Outcome != "completed" || r.Steps[0].Verification != dw.VerifyVerified) {
			b, _ := json.Marshal(r)
			t.Fatalf("%s receipt=%s %v", id, b, e)
		}
		if !success && (e == nil || r.Outcome == "completed" || r.Fault == nil || r.Fault.Code != "verification_timeout" || r.Steps[0].Delivery != dw.DeliveryComplete || (name == "Label only" && r.Steps[0].Verification != dw.VerifyUnknown) || (name == "Value checkbox" && r.Steps[0].Verification != dw.VerifyNotMet)) {
			t.Fatalf("label falsely verified=%+v %v", r, e)
		}
		recovered, e := a.GetReceipt(ctx, r.RunID)
		if recovered.RunID != r.RunID || (success && e != nil) {
			t.Fatalf("receipt recovery: %+v %v", recovered, e)
		}
		again, _ := a.Execute(ctx, p)
		if again.RunID != r.RunID {
			t.Fatal("identical request replayed")
		}
		t.Logf("%s delivery=%s verification=%s outcome=%s fault=%v", id, r.Steps[0].Delivery, r.Steps[0].Verification, r.Outcome, r.Steps[0].Fault)
	}
	unicode := "Value 联调 🌍"
	stringPlan := dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":unicode-value"), Steps: []dw.Step{{ID: "unicode", Op: "set_value", Target: dw.Target{Ref: refs["内容"]}, SetValue: &dw.SetValue{Text: unicode}}}}
	stringReceipt, e := a.Execute(ctx, stringPlan)
	if e != nil || stringReceipt.Outcome != "completed" || stringReceipt.Steps[0].Verification != dw.VerifyVerified {
		t.Fatalf("Unicode value predicate: %+v %v", stringReceipt, e)
	}
	read("内容", unicode)
	// A wait step evaluates the slider's numeric value without introducing another input.
	sliderWant := "37"
	sliderPlan := dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":slider-value"), Steps: []dw.Step{{ID: "slider", Op: "wait", After: []dw.Predicate{{Target: dw.Target{Ref: refs["Numeric slider"]}, Property: "value", EqualsString: &sliderWant}}}, {ID: "mixed", Op: "wait", After: []dw.Predicate{{Target: dw.Target{Ref: refs["Mixed checkbox"]}, Property: "value", EqualsString: func() *string { v := "2"; return &v }()}}}}}
	sliderReceipt, e := a.Execute(ctx, sliderPlan)
	if e != nil || sliderReceipt.Outcome != "completed" {
		t.Fatalf("numeric slider predicate: %+v %v", sliderReceipt, e)
	}
	t.Log("numeric slider37 and mixed checkbox2 value predicates verified")
	act("checkbox-on", "Value checkbox", "1", true)
	read("Value checkbox", "1")
	act("checkbox-off", "Value checkbox", "0", true)
	read("Value checkbox", "0")
	act("checkbox-failed-condition", "Value checkbox", "99", false)
	read("Value checkbox", "1")
	act("label-is-not-value", "Label only", "Label only", false)
	data, e := os.ReadFile(logPath)
	if e != nil {
		t.Fatal(e)
	}
	states := []string{}
	labels := 0
	for _, line := range strings.Split(string(data), "\n") {
		var ev struct{ Event, Value string }
		if json.Unmarshal([]byte(line), &ev) == nil {
			if ev.Event == "checkbox" {
				states = append(states, ev.Value)
			}
			if ev.Event == "label_action" {
				labels++
			}
		}
	}
	if strings.Join(states, ",") != "1,0,1" || labels != 1 {
		t.Fatalf("independent event evidence checkbox=%v label=%d", states, labels)
	}
	t.Logf("independent app events checkbox=%v label_action=%d; recovery sent no input", states, labels)
}
