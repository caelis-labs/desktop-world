// discoveryprobe measures native coverage without exposing unrelated desktop text.
// It is read-only, does not request permissions, and never activates a window.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
	"github.com/caelis-labs/desktop-world/poc/native-go-runtime/internal/grantcandidate"
)

type page struct {
	Projection        dw.Projection `json:"projection"`
	Number            int           `json:"number"`
	Count             int           `json:"count"`
	TargetApps        int           `json:"target_apps"`
	TargetWins        int           `json:"target_windows"`
	CaptureAppMatches bool          `json:"capture_app_matches,omitempty"`
	Complete          bool          `json:"complete"`
	Dirty             bool          `json:"dirty"`
	Truncated         bool          `json:"truncated"`
	More              bool          `json:"more"`
	Visited           int           `json:"visited"`
	Unavailable       []string      `json:"unavailable"`
	OwnedObjects      []string      `json:"owned_objects,omitempty"`
}

func main() {
	title := flag.String("title", "", "exact owned test window title; never printed")
	appName := flag.String("app", "", "exact owned test app name; never printed")
	flag.Parse()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	w, err := local.Open(ctx, local.Options{})
	if err != nil {
		fail(err)
	}
	defer w.Close(context.Background())
	a, err := w.NewActor(ctx, dw.ActorConfig{ID: "poc-discovery-coverage", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if err != nil {
		fail(err)
	}
	defer a.Close()
	var ownedApp dw.Ref
	var appInventory dw.Observation
	detailApps := map[dw.Ref]bool{}
	detailClean := true
	for _, projection := range []dw.Projection{dw.ProjectionDetail, dw.ProjectionSummary, dw.ProjectionCaptureWindows} {
		continuation := ""
		for i := 0; i < 8; i++ {
			ob, err := a.Observe(ctx, dw.ObserveRequest{
				Scope: dw.Scope{Desktop: true}, Projection: projection,
				Fields:       []string{"name", "role", "app", "lifecycle"},
				Freshness:    dw.Freshness{Mode: "refresh"},
				Budget:       dw.Budget{MaxResults: 1024, MaxVisitedNodes: 10000, MaxOutputBytes: 1 << 20, ReadDeadline: 2 * time.Second},
				Continuation: continuation,
			})
			if err != nil {
				fail(err)
			}
			p := page{Projection: projection, Number: i + 1, Count: len(ob.Objects), Complete: ob.Coverage.Complete,
				Dirty: ob.Coverage.Dirty, Truncated: ob.Coverage.Truncated, More: ob.Coverage.Continuation != "",
				Visited: ob.Coverage.VisitedNodes, Unavailable: ob.Coverage.UnavailableSources}
			if projection == dw.ProjectionDetail {
				appInventory.Objects = append(appInventory.Objects, ob.Objects...)
				appInventory.Coverage = ob.Coverage
				if ob.Coverage.Dirty || (ob.Coverage.Truncated && ob.Coverage.Continuation == "") || len(ob.Coverage.UnavailableSources) != 0 {
					detailClean = false
				}
			}
			for _, o := range ob.Objects {
				if o.Name.Status != dw.FactKnown || o.Name.Value == nil {
					continue
				}
				if o.Kind == dw.KindApplication && *o.Name.Value == *appName {
					p.TargetApps++
					if projection == dw.ProjectionDetail {
						detailApps[o.Ref] = true
					}
				}
				if o.Kind == dw.KindWindow && *o.Name.Value == *title {
					p.TargetWins++
					if projection == dw.ProjectionCaptureWindows {
						p.CaptureAppMatches = ownedApp != "" && o.App == ownedApp
					}
				}
			}
			b, _ := json.Marshal(p)
			fmt.Println(string(b))
			continuation = ob.Coverage.Continuation
			if continuation == "" {
				if projection == dw.ProjectionDetail {
					detailClean = detailClean && ob.Coverage.Complete
					if !detailClean {
						appInventory.Coverage.Complete = false
					}
					if detailClean && len(detailApps) == 1 {
						for ref := range detailApps {
							ownedApp = ref
						}
					}
				}
				break
			}
			if i == 7 && projection == dw.ProjectionDetail {
				detailClean = false
			}
		}
	}
	if ownedApp != "" {
		for _, projection := range []dw.Projection{dw.ProjectionSummary, dw.ProjectionOutline, dw.ProjectionCaptureWindows} {
			ob, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{ownedApp}},
				Projection: projection, Fields: []string{"name", "role", "app", "lifecycle"},
				Freshness: dw.Freshness{Mode: "refresh"}, Budget: dw.Budget{MaxResults: 64, MaxDepth: 3, MaxVisitedNodes: 512, MaxOutputBytes: 1 << 16, ReadDeadline: 2 * time.Second}})
			if err != nil {
				fail(err)
			}
			p := page{Projection: projection, Number: 0, Count: len(ob.Objects), Complete: ob.Coverage.Complete,
				Dirty: ob.Coverage.Dirty, Truncated: ob.Coverage.Truncated, More: ob.Coverage.Continuation != "",
				Visited: ob.Coverage.VisitedNodes, Unavailable: ob.Coverage.UnavailableSources}
			for _, o := range ob.Objects {
				if o.Kind == dw.KindWindow && o.Name.Status == dw.FactKnown && o.Name.Value != nil && *o.Name.Value == *title {
					p.TargetWins++
				}
				name := ""
				if o.Name.Status == dw.FactKnown && o.Name.Value != nil {
					name = *o.Name.Value
				}
				p.OwnedObjects = append(p.OwnedObjects, string(o.Kind)+"/"+o.Role+"/"+name)
			}
			b, _ := json.Marshal(p)
			fmt.Println(string(b))
		}
		var captures [2]dw.Observation
		for i := 0; i < 2; i++ {
			ob, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{ownedApp}},
				Projection: dw.ProjectionCaptureWindows, Fields: []string{"name", "role", "app", "lifecycle"},
				Freshness: dw.Freshness{Mode: "refresh"}, Budget: dw.Budget{MaxResults: 64, MaxVisitedNodes: 512, MaxOutputBytes: 1 << 16, ReadDeadline: 2 * time.Second}})
			if err != nil {
				fail(err)
			}
			captures[i] = ob
		}
		candidate, state := grantcandidate.Resolve(*appName, *title, appInventory, captures[0], captures[1])
		b, _ := json.Marshal(map[string]any{"resolution": state, "stable_exact_native_window": candidate.CaptureWindow != "",
			"authorizes_ax_action": false})
		fmt.Println(string(b))
	} else {
		b, _ := json.Marshal(map[string]any{"resolution": "unresolved", "reason": "exact_app_inventory_not_complete_unique"})
		fmt.Println(string(b))
	}
}

func fail(err error) { fmt.Fprintln(os.Stderr, err); os.Exit(1) }
