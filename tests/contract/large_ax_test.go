package contract_test

import (
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
	"strings"
	"testing"
	"time"
)

func TestLargeSlowAXScanResumesBeyondOldPrefix(t *testing.T) {
	ctx := context.Background()
	w, f, err := dwtest.New(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close(ctx)
	f.Add(dwtest.Node{ID: "app", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Large AX Fixture")}})
	f.Add(dwtest.Node{ID: "window", App: "app", Parent: "app", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("Large AX Window")}})
	for i := 0; i < 20000; i++ {
		name := dw.Unknown[string]()
		role := "text"
		if i == 3800 {
			name, role = dw.Known("Unique distant control"), "button"
		}
		f.Add(dwtest.Node{ID: fmt.Sprintf("node-%05d", i), App: "app", Window: "window", Parent: "window", Object: dw.Object{Kind: dw.KindUI, Role: role, Name: name}})
	}
	a, err := w.NewActor(ctx, dw.ActorConfig{ID: "large-scan", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe", "sync"}})
	if err != nil {
		t.Fatal(err)
	}
	inventory, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary})
	if err != nil {
		t.Fatal(err)
	}
	var window dw.Ref
	for _, o := range inventory.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == "Large AX Window" {
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatal("fixture window absent")
	}
	f.SetSlowQuery(time.Microsecond)
	name := "Unique distant control"
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name"}, Match: &dw.Locator{Within: window, Role: "button", NameEquals: &name}, Budget: dw.Budget{MaxDepth: 2, MaxVisitedNodes: 600, MaxResults: 16, MaxOutputBytes: 4096, ReadDeadline: 8 * time.Second}}
	start := time.Now()
	previous, modelBytes, calls := 0, 0, 0
	found := false
	var resume string
	for ; calls < 12; calls++ {
		ob, err := a.Observe(ctx, req)
		if err != nil {
			t.Fatal(err)
		}
		if ob.Coverage.VisitedNodes <= previous || ob.Coverage.VisitedNodes > 10000 {
			t.Fatalf("cursor did not advance: %d -> %d", previous, ob.Coverage.VisitedNodes)
		}
		previous = ob.Coverage.VisitedNodes
		raw, _ := protocol.Marshal(ob)
		compact, err := protocol.CompactJSON(raw)
		if err != nil {
			t.Fatal(err)
		}
		modelBytes += len(compact)
		if len(compact) > 4096 {
			t.Fatalf("model page exceeded budget: %d", len(compact))
		}
		if len(ob.Objects) > 0 {
			if len(ob.Objects) != 1 || ob.Objects[0].Role != "button" || ob.Objects[0].Name.Value == nil || *ob.Objects[0].Name.Value != name {
				t.Fatalf("wrong match: %+v", ob.Objects)
			}
			found = true
			resume = ob.Coverage.Continuation
			break
		}
		if ob.Coverage.Complete || ob.Coverage.Continuation == "" {
			t.Fatalf("zero-match partial had no recovery: %+v", ob.Coverage)
		}
		req.Continuation = ob.Coverage.Continuation
		// Managed-host envelope reservation can change by a byte as request IDs
		// grow. This must not invalidate the native traversal cursor.
		req.Budget.MaxOutputBytes--
	}
	if !found || previous <= 3400 {
		t.Fatalf("distant target unreached: found=%t visited=%d", found, previous)
	}
	t.Logf("N=20000 delay=1us target_index=3800 calls=%d visited=%d hits=1 model_bytes=%d elapsed=%s complete=false (scan still bounded)", calls+1, previous, modelBytes, time.Since(start))
	req.Continuation = resume
	limitFound := false
	for i := 0; i < 20 && req.Continuation != ""; i++ {
		ob, err := a.Observe(ctx, req)
		if err != nil {
			t.Fatal(err)
		}
		if ob.Coverage.VisitedNodes <= previous || ob.Coverage.VisitedNodes > 10000 {
			t.Fatalf("unbounded or stalled scan: %d -> %d", previous, ob.Coverage.VisitedNodes)
		}
		previous = ob.Coverage.VisitedNodes
		if ob.Coverage.Continuation == "" {
			if ob.Coverage.Complete || previous != 10000 || !strings.Contains(strings.Join(ob.Coverage.UnavailableSources, ","), "ax_scan_limit") {
				t.Fatalf("limit not explicit: %+v", ob.Coverage)
			}
			limitFound = true
			break
		}
		req.Continuation = ob.Coverage.Continuation
	}
	if !limitFound {
		t.Fatal("scan did not report its finite limit")
	}
	t.Logf("finite recovery cap visited=%d complete=false source=ax_scan_limit", previous)
}

func TestLargeAXModelBudgetIsCumulative(t *testing.T) {
	ctx := context.Background()
	w, f, err := dwtest.New(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close(ctx)
	f.Add(dwtest.Node{ID: "app", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Budget Fixture")}})
	f.Add(dwtest.Node{ID: "window", App: "app", Parent: "app", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("Budget Window")}})
	for i := 0; i < 2000; i++ {
		f.Add(dwtest.Node{ID: fmt.Sprintf("link-%04d", i), App: "app", Window: "window", Parent: "window", Object: dw.Object{Kind: dw.KindUI, Role: "link", Name: dw.Known(fmt.Sprintf("Named link %04d", i))}})
	}
	a, err := w.NewActor(ctx, dw.ActorConfig{ID: "budget-scan", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if err != nil {
		t.Fatal(err)
	}
	inv, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary})
	if err != nil {
		t.Fatal(err)
	}
	var window dw.Ref
	for _, o := range inv.Objects {
		if o.Kind == dw.KindWindow {
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatal("window absent")
	}
	f.SetSlowQuery(time.Nanosecond)
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name"}, Match: &dw.Locator{Within: window, Role: "link"}, Budget: dw.Budget{MaxDepth: 2, MaxVisitedNodes: 600, MaxResults: 50, MaxOutputBytes: 4096, ReadDeadline: 8 * time.Second}}
	bytes, calls, limited := 0, 0, false
	for ; calls < 30; calls++ {
		ob, err := a.Observe(ctx, req)
		if err != nil {
			t.Fatal(err)
		}
		raw, _ := protocol.Marshal(ob)
		compact, err := protocol.CompactJSON(raw)
		if err != nil {
			t.Fatal(err)
		}
		bytes += len(compact)
		if len(compact) > 4096 {
			t.Fatalf("page too large: %d", len(compact))
		}
		if strings.Contains(strings.Join(ob.Coverage.UnavailableSources, ","), "ax_output_limit") {
			if ob.Coverage.Complete || ob.Coverage.Continuation != "" {
				t.Fatalf("output cap hid uncertainty: %+v", ob.Coverage)
			}
			limited = true
			break
		}
		if ob.Coverage.Continuation == "" {
			t.Fatal("broad scan finished without its output cap")
		}
		req.Continuation = ob.Coverage.Continuation
	}
	if !limited || bytes > 30*1024 {
		t.Fatalf("unbounded model projection: calls=%d bytes=%d limited=%t", calls+1, bytes, limited)
	}
	t.Logf("broad scan calls=%d model_bytes=%d terminal=ax_output_limit complete=false", calls+1, bytes)
}
