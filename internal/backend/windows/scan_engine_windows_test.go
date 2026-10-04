//go:build windows && amd64

package windows

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/engine"
)

// Controlled provider transitions use the production Windows budget/cursor
// loop through the real engine, without requiring a logged-in CI desktop.
type scanTestDriver struct {
	backend.Driver
	scans *Driver
	delay time.Duration
}

func (d *scanTestDriver) Open(context.Context) error { return nil }
func (d *scanTestDriver) Close(context.Context) error {
	for _, s := range d.scans.scans {
		s.release()
	}
	return nil
}
func (d *scanTestDriver) Environment(context.Context) (dw.Environment, error) {
	return dw.Environment{Platform: "fixture", Topology: 1, Permissions: []dw.Permission{{Name: "accessibility", State: "granted"}}}, nil
}
func (d *scanTestDriver) Query(ctx context.Context, q backend.Query) (backend.Page, error) {
	deadline := scanDeadline(ctx, q)
	q.Desktop = false // The test owns its roots, never enumerate the CI desktop.
	if q.Summary {
		q.Roots = []backend.Key{"app", "window"}
	} else {
		q.Roots = []backend.Key{"window"}
		for i := range 36 {
			q.Roots = append(q.Roots, backend.Key(fmt.Sprintf("node-%02d", i)))
		}
	}
	if q.NoContinuation {
		q.MaxNodes = 1 // Force incomplete internal queries independent of timing.
	}
	s, err := d.scans.beginScan(ctx, q)
	if err != nil {
		return backend.Page{}, err
	}
	return d.scans.scanPage(ctx, q, s, deadline, func(p *backend.Page) {
		if !q.Summary {
			time.Sleep(d.delay)
		}
		key := s.stack[len(s.stack)-1].key
		n := backend.Node{Key: key, Fields: q.Fields, App: "app", Window: "window", Parent: "window", Object: dw.Object{Kind: dw.KindUI, Role: "text", Name: dw.Known(string(key)), Lifecycle: dw.LifeLive}}
		switch key {
		case "app":
			n.App, n.Window, n.Parent, n.Object.Kind = "app", "", "", dw.KindApplication
		case "window":
			n.Parent, n.Object.Kind = "app", dw.KindWindow
		case "node-35":
			n.Object.Role, n.Object.Name = "button", dw.Known("Deep submit")
		}
		p.Nodes = append(p.Nodes, n)
		s.visited++
		s.pop()
	}, func() backend.Seat { return backend.Seat{Health: "ready"} })
}

func scanTestActor(t *testing.T) (dw.World, dw.Actor, *scanTestDriver, dw.Ref) {
	t.Helper()
	ctx := context.Background()
	d := &scanTestDriver{scans: New().(*Driver)}
	w, err := engine.Open(ctx, d, engine.Options{SeatID: t.Name()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { w.Close(ctx) })
	a, err := w.NewActor(ctx, dw.ActorConfig{ID: "scan-test", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe", "sync", "bind"}})
	if err != nil {
		t.Fatal(err)
	}
	inv, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary})
	if err != nil {
		t.Fatal(err)
	}
	for _, o := range inv.Objects {
		if o.Kind == dw.KindWindow {
			return w, a, d, o.Ref
		}
	}
	t.Fatal("window missing")
	return nil, nil, nil, ""
}

func TestTimeBudgetDeliversContinuationThroughEngine(t *testing.T) {
	_, a, d, window := scanTestActor(t)
	d.delay = 30 * time.Millisecond
	ctx := context.Background()
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"name", "role"}, Budget: dw.Budget{MaxDepth: 2, MaxVisitedNodes: 1000, MaxResults: 64, MaxOutputBytes: 16 * 1024, ReadDeadline: 500 * time.Millisecond}}
	first, err := a.Observe(ctx, req)
	if err != nil || first.Coverage.Complete || first.Coverage.Continuation == "" || first.Coverage.VisitedNodes == 0 || first.Coverage.VisitedNodes >= 37 || !strings.Contains(strings.Join(first.Coverage.UnavailableSources, ","), "uia_timeout") {
		t.Fatalf("time-limited partial page not delivered: %+v, %v", first, err)
	}
	for _, o := range first.Objects {
		if o.Name.Value != nil && *o.Name.Value == "Deep submit" {
			t.Fatal("target should be beyond first timed prefix")
		}
	}
	d.delay = 0
	req.Continuation = first.Coverage.Continuation
	next, err := a.Observe(ctx, req)
	if err != nil || !next.Coverage.Dirty || next.Coverage.VisitedNodes <= first.Coverage.VisitedNodes {
		t.Fatal("resume did not preserve frontier", next, err)
	}
	for _, o := range next.Objects {
		if o.Name.Value != nil && *o.Name.Value == "Deep submit" {
			return
		}
	}
	t.Fatal("resume failed to reach distant target")
}

func TestInternalQueriesDoNotRetainUnreachableScans(t *testing.T) {
	w, a, d, window := scanTestActor(t)
	ctx := context.Background()
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"name", "role"}, Budget: dw.Budget{MaxVisitedNodes: 1, MaxResults: 8}}
	first, err := a.Observe(ctx, req)
	if err != nil || first.Coverage.Continuation == "" || len(d.scans.scans) != 1 {
		t.Fatal(first, err)
	}
	env, err := w.Environment(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for i := range 20 {
		changes, err := a.Changes(ctx, dw.ChangeRequest{Cursor: first.Cursor})
		if err != nil || !changes.ResetRequired || changes.ResetReason != "coverage_incomplete" {
			t.Fatal(changes, err)
		}
		_, err = a.Execute(ctx, dw.Plan{Epoch: env.Epoch, RequestID: dw.RequestID(fmt.Sprintf("%s:bind-%d", env.Epoch, i)), Steps: []dw.Step{{ID: "bind", Op: "bind", Bind: &dw.Bind{Name: "target", RequireUnique: true, Locator: dw.Locator{Within: window, Role: "button", MaxDepth: 2}}}}})
		if f, ok := err.(*dw.Fault); !ok || f.Code != "search_incomplete" {
			t.Fatal("incomplete bind outcome changed", err)
		}
		if len(d.scans.scans) != 1 {
			t.Fatalf("internal query retained unreachable cursor: %d", len(d.scans.scans))
		}
	}
	if _, err = a.Observe(ctx, req); err != nil {
		t.Fatal("ordinary observation blocked after abandoned internal scans", err)
	}
	req.Continuation = first.Coverage.Continuation
	resumed, err := a.Observe(ctx, req)
	if err != nil || resumed.Coverage.VisitedNodes != 2 {
		t.Fatal("internal cleanup disturbed caller's retained frontier", resumed, err)
	}
}
