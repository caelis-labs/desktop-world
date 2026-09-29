// A renderer can use the same Ref as actions; resolving an anchor sends no input.
package main

import (
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
)

func main() {
	ctx := context.Background()
	w, f, e := dwtest.New(ctx)
	if e != nil {
		panic(e)
	}
	defer w.Close(ctx)
	f.Form()
	a, e := w.NewActor(ctx, dw.ActorConfig{ID: "character", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe", "resolve_anchor"}})
	if e != nil {
		panic(e)
	}
	ob, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline})
	if e != nil {
		panic(e)
	}
	for _, o := range ob.Objects {
		if o.Role != "button" {
			continue
		}
		anchor, e := a.ResolveAnchor(ctx, dw.Anchor{Target: o.Ref, U: .5, V: 0})
		if e != nil {
			panic(e)
		}
		fmt.Printf("Look at %s in %s: (%g, %g), topology %d; fixture input events: %d\n", o.Ref, anchor.Point.Frame, anchor.Point.X, anchor.Point.Y, anchor.Point.Topology, len(f.Events()))
	}
}
