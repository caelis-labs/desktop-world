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

// The fixture title and its own event log must be explicitly supplied. Ordinary
// go test never sends real input or reads unrelated application text.
func TestNativeFixture(t *testing.T) {
	title, logPath := os.Getenv("DW_NATIVE_FIXTURE_TITLE"), os.Getenv("DW_NATIVE_FIXTURE_LOG")
	if title == "" || logPath == "" {
		t.Skip("native fixture is opt-in: set DW_NATIVE_FIXTURE_TITLE and DW_NATIVE_FIXTURE_LOG")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	w, e := local.Open(ctx, local.Options{})
	if e != nil {
		t.Fatal(e)
	}
	defer func() {
		c, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		if e := w.Close(c); e != nil {
			t.Error(e)
		}
	}()
	a, e := w.NewActor(ctx, dw.ActorConfig{ID: "fixture-discovery", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if e != nil {
		t.Fatal(e)
	}
	env, e := w.Environment(ctx)
	if e != nil {
		t.Fatal(e)
	}
	ob, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Fields: []string{"name", "role", "capabilities"}, Budget: dw.Budget{ReadDeadline: 10 * time.Second, MaxResults: 1024, MaxOutputBytes: 1 << 20, MaxVisitedNodes: 2048}})
	if e != nil {
		t.Fatal(e)
	}
	var window dw.Ref
	for _, o := range ob.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == title {
			if window != "" {
				t.Fatal("ambiguous fixture title")
			}
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatalf("fixture window absent (objects=%d coverage=%+v)", len(ob.Objects), ob.Coverage)
	}
	a.Close()
	scoped, e := w.NewActor(ctx, dw.ActorConfig{ID: "fixture-only", ReadScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, Operations: []string{"observe", "bind", "focus", "set_value", "keyboard.type_text", "keyboard.press", "pointer.move", "pointer.click", "pointer.drag", "pointer.scroll", "read", "resolve_anchor"}})
	if e != nil {
		t.Fatal(e)
	}
	outline, e := scoped.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Budget: dw.Budget{ReadDeadline: 3 * time.Second, MaxDepth: 6, MaxOutputBytes: 1 << 20}})
	if e != nil {
		t.Fatal(e)
	}
	var input dw.Ref
	for _, o := range outline.Objects {
		if o.Role == "text_field" && o.Name.Value != nil && *o.Name.Value == "内容" {
			input = o.Ref
		}
	}
	if input == "" {
		b, _ := json.Marshal(outline)
		t.Fatalf("fixture field absent: %s", b)
	}
	anchor, e := scoped.ResolveAnchor(ctx, dw.Anchor{Target: input, U: .5, V: .5})
	if e != nil || !anchor.Valid {
		t.Fatalf("anchor: %+v %v", anchor, e)
	}
	value := "Desktop World 验收 🌍"
	p := dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":native-form"), Timeout: 10 * time.Second, Steps: []dw.Step{{ID: "window", Op: "focus", Target: dw.Target{Ref: window}}, {ID: "field", Op: "focus", Target: dw.Target{Ref: input}}, {ID: "value", Op: "set_value", Target: dw.Target{Ref: input}, SetValue: &dw.SetValue{Text: value}}, {ID: "select-all", Op: "keyboard.press", Target: dw.Target{Ref: input}, Press: &dw.KeyChord{Key: "A", Modifiers: []string{"primary"}}}, {ID: "unicode", Op: "keyboard.type_text", Target: dw.Target{Ref: input}, TypeText: &dw.TypeText{Text: value + "!"}}, {ID: "submit", Op: "keyboard.press", Target: dw.Target{Ref: input}, Press: &dw.KeyChord{Key: "Enter"}}}}
	r, e := scoped.Execute(ctx, p)
	if e != nil || r.Outcome != "completed" {
		b, _ := json.Marshal(r)
		t.Fatalf("receipt %s: %v", b, e)
	}
	again, e := scoped.Execute(ctx, p)
	if e != nil || again.RunID != r.RunID {
		t.Fatalf("duplicate request: %+v %v", again, e)
	}
	deadline := time.Now().Add(2 * time.Second)
	for {
		data, e := os.ReadFile(logPath)
		if e != nil {
			t.Fatal(e)
		}
		submits := 0
		valid := false
		for _, line := range strings.Split(string(data), "\n") {
			var event struct{ Event, Value string }
			if json.Unmarshal([]byte(line), &event) != nil {
				continue
			}
			if event.Event == "submit" {
				submits++
				valid = event.Value == value+"!"
			}
		}
		if submits > 0 {
			if submits != 1 || !valid {
				t.Fatalf("independent fixture log mismatched: submits=%d valid=%t", submits, valid)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("fixture did not receive submission")
		}
		time.Sleep(20 * time.Millisecond)
	}

	var button dw.Ref
	for _, o := range outline.Objects {
		if o.Role == "button" && o.Name.Value != nil && *o.Name.Value == "提交" {
			button = o.Ref
		}
	}
	if button == "" {
		t.Fatal("submit button absent")
	}
	mousePlan := dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":native-pointer"), Steps: []dw.Step{
		{ID: "move", Op: "pointer.move", Target: dw.Target{Ref: button}},
		{ID: "click", Op: "pointer.click", Target: dw.Target{Ref: button}, Click: &dw.Click{Button: "left", Count: 1}},
		{ID: "drag", Op: "pointer.drag", Target: dw.Target{Anchor: &dw.Anchor{Target: input, U: .2, V: .5}}, Drag: &dw.Drag{To: dw.Target{Anchor: &dw.Anchor{Target: input, U: .8, V: .5}}, Duration: 150 * time.Millisecond}},
		{ID: "scroll", Op: "pointer.scroll", Target: dw.Target{Ref: input}, Scroll: &dw.Scroll{Unit: "wheel_step", DY: 1}},
	}}
	mr, e := scoped.Execute(ctx, mousePlan)
	if e != nil || mr.Outcome != "completed" {
		t.Fatalf("mouse receipt %+v: %v", mr, e)
	}
	deadline = time.Now().Add(time.Second)
	for {
		data, e := os.ReadFile(logPath)
		if e != nil {
			t.Fatal(e)
		}
		count := 0
		for _, line := range strings.Split(string(data), "\n") {
			var ev struct{ Event string }
			if json.Unmarshal([]byte(line), &ev) == nil && ev.Event == "submit" {
				count++
			}
		}
		if count == 2 {
			break
		}
		if count > 2 || time.Now().After(deadline) {
			t.Fatalf("pointer click independent submit count=%d, want 2", count)
		}
		time.Sleep(20 * time.Millisecond)
	}
	if path := os.Getenv("DW_NATIVE_CAPTURE_PATH"); path != "" {
		captureActor, e := w.NewActor(ctx, dw.ActorConfig{ID: "fixture-capture", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"capture", "read_asset"}})
		if e != nil {
			t.Fatal(e)
		}
		image, e := captureActor.Capture(ctx, dw.CaptureRequest{Kind: "visible_region", Target: window, MaxPixelWidth: 1000, MaxPixelHeight: 800})
		if e != nil || len(image.Tiles) == 0 {
			t.Fatalf("capture %+v: %v", image, e)
		}
		asset, e := captureActor.ReadAsset(ctx, image.Tiles[0].Asset)
		if e != nil {
			t.Fatal(e)
		}
		if e = os.WriteFile(path, asset.Bytes, 0600); e != nil {
			t.Fatal(e)
		}
	}

	if os.Getenv("DW_NATIVE_CANCEL_TEST") == "1" {
		interrupted, e := scoped.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":cancel-drag"), Steps: []dw.Step{{ID: "cancel-drag", Op: "pointer.drag", Target: dw.Target{Anchor: &dw.Anchor{Target: input, U: .1, V: .5}}, Drag: &dw.Drag{To: dw.Target{Anchor: &dw.Anchor{Target: input, U: .9, V: .5}}, Duration: 2 * time.Second}, Timeout: 100 * time.Millisecond}}})
		if e == nil || interrupted.Outcome == "completed" {
			t.Fatalf("drag cancellation falsely completed: %+v %v", interrupted, e)
		}
		// A read waits behind native cleanup, then a new effect must be admitted.
		_, _ = scoped.ReadText(ctx, dw.TextRequest{Target: input})
		time.Sleep(30 * time.Millisecond)
		recovered, e := scoped.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":after-cancel"), Steps: []dw.Step{{ID: "move", Op: "pointer.move", Target: dw.Target{Ref: input}}}})
		if e != nil || recovered.Outcome != "completed" {
			t.Fatalf("seat did not recover after paired drag cleanup: %+v %v", recovered, e)
		}
	}

	// Replacing the control must not silently rebind the old public Ref.
	var replace dw.Ref
	for _, o := range outline.Objects {
		if o.Role == "button" && o.Name.Value != nil && *o.Name.Value == "替换输入框" {
			replace = o.Ref
		}
	}
	if replace != "" {
		// The host explicitly grants invoke for the two known fixture controls only.
		replacer, e := w.NewActor(ctx, dw.ActorConfig{ID: "fixture-replace", ReadScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{replace}}}, Operations: []string{"invoke"}})
		if e != nil {
			t.Fatal(e)
		}
		rr, e := replacer.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":replace"), Steps: []dw.Step{{ID: "replace", Op: "invoke", Target: dw.Target{Ref: replace}}}})
		if e != nil || rr.Outcome != "completed" {
			t.Fatalf("replace %+v %v", rr, e)
		}
		var next dw.Ref
		deadline = time.Now().Add(time.Second)
		for next == "" && time.Now().Before(deadline) {
			fresh, e := scoped.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Budget: dw.Budget{MaxDepth: 6, ReadDeadline: time.Second}})
			if e != nil {
				t.Fatal(e)
			}
			for _, o := range fresh.Objects {
				if o.Role == "text_field" && o.Name.Value != nil && *o.Name.Value == "内容" && o.Ref != input {
					next = o.Ref
				}
			}
		}
		if next == "" {
			t.Fatal("replacement did not produce a new Ref")
		}
		stale, e := scoped.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":old-ref"), Steps: []dw.Step{{ID: "write", Op: "set_value", Target: dw.Target{Ref: input}, SetValue: &dw.SetValue{Text: "MUST NOT ARRIVE"}}}})
		if e == nil || stale.Outcome != "stopped" || stale.Steps[0].Delivery != dw.DeliveryNone {
			t.Fatalf("old ref was not refused: %+v %v", stale, e)
		}
		current, e := scoped.ReadText(ctx, dw.TextRequest{Target: next})
		if e != nil || current.Text.Value == nil || *current.Text.Value != "" {
			t.Fatalf("replacement value changed: %+v %v", current, e)
		}
	}
	t.Logf("native fixture passed: %s topology=%d, 6-step plan, independent keyboard submit=1, pointer submit=1, drag/scroll dispatched", env.Platform, env.Topology)
}
