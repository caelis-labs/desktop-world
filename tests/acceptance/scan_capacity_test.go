package acceptance_test

import (
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"testing"
	"time"
)

// TestNativeScanCapacityPreservesCursor uses the real AppKit fixture and
// managed helper. A new scan at capacity must not evict an existing cursor.
func TestNativeScanCapacityPreservesCursor(t *testing.T) {
	title, executable := os.Getenv("DW_NATIVE_FIXTURE_TITLE"), os.Getenv("DW_NATIVE_HELPER_PATH")
	if title == "" || executable == "" {
		t.Skip("explicit native fixture and helper required")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	c, err := host.Start(ctx, host.Options{Executable: executable})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	const turn = "scan-capacity"
	if err := c.BeginTurn(ctx, turn); err != nil {
		t.Fatal(err)
	}
	reply, err := c.Call(ctx, turn, "inventory", "observe", dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{ReadDeadline: 5 * time.Second, MaxResults: 1024, MaxVisitedNodes: 2048, MaxOutputBytes: 1 << 20}})
	if err != nil || reply.Error != nil {
		t.Fatalf("inventory: %v %+v", err, reply.Error)
	}
	var inv dw.Observation
	if err := protocol.Decode(reply.Result, &inv); err != nil {
		t.Fatal(err)
	}
	var window dw.Ref
	for _, o := range inv.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == title {
			if window != "" {
				t.Fatal("ambiguous fixture window")
			}
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatal("fixture window not found")
	}
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name"}, Budget: dw.Budget{MaxDepth: 6, MaxVisitedNodes: 1, MaxResults: 1, MaxOutputBytes: 4096, ReadDeadline: 2 * time.Second}}
	var first string
	for i := 0; i < 16; i++ {
		reply, err := c.Call(ctx, turn, fmt.Sprintf("scan-%02d", i), "observe", req)
		if err != nil || reply.Error != nil {
			t.Fatalf("scan %d: %v %+v", i, err, reply.Error)
		}
		var ob dw.Observation
		if err := protocol.Decode(reply.Result, &ob); err != nil {
			t.Fatal(err)
		}
		if ob.Coverage.VisitedNodes != 1 || ob.Coverage.Continuation == "" {
			t.Fatalf("scan %d lacked retained continuation: %+v", i, ob.Coverage)
		}
		if i == 0 {
			first = ob.Coverage.Continuation
		}
	}
	reply, err = c.Call(ctx, turn, "scan-over-capacity", "observe", req)
	if err != nil || reply.Error == nil || reply.Error.Code != "ax_scan_capacity" || reply.Error.RetryClass != "never_automatically" {
		t.Fatalf("capacity not explicit: %v %+v", err, reply.Error)
	}
	req.Continuation = first
	reply, err = c.Call(ctx, turn, "resume-first", "observe", req)
	if err != nil || reply.Error != nil {
		t.Fatalf("old cursor was evicted: %v %+v", err, reply.Error)
	}
	var resumed dw.Observation
	if err := protocol.Decode(reply.Result, &resumed); err != nil {
		t.Fatal(err)
	}
	if resumed.Coverage.VisitedNodes != 2 || resumed.Coverage.Continuation == "" {
		t.Fatalf("old cursor did not advance: %+v", resumed.Coverage)
	}
	if err := c.EndTurn(ctx, turn); err != nil {
		t.Fatal(err)
	}
	t.Log("16 active native scan cursors retained; seventeenth returned ax_scan_capacity; first cursor resumed from visited=1 to 2")
}
