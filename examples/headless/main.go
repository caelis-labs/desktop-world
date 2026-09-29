// Run an end-to-end form against the deterministic fixture without OS input.
package main

import (
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
	"time"
)

func main() {
	ctx := context.Background()
	w, f, e := dwtest.New(ctx)
	must(e)
	defer w.Close(ctx)
	f.Form()
	env, e := w.Environment(ctx)
	must(e)
	a, e := w.NewActor(ctx, dw.ActorConfig{ID: "demo", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe", "bind", "focus", "set_value", "keyboard.press"}})
	must(e)
	ob, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary})
	must(e)
	var window dw.Ref
	for _, o := range ob.Objects {
		if o.Kind == dw.KindWindow {
			window = o.Ref
		}
	}
	name := "内容"
	r, e := a.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(string(env.Epoch) + ":demo"), Timeout: 3 * time.Second, Steps: []dw.Step{{ID: "window", Op: "focus", Target: dw.Target{Ref: window}}, {ID: "find", Op: "bind", Bind: &dw.Bind{Name: "input", RequireUnique: true, Locator: dw.Locator{Within: window, Role: "text_field", NameEquals: &name}}}, {ID: "focus", Op: "focus", Target: dw.Target{Bound: "input"}}, {ID: "fill", Op: "set_value", Target: dw.Target{Bound: "input"}, SetValue: &dw.SetValue{Text: "Hello Desktop World 🌍"}}, {ID: "submit", Op: "keyboard.press", Target: dw.Target{Bound: "input"}, Press: &dw.KeyChord{Key: "Enter"}, Completion: "dispatch"}}})
	b, _ := protocol.Marshal(r)
	fmt.Println(string(b))
	must(e)
	fmt.Printf("fixture recorded %d effects\n", len(f.Events()))
}
func must(e error) {
	if e != nil {
		panic(e)
	}
}
