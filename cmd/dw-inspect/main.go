// dw-inspect is read-only. It never requests permissions or sends input.
package main

import (
	"context"
	"flag"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"time"
)

func main() {
	environment := flag.Bool("environment", false, "show environment and permission state only")
	flag.Parse()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	w, e := local.Open(ctx, local.Options{})
	if e != nil {
		fail(e)
	}
	defer func() {
		c, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = w.Close(c)
	}()
	if *environment {
		v, e := w.Environment(ctx)
		if e != nil {
			fail(e)
		}
		printJSON(v)
		return
	}
	a, e := w.NewActor(ctx, dw.ActorConfig{ID: "inspector", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if e != nil {
		fail(e)
	}
	v, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{ReadDeadline: 3 * time.Second}})
	if e != nil {
		fail(e)
	}
	printJSON(v)
}
func printJSON(v any) {
	b, e := protocol.Marshal(v)
	if e != nil {
		fail(e)
	}
	fmt.Println(string(b))
}
func fail(e error) { fmt.Fprintln(os.Stderr, e); os.Exit(1) }
