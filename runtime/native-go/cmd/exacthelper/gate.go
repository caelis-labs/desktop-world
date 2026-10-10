//go:build darwin && cgo && dtw_poc_exactgrant

package main

import (
	"context"
	"errors"
	"sync"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/engine"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"github.com/caelis-labs/desktop-world/runtime/native-go/internal/exactgrant"
)

type identitySource interface {
	POCWindowIdentity(context.Context, dw.Ref) (engine.POCWindowIdentity, error)
}

type gate struct {
	mu        sync.Mutex
	turn      string
	discovery dw.Actor
	source    identitySource
	apps      map[dw.Ref]bool
	windows   map[string]boundWindow
}

type boundWindow struct {
	app   dw.Ref
	ref   dw.Ref
	grant exactgrant.Grant
}

func newGate(source identitySource) *gate {
	return &gate{source: source, apps: map[dw.Ref]bool{}, windows: map[string]boundWindow{}}
}

func (g *gate) reset(turn string) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.turn, g.apps, g.windows = turn, map[dw.Ref]bool{}, map[string]boundWindow{}
}

type authorizerFunc func(context.Context, dw.Intent) (dw.Decision, error)

func (f authorizerFunc) Check(ctx context.Context, in dw.Intent) (dw.Decision, error) {
	return f(ctx, in)
}

type guardedWorld struct {
	dw.World
	gate *gate
}

func (w guardedWorld) NewActor(ctx context.Context, c dw.ActorConfig) (dw.Actor, error) {
	if c.ID == "helper-agent" {
		original := c.Authorizer
		c.Authorizer = authorizerFunc(func(ctx context.Context, in dw.Intent) (dw.Decision, error) {
			if original == nil {
				return dw.Decision{}, errors.New("missing host authorizer")
			}
			decision, err := original.Check(ctx, in)
			if err != nil || !decision.Allow {
				return decision, err
			}
			return w.gate.Check(ctx, in)
		})
	}
	actor, err := w.World.NewActor(ctx, c)
	if err == nil && c.ID == "helper-discovery" {
		w.gate.mu.Lock()
		w.gate.discovery = actor
		w.gate.mu.Unlock()
	}
	return actor, err
}

func toGrantIdentity(i engine.POCWindowIdentity) exactgrant.Identity {
	if i.StartSec <= 0 || i.StartUSec < 0 || i.StartUSec >= 1_000_000 {
		return exactgrant.Identity{}
	}
	return exactgrant.Identity{PID: i.PID, ProcessStart: uint64(i.StartSec)*1_000_000 + uint64(i.StartUSec),
		NativeWindowID: i.NativeWindowID, ApplicationRef: string(i.Application), AXWindowRef: string(i.Window)}
}

// syncStatus runs only after a trusted control request. It never changes a
// successfully bound grant; a changed title, PID or native window stays stale.
func (g *gate) syncStatus(ctx context.Context, status helper.GrantStatus) {
	g.mu.Lock()
	if g.turn != status.Turn {
		g.mu.Unlock()
		return
	}
	discovery := g.discovery
	bound := make(map[string]bool, len(g.windows))
	for id := range g.windows {
		bound[id] = true
	}
	g.mu.Unlock()
	apps := map[dw.Ref]bool{}
	for _, item := range status.Grants {
		if item.State == "active" && item.Application != "" && item.WindowTitle == "" {
			apps[item.Application] = true // explicit App/Name grants retain App scope.
		}
	}
	for _, item := range status.Grants {
		if item.State != "active" || item.WindowTitle == "" || item.Application == "" || bound[item.ID] || discovery == nil {
			continue
		}
		ob, err := discovery.Observe(ctx, dw.ObserveRequest{Freshness: dw.Freshness{Mode: "refresh"},
			Scope: dw.Scope{Refs: []dw.Ref{item.Application}}, Projection: dw.ProjectionSummary,
			Fields: []string{"name", "role", "app", "lifecycle"},
			Match:  &dw.Locator{Within: item.Application, NameEquals: &item.WindowTitle},
			Budget: dw.Budget{MaxResults: 32, MaxVisitedNodes: 512, MaxDepth: 12,
				MaxOutputBytes: 32768, ReadDeadline: 2 * time.Second}})
		if err != nil || !ob.Coverage.Complete || ob.Coverage.Dirty || ob.Coverage.Truncated ||
			ob.Coverage.Continuation != "" || len(ob.Coverage.UnavailableSources) != 0 {
			continue
		}
		var ref dw.Ref
		for _, object := range ob.Objects {
			if object.Kind != dw.KindWindow || object.App != item.Application || object.Lifecycle != dw.LifeLive ||
				object.Name.Status != dw.FactKnown || object.Name.Value == nil || *object.Name.Value != item.WindowTitle {
				continue
			}
			if ref != "" {
				ref = ""
				break
			}
			ref = object.Ref
		}
		if ref == "" {
			continue
		}
		fresh, err := g.source.POCWindowIdentity(ctx, ref)
		if err != nil || fresh.Window != ref || fresh.Application != item.Application {
			continue
		}
		identity := toGrantIdentity(fresh)
		grant, err := exactgrant.Bind(identity, identity)
		if err != nil {
			continue
		}
		g.mu.Lock()
		if g.turn == status.Turn && g.windows[item.ID].ref == "" {
			g.windows[item.ID] = boundWindow{app: item.Application, ref: ref, grant: grant}
		}
		g.mu.Unlock()
	}
	g.mu.Lock()
	if g.turn == status.Turn {
		g.apps = apps
		for id := range g.windows {
			active := false
			for _, item := range status.Grants {
				if item.ID == id && item.State == "active" {
					active = true
					break
				}
			}
			if !active {
				delete(g.windows, id)
			}
		}
	}
	g.mu.Unlock()
}

