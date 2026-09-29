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

func TestBrowserFixture(t *testing.T) {
	title, logPath := os.Getenv("DW_BROWSER_FIXTURE_TITLE"), os.Getenv("DW_BROWSER_FIXTURE_LOG")
	if title == "" || logPath == "" {
		t.Skip("browser native test is opt-in")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	w, err := local.Open(ctx, local.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		c, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		if err := w.Close(c); err != nil {
			t.Error(err)
		}
	}()
	env, err := w.Environment(ctx)
	if err != nil {
		t.Fatal(err)
	}
	discovery, err := w.NewActor(ctx, dw.ActorConfig{ID: "browser-discovery", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if err != nil {
		t.Fatal(err)
	}
	ob, err := discovery.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{MaxResults: 1024, MaxOutputBytes: 1 << 20, MaxVisitedNodes: 2048, ReadDeadline: 10 * time.Second}})
	if err != nil {
		t.Fatal(err)
	}
	var window dw.Ref
	for _, o := range ob.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && strings.HasPrefix(*o.Name.Value, title) {
			if window != "" {
				t.Fatal("ambiguous browser fixture window")
			}
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatal("browser fixture must be the active tab of a uniquely titled window")
	}
	discovery.Close()
	a, err := w.NewActor(ctx, dw.ActorConfig{ID: "browser-fixture-only", ReadScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, Operations: []string{"observe", "bind", "focus", "keyboard.press", "keyboard.type_text"}})
	if err != nil {
		t.Fatal(err)
	}
	outline, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Budget: dw.Budget{MaxDepth: 24, MaxResults: 1024, MaxVisitedNodes: 5000, MaxOutputBytes: 1 << 20, ReadDeadline: 10 * time.Second}})
	if err != nil {
		t.Fatal(err)
	}
	var input dw.Ref
	for _, o := range outline.Objects {
		if o.Role == "text_field" && o.Name.Value != nil && *o.Name.Value == "DW 网页内容" {
			if input != "" {
				t.Fatal("ambiguous field")
			}
			input = o.Ref
		}
	}
	if input == "" {
		t.Fatalf("browser native AX/UIA field absent: objects=%d coverage=%+v", len(outline.Objects), outline.Coverage)
	}
	text := "Browser 原生验收 🌍"
	p := dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":browser"), Steps: []dw.Step{
		{ID: "window", Op: "focus", Target: dw.Target{Ref: window}},
		{ID: "field", Op: "focus", Target: dw.Target{Ref: input}},
		{ID: "select", Op: "keyboard.press", Target: dw.Target{Ref: input}, Press: &dw.KeyChord{Key: "A", Modifiers: []string{"primary"}}},
		{ID: "type", Op: "keyboard.type_text", Target: dw.Target{Ref: input}, TypeText: &dw.TypeText{Text: text}},
		{ID: "submit", Op: "keyboard.press", Target: dw.Target{Ref: input}, Press: &dw.KeyChord{Key: "Enter"}},
	}}
	r, err := a.Execute(ctx, p)
	if err != nil || r.Outcome != "completed" {
		t.Fatalf("browser receipt %+v: %v", r, err)
	}
	second, err := a.Execute(ctx, p)
	if err != nil || second.RunID != r.RunID {
		t.Fatalf("dedup: %+v %v", second, err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for {
		data, err := os.ReadFile(logPath)
		if err != nil {
			t.Fatal(err)
		}
		count, keys, inputs := 0, 0, 0
		for _, line := range strings.Split(string(data), "\n") {
			var event struct {
				Event, Value string
				Trusted      bool
			}
			if json.Unmarshal([]byte(line), &event) != nil {
				continue
			}
			if event.Event == "canvas_click" {
				t.Fatal("unexpected Canvas input")
			}
			switch event.Event {
			case "submit":
				count++
				if event.Value != text || !event.Trusted {
					t.Fatalf("unexpected submit %+v", event)
				}
			case "keydown":
				if event.Trusted {
					keys++
				}
			case "input":
				if event.Trusted {
					inputs++
				}
			}
		}
		if count == 1 && keys > 0 && inputs > 0 {
			break
		}
		if count > 1 || time.Now().After(deadline) {
			t.Fatalf("independent browser evidence: submits=%d keys=%d inputs=%d", count, keys, inputs)
		}
		time.Sleep(20 * time.Millisecond)
	}
	// Pixels with no semantic object must never become an invented native target.
	name := "DW Canvas Secret Action"
	negative, err := a.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":canvas-negative"), Steps: []dw.Step{{ID: "canvas", Op: "bind", Bind: &dw.Bind{Name: "canvas", RequireUnique: true, Locator: dw.Locator{Within: window, Role: "button", NameEquals: &name, MaxDepth: 24}}}}})
	if err == nil || negative.Outcome != "stopped" || len(negative.Steps) != 1 || negative.Steps[0].Delivery != dw.DeliveryNA || len(negative.Bindings) != 0 || negative.Fault == nil || (negative.Fault.Code != "ambiguous_target" && negative.Fault.Code != "search_incomplete") {
		t.Fatalf("invented Canvas target: %+v %v", negative, err)
	}
	if path := os.Getenv("DW_BROWSER_CAPTURE_PATH"); path != "" {
		c, err := w.NewActor(ctx, dw.ActorConfig{ID: "browser-capture", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"capture", "read_asset"}})
		if err != nil {
			t.Fatal(err)
		}
		im, err := c.Capture(ctx, dw.CaptureRequest{Kind: "visible_region", Target: window, MaxPixelWidth: 1200, MaxPixelHeight: 900})
		if err != nil || len(im.Tiles) == 0 {
			t.Fatalf("capture %v", err)
		}
		asset, err := c.ReadAsset(ctx, im.Tiles[0].Asset)
		if err != nil {
			t.Fatal(err)
		}
		if err = os.WriteFile(path, asset.Bytes, 0600); err != nil {
			t.Fatal(err)
		}
	}
	t.Logf("native browser passed: 5-step input, trusted DOM input/key/submit evidence, request dedup, Canvas negative; coverage complete=%t", outline.Coverage.Complete)
}
