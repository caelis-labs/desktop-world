package acceptance_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"
)

// TestNativeLargeSiteDiscovery reads the selected real Chrome tab through the
// managed helper. It makes no input or screenshot request. The tab is created
// and closed by the operator outside this opt-in test.
func TestNativeLargeSiteDiscovery(t *testing.T) {
	title, label, executable := os.Getenv("DW_LARGE_SITE_TITLE"), os.Getenv("DW_LARGE_SITE_LABEL"), os.Getenv("DW_NATIVE_HELPER_PATH")
	if title == "" || label == "" || executable == "" {
		t.Skip("real Chrome tab and helper required")
	}
	minVisited, _ := strconv.Atoi(os.Getenv("DW_LARGE_SITE_MIN_VISITED"))
	if minVisited == 0 {
		minVisited = 3400
	}
	chunk, _ := strconv.Atoi(os.Getenv("DW_LARGE_SITE_CHUNK"))
	if chunk == 0 {
		chunk = 1200
	}
	lateStart := (minVisited/chunk + 1) * chunk
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	c, err := host.Start(ctx, host.Options{Executable: executable})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	turn := "large-site-" + label
	if err := c.BeginTurn(ctx, turn); err != nil {
		t.Fatal(err)
	}
	inventory, err := c.Call(ctx, turn, "inventory", "observe", dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{MaxVisitedNodes: 2048, MaxResults: 1024, MaxOutputBytes: 1 << 20, ReadDeadline: 10 * time.Second}})
	if err != nil || inventory.Error != nil {
		t.Fatalf("inventory: %v %+v", err, inventory.Error)
	}
	var initial dw.Observation
	if err := protocol.Decode(inventory.Result, &initial); err != nil {
		t.Fatal(err)
	}
	type candidate struct{ window, app dw.Ref }
	var candidates []candidate
	for _, o := range initial.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && strings.HasPrefix(*o.Name.Value, title) {
			candidates = append(candidates, candidate{o.Ref, o.App})
		}
	}
	if len(candidates) == 0 {
		t.Fatalf("Chrome tab window not found for %s", label)
	}
	var window, app dw.Ref
	bestVisited := -1
	for i, candidate := range candidates {
		probe := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{candidate.window}}, Projection: dw.ProjectionOutline, Fields: []string{"role"}, Match: &dw.Locator{Within: candidate.window, Role: "nonexistent"}, Budget: dw.Budget{MaxDepth: 30, MaxVisitedNodes: 1200, MaxResults: 1, MaxOutputBytes: 4096, ReadDeadline: 10 * time.Second}}
		reply, err := c.Call(ctx, turn, fmt.Sprintf("probe-%d", i), "observe", probe)
		if err != nil || reply.Error != nil {
			t.Fatalf("candidate probe %d: %v %+v", i, err, reply.Error)
		}
		var ob dw.Observation
		if err := protocol.Decode(reply.Result, &ob); err != nil {
			t.Fatal(err)
		}
		if ob.Coverage.VisitedNodes > bestVisited {
			bestVisited, window, app = ob.Coverage.VisitedNodes, candidate.window, candidate.app
		}
	}
	t.Logf("%s candidate_windows=%d selected_probe_visited=%d", label, len(candidates), bestVisited)
	if err := c.Grant(ctx, turn, app); err != nil {
		t.Fatal(err)
	}
	base := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "name"}, Match: &dw.Locator{Within: window, Role: "link"}, Budget: dw.Budget{MaxDepth: 30, MaxVisitedNodes: chunk, MaxResults: 50, MaxOutputBytes: 8192, ReadDeadline: 10 * time.Second}}
	if contains := os.Getenv("DW_LARGE_SITE_SURVEY_CONTAINS"); contains != "" {
		base.Match.NameContains = &contains
	}
	var lateName string
	var calls, bytes, hits, maxPage, lastVisited int
	broadCap := os.Getenv("DW_LARGE_SITE_BROAD_CAP") == "1"
	start := time.Now()
	if targetFile := os.Getenv("DW_LARGE_SITE_TARGET_FILE"); targetFile != "" {
		data, err := os.ReadFile(targetFile)
		if err != nil {
			t.Fatal(err)
		}
		lateName = string(data)
	} else {
		for ; calls < 80; calls++ {
			reply, err := c.Call(ctx, turn, fmt.Sprintf("survey-%d", calls), "observe", base)
			if err != nil || reply.Error != nil {
				t.Fatalf("survey call %d: %v %+v", calls+1, err, reply.Error)
			}
			var ob dw.Observation
			if err := protocol.Decode(reply.Result, &ob); err != nil {
				t.Fatal(err)
			}
			body := host.Content(reply).Content[0].Text
			bytes += len(body)
			if len(body) > maxPage {
				maxPage = len(body)
			}
			if len(body) > 8192 {
				t.Fatalf("model page exceeds limit: %d", len(body))
			}
			if ob.Coverage.VisitedNodes < lastVisited {
				t.Fatalf("scan regressed: %d -> %d", lastVisited, ob.Coverage.VisitedNodes)
			}
			lastVisited = ob.Coverage.VisitedNodes
			hits += len(ob.Objects)
			t.Logf("%s survey_page=%d visited=%d hits=%d bytes=%d continuation=%t sources=%v", label, calls+1, ob.Coverage.VisitedNodes, len(ob.Objects), len(body), ob.Coverage.Continuation != "", ob.Coverage.UnavailableSources)
			if broadCap && strings.Contains(strings.Join(ob.Coverage.UnavailableSources, ","), "ax_output_limit") {
				if bytes > 30*1024 || ob.Coverage.Complete || ob.Coverage.Continuation != "" {
					t.Fatalf("broad output cap violated: bytes=%d coverage=%+v", bytes, ob.Coverage)
				}
				t.Logf("%s broad_cap calls=%d visited=%d hits=%d model_bytes=%d max_page_bytes=%d elapsed=%s complete=false source=ax_output_limit", label, calls+1, lastVisited, hits, bytes, maxPage, time.Since(start))
				if err := c.EndTurn(ctx, turn); err != nil {
					t.Fatal(err)
				}
				return
			}
			if !broadCap && ob.Coverage.VisitedNodes > lateStart && ob.Coverage.VisitedNodes <= lateStart+chunk {
				for _, o := range ob.Objects {
					if o.Role == "link" && o.Name.Value != nil && len(*o.Name.Value) >= 8 && len(*o.Name.Value) <= 100 {
						lateName = *o.Name.Value
						break
					}
				}
			}
			if lateName != "" {
				break
			}
			if ob.Coverage.Continuation == "" {
				break
			}
			base.Continuation = ob.Coverage.Continuation
		}
		if lateName == "" {
			t.Fatalf("no named link past old prefix: visited=%d calls=%d hits=%d bytes=%d", lastVisited, calls+1, hits, bytes)
		}
		t.Logf("%s survey calls=%d visited=%d hits=%d model_bytes=%d max_page_bytes=%d elapsed=%s", label, calls+1, lastVisited, hits, bytes, maxPage, time.Since(start))
	}
	sha := sha256.Sum256([]byte(lateName))
	name := lateName
	exact := base
	exact.Continuation = ""
	exact.Match = &dw.Locator{Within: window, Role: "link", NameEquals: &name}
	exact.Budget.MaxResults = 8
	exact.Budget.MaxOutputBytes = 4096
	start = time.Now()
	var exactBytes, firstVisited, firstHits, exactVisited, exactCalls int
	for ; exactCalls < 12; exactCalls++ {
		reply, err := c.Call(ctx, turn, fmt.Sprintf("exact-%d", exactCalls), "observe", exact)
		if err != nil || reply.Error != nil {
			t.Fatalf("exact call %d: %v %+v", exactCalls+1, err, reply.Error)
		}
		var ob dw.Observation
		if err := protocol.Decode(reply.Result, &ob); err != nil {
			t.Fatal(err)
		}
		body := host.Content(reply).Content[0].Text
		exactBytes += len(body)
		if len(body) > 4096 {
			t.Fatalf("exact model page exceeds limit: %d", len(body))
		}
		if exactCalls == 0 {
			firstVisited, firstHits = ob.Coverage.VisitedNodes, len(ob.Objects)
		}
		exactVisited = ob.Coverage.VisitedNodes
		for _, o := range ob.Objects {
			if o.Role == "link" && o.Name.Value != nil && *o.Name.Value == name {
				t.Logf("%s exact calls=%d visited=%d hits=%d model_bytes=%d elapsed=%s first_call_visited=%d first_call_hits=%d target_sha256=%s", label, exactCalls+1, exactVisited, len(ob.Objects), exactBytes, time.Since(start), firstVisited, firstHits, hex.EncodeToString(sha[:]))
				if firstHits != 0 || exactVisited <= minVisited {
					t.Fatal("target did not require traversal progress beyond prior ceiling")
				}
				if err := c.EndTurn(ctx, turn); err != nil {
					t.Fatal(err)
				}
				return
			}
		}
		if ob.Coverage.Continuation == "" {
			break
		}
		exact.Continuation = ob.Coverage.Continuation
	}
	t.Fatalf("exact named target not recovered: calls=%d visited=%d bytes=%d", exactCalls+1, exactVisited, exactBytes)
}
