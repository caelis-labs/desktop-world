package helper

import (
	"context"
	"fmt"
	"sort"
	"sync"
	"time"

	dw "github.com/caelis-labs/desktop-world"
)

// ApplicationGrant separates an approved selector from its one-time native binding.
type ApplicationGrant struct {
	ID                               string
	Application                      dw.Ref
	Name, WindowTitle, State, Reason string
}
type GrantStatus struct {
	Turn   string
	Grants []ApplicationGrant
}
type grantWatchKey struct{}
type grantWatch struct {
	mu     sync.Mutex
	cancel context.CancelFunc
	stops  map[dw.Ref]func() bool
	closed bool
}

func (w *grantWatch) add(ref dw.Ref, ctx context.Context) {
	if ctx == nil {
		return
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	if !w.closed && w.stops[ref] == nil {
		w.stops[ref] = context.AfterFunc(ctx, w.cancel)
	}
}
func (w *grantWatch) close() {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.closed = true
	for _, stop := range w.stops {
		stop()
	}
}

func (s *Server) declare(r ControlRequest) error {
	if (r.Name == "") == (r.WindowTitle == "") || len(r.Name)+len(r.WindowTitle) > 4096 || r.Application != "" || r.GrantID != "" {
		return dw.Invalid("declare requires exactly one exact name or window_title")
	}
	g := s.grants
	g.mu.Lock()
	defer g.mu.Unlock()
	if r.Turn != g.turn {
		return dw.NewFault("turn_expired", "no matching active turn", "never_automatically")
	}
	key := "name:" + r.Name
	if r.WindowTitle != "" {
		key = "window:" + r.WindowTitle
	}
	if old := g.declarations[key]; old != nil && old.State != "revoked" && old.State != "expired" {
		return nil
	}
	if len(g.declarations) >= 32 && g.declarations[key] == nil {
		return dw.Invalid("at most 32 application declarations per turn")
	}
	g.sequence++
	g.declarations[key] = &ApplicationGrant{ID: fmt.Sprintf("grant-%d", g.sequence), Name: r.Name, WindowTitle: r.WindowTitle, State: "pending", Reason: "application_not_running"}
	return nil
}
func (s *Server) revoke(r ControlRequest) error {
	if (r.GrantID == "") == (r.Application == "") || r.Name != "" || r.WindowTitle != "" {
		return dw.Invalid("revoke requires grant_id or application")
	}
	g := s.grants
	g.mu.Lock()
	defer g.mu.Unlock()
	if r.Turn != g.turn {
		return dw.NewFault("turn_expired", "no matching active turn", "never_automatically")
	}
	found := false
	app := r.Application
	for _, v := range g.declarations {
		if v.ID == r.GrantID || (r.Application != "" && v.Application == r.Application) {
			found = true
			app = v.Application
			v.State, v.Reason = "revoked", "host_revoked"
		}
	}
	// Revoking a bound declaration revokes that application, including aliases.
	if found && app != "" {
		for _, v := range g.declarations {
			if v.Application == app {
				v.State, v.Reason = "revoked", "host_revoked"
			}
		}
		if cancel := g.appCancels[app]; cancel != nil {
			cancel()
		}
		delete(g.apps, app)
		delete(g.appContexts, app)
		delete(g.appCancels, app)
	}
	if !found {
		return dw.Invalid("unknown application grant")
	}
	return nil
}
func (s *Server) grantStatus(turn string) (GrantStatus, error) {
	g := s.grants
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.turn != turn {
		return GrantStatus{}, dw.NewFault("turn_expired", "no matching active turn", "never_automatically")
	}
	out := GrantStatus{Turn: turn, Grants: []ApplicationGrant{}}
	for _, v := range g.declarations {
		out.Grants = append(out.Grants, *v)
	}
	sort.Slice(out.Grants, func(i, j int) bool { return out.Grants[i].ID < out.Grants[j].ID })
	return out, nil
}

// Refresh is bounded and never rebinds an already-bound (including expired) declaration.
// The discovery actor is read-only and does not depend on the grant being resolved.
func (s *Server) refreshGrants(ctx context.Context, turn string) {
	if s.grants == nil || s.discovery == nil {
		return
	}
	g := s.grants
	g.mu.Lock()
	var pending []ApplicationGrant
	var active []ApplicationGrant
	if g.turn != turn {
		g.mu.Unlock()
		return
	}
	for _, v := range g.declarations {
		if v.State == "pending" || v.State == "ambiguous" || v.State == "unresolved" {
			pending = append(pending, *v)
		} else if v.State == "active" {
			active = append(active, *v)
		}
	}
	g.mu.Unlock()
	var inventory dw.Observation
	var inventoryErr error
	inventoryRead := false
	getInventory := func() (dw.Observation, error) {
		if !inventoryRead {
			inventoryRead = true
			inventory, inventoryErr = s.discovery.Observe(ctx, dw.ObserveRequest{Freshness: dw.Freshness{Mode: "refresh"}, Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Fields: []string{"name", "role", "app", "lifecycle"}, Budget: dw.Budget{MaxResults: 1024, MaxVisitedNodes: 10000, MaxOutputBytes: 1 << 20, ReadDeadline: 2 * time.Second}})
		}
		return inventory, inventoryErr
	}
	for _, v := range active {
		ob, e := s.discovery.Observe(ctx, dw.ObserveRequest{Freshness: dw.Freshness{Mode: "refresh"}, Scope: dw.Scope{Refs: []dw.Ref{v.Application}}, Projection: dw.ProjectionDetail, Fields: []string{"kind", "lifecycle"}, Budget: dw.Budget{MaxResults: 1, ReadDeadline: time.Second}})
		// A provider timeout is not proof of process exit. Native reads still check lifecycle at dispatch.
		if e == nil && len(ob.Objects) == 1 && ob.Objects[0].Lifecycle == dw.LifeLive {
			continue
		}
		if e == nil && (!ob.Coverage.Complete || ob.Coverage.Dirty || ob.Coverage.Truncated || len(ob.Coverage.UnavailableSources) > 0) {
			// AX/UIA may report a vanished scoped root as incomplete instead of ref_gone.
			// Only a complete desktop application inventory proves its absence.
			all, err := getInventory()
			if err != nil || !all.Coverage.Complete || all.Coverage.Dirty || all.Coverage.Truncated || len(all.Coverage.UnavailableSources) > 0 {
				continue
			}
			present := false
			for _, o := range all.Objects {
				if o.Ref == v.Application {
					present = true
					break
				}
			}
			if present {
				continue
			}
		}
		if e != nil {
			f, ok := e.(*dw.Fault)
			if !ok || (f.Code != "ref_gone" && f.Code != "ref_expired") {
				continue
			}
		}
		g.mu.Lock()
		if g.turn != turn {
			g.mu.Unlock()
			return
		}
		current := false
		for _, cur := range g.declarations {
			if cur.ID == v.ID && cur.State == "active" {
				current = true
			}
		}
		if !current {
			g.mu.Unlock()
			continue
		}
		if cur := g.declarations["ref:"+string(v.Application)]; cur != nil && cur.State == "active" {
			cur.State, cur.Reason = "expired", "application_exited"
		}
		for _, cur := range g.declarations {
			if cur.Application == v.Application && cur.State == "active" {
				cur.State, cur.Reason = "expired", "application_exited"
			}
		}
		if c := g.appCancels[v.Application]; c != nil {
			c()
		}
		delete(g.apps, v.Application)
		delete(g.appContexts, v.Application)
		delete(g.appCancels, v.Application)
		g.mu.Unlock()
	}
	if len(pending) == 0 {
		return
	}
	ob, e := getInventory()
	for _, v := range pending {
		state, reason := "pending", "application_not_running"
		matches := map[dw.Ref]bool{}
		if e != nil || !ob.Coverage.Complete || ob.Coverage.Dirty || ob.Coverage.Truncated || len(ob.Coverage.UnavailableSources) > 0 {
			state, reason = "unresolved", "discovery_incomplete"
		} else {
			for _, o := range ob.Objects {
				if o.Name.Value == nil || o.Name.Status != dw.FactKnown {
					continue
				}
				if v.Name != "" && o.Kind == dw.KindApplication && *o.Name.Value == v.Name {
					matches[o.Ref] = true
				}
				if v.WindowTitle != "" && o.Kind == dw.KindWindow && *o.Name.Value == v.WindowTitle && o.App != "" {
					matches[o.App] = true
				}
			}
			if len(matches) > 1 {
				state, reason = "ambiguous", "multiple_application_instances"
			} else if len(matches) == 1 {
				state, reason = "active", ""
			}
		}
		key := "name:" + v.Name
		if v.WindowTitle != "" {
			key = "window:" + v.WindowTitle
		}
		g.mu.Lock()
		cur := g.declarations[key]
		if g.turn == turn && cur != nil && cur.ID == v.ID && (cur.State == "pending" || cur.State == "ambiguous" || cur.State == "unresolved") {
			cur.State, cur.Reason = state, reason
			if state == "active" {
				for app := range matches {
					cur.Application = app
					g.apps[app] = true
					if g.appContexts[app] == nil {
						g.appContexts[app], g.appCancels[app] = context.WithCancel(g.ctx)
					}
				}
			}
		}
		g.mu.Unlock()
	}
}