func (g *gate) visibleStatus(ctx context.Context, status helper.GrantStatus) helper.GrantStatus {
	g.mu.Lock()
	bound := make(map[string]boundWindow, len(g.windows))
	for id, value := range g.windows {
		bound[id] = value
	}
	g.mu.Unlock()
	for i := range status.Grants {
		item := &status.Grants[i]
		if item.State != "active" || item.WindowTitle == "" {
			continue
		}
		window := bound[item.ID]
		if window.ref == "" {
			item.State, item.Reason = "unresolved", "window_identity_unresolved"
			continue
		}
		fresh, err := g.source.POCWindowIdentity(ctx, window.ref)
		if err != nil || window.grant.Check(toGrantIdentity(fresh), []exactgrant.Target{{
			ApplicationRef: string(fresh.Application), AXWindowRef: string(fresh.Window),
			NativeWindowID: fresh.NativeWindowID,
		}}) != nil {
			item.State, item.Reason = "unresolved", "window_identity_unavailable_or_changed"
		}
	}
	return status
}

// Check runs inside the real engine action authorizer before every step.
// The original helper authorizer has already approved the turn/App ceiling.
func (g *gate) Check(ctx context.Context, in dw.Intent) (dw.Decision, error) {
	switch in.Operation {
	case "observe", "read", "sync", "bind", "bind_focus", "wait", "resolve_anchor", "capture", "read_asset":
		return dw.Decision{Allow: true}, nil
	}
	if len(in.Targets) == 0 {
		return dw.Decision{}, nil
	}
	g.mu.Lock()
	apps := make(map[dw.Ref]bool, len(g.apps))
	for app, approved := range g.apps {
		apps[app] = approved
	}
	windows := make([]boundWindow, 0, len(g.windows))
	for _, bound := range g.windows {
		windows = append(windows, bound)
	}
	g.mu.Unlock()
	allExplicitApps := len(in.Applications) > 0
	for _, app := range in.Applications {
		allExplicitApps = allExplicitApps && apps[app]
	}
	if allExplicitApps {
		return dw.Decision{Allow: true}, nil
	}
	for _, target := range in.Targets {
		fresh, err := g.source.POCWindowIdentity(ctx, target)
		if err != nil {
			// App-scoped actions on an App Ref are separately handled below.
			if apps[target] {
				continue
			}
			return dw.Decision{}, nil
		}
		if apps[fresh.Application] {
			continue
		}
		matched := false
		for _, bound := range windows {
			if bound.app != fresh.Application {
				continue
			}
			live, err := g.source.POCWindowIdentity(ctx, bound.ref)
			if err != nil {
				continue
			}
			if bound.grant.Check(toGrantIdentity(live), []exactgrant.Target{{
				ApplicationRef: string(fresh.Application), AXWindowRef: string(fresh.Window),
				NativeWindowID: fresh.NativeWindowID}}) == nil {
				matched = true
				break
			}
		}
		if !matched {
			return dw.Decision{}, nil
		}
	}
	return dw.Decision{Allow: true}, nil
}
