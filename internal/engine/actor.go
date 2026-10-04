package engine

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/wire"
	"reflect"
	"sort"
	"strings"
	"time"
)

type actor struct {
	textVersions    map[dw.Ref]textState
	nextTextVersion dw.Version
	w               *World
	config          dw.ActorConfig
	closed          bool
	views           map[dw.Cursor]*view
	pages           map[string]*page
	texts           map[string]textPage
	assets          map[dw.AssetID]assetRecord
}
type view struct {
	topology          dw.Version
	request           dw.ObserveRequest
	objects           []dw.Object
	seat              dw.SeatState
	coverage          dw.Coverage
	rev               dw.Revision
	at                time.Time
	permissionVersion uint64
	offset, limit     int
}
type page struct {
	permissionVersion uint64
	request           dw.ObserveRequest
	objects           []dw.Object
	seat              dw.SeatState
	coverage          dw.Coverage
	rev               dw.Revision
	env               dw.Environment
	offset            int
	scanCursor        string
	outputBytes       int
	expires           time.Time
}

func (a *actor) ID() dw.ActorID { return a.config.ID }
func (a *actor) invalidateLocked() {
	a.views = map[dw.Cursor]*view{}
	a.pages = map[string]*page{}
	a.texts = map[string]textPage{}
	a.textVersions = map[dw.Ref]textState{}
	a.assets = map[dw.AssetID]assetRecord{}
}
func (a *actor) Close() error {
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	a.closed = true
	a.invalidateLocked()
	for _, r := range a.w.runs {
		if r.actor == a {
			r.cancel()
		}
	}
	return nil
}
func (a *actor) allows(op string) bool {
	for _, x := range a.config.Operations {
		if x == op {
			return true
		}
	}
	return false
}
func (a *actor) inScopeLocked(r dw.Ref, scopes []dw.Scope) bool {
	for _, s := range scopes {
		if s.Desktop {
			return true
		}
		for _, root := range s.Refs {
			if r == root {
				return true
			}
			if obj, base := a.w.objects[r], a.w.objects[root]; obj != nil && base != nil {
				if base.object.Kind == dw.KindWindow && obj.object.Window != "" {
					if obj.object.Window == root {
						return true
					}
					continue
				}
				if base.object.Kind == dw.KindApplication && obj.object.App != "" {
					if obj.object.App == root {
						return true
					}
					continue
				}
			}
			cur := r
			seen := map[dw.Ref]bool{}
			for cur != "" && !seen[cur] {
				if cur == root {
					return true
				}
				seen[cur] = true
				n := a.w.objects[cur]
				if n == nil {
					break
				}
				if n.object.App == root || n.object.Window == root {
					return true
				}
				cur = n.object.Parent
			}
		}
	}
	return false
}
func (a *actor) check(ctx context.Context, in dw.Intent, write bool) error {
	if !a.config.InputPolicy.Allows(in.Operation) {
		return dw.NewFault("requires_shared_input", "host policy forbids focus and shared input", "never_automatically")
	}
	if e := ctx.Err(); e != nil {
		return e
	}
	a.w.mu.Lock()
	if a.closed || a.w.closing {
		a.w.mu.Unlock()
		return fault("actor_closed")
	}
	scopes := a.config.ReadScopes
	if write {
		scopes = a.config.WriteScopes
	}
	ok := a.allows(in.Operation)
	if in.Scope.Desktop {
		desktop := false
		for _, s := range scopes {
			desktop = desktop || s.Desktop
		}
		ok = ok && desktop
	}
	for _, r := range append(append([]dw.Ref{}, in.Targets...), in.Scope.Refs...) {
		ok = ok && a.inScopeLocked(r, scopes)
	}
	// Resolve owners from engine identity, never caller-supplied application names.
	in.Applications = nil
	seenApps := map[dw.Ref]bool{}
	for _, ref := range in.Targets {
		var app dw.Ref
		if object := a.w.objects[ref]; object != nil {
			app = object.object.App
			if object.object.Kind == dw.KindApplication {
				app = ref
			}
		}
		// Preserve an unresolved owner as an empty Ref so an authorizer cannot
		// accidentally approve a mixed known/unknown target set.
		if !seenApps[app] {
			in.Applications = append(in.Applications, app)
			seenApps[app] = true
		}
	}
	a.w.mu.Unlock()
	if !ok {
		return fault("permission_denied")
	}
	in.Actor = a.ID()
	if auth := a.config.Authorizer; auth != nil {
		d, e := auth.Check(ctx, copyOf(in))
		if e != nil || !d.Allow {
			a.w.mu.Lock()
			a.invalidateLocked()
			a.w.mu.Unlock()
			return fault("permission_denied")
		}
	}
	return ctx.Err()
}
func defaults(r dw.ObserveRequest) dw.ObserveRequest {
	r = copyOf(r)
	if r.Projection == "" {
		r.Projection = dw.ProjectionSummary
	}
	b := &r.Budget
	if b.MaxResults == 0 {
		b.MaxResults = 64
		if r.Projection == dw.ProjectionSummary {
			b.MaxResults = 32
		}
	}
	if b.MaxDepth == 0 {
		b.MaxDepth = 3
	}
	if b.MaxVisitedNodes == 0 {
		b.MaxVisitedNodes = 512
	}
	if b.MaxOutputBytes == 0 {
		b.MaxOutputBytes = 16384
	}
	if b.MaxTextRunes == 0 {
		b.MaxTextRunes = 192
	}
	if b.ReadDeadline == 0 {
		b.ReadDeadline = 2 * time.Second
		if r.Scope.Desktop {
			b.ReadDeadline = 5 * time.Second
		}
	}
	if len(r.Fields) == 0 {
		r.Fields = []string{"role", "name", "states", "bounds", "capabilities", "app", "window", "parent", "lifecycle"}
		if r.Projection == dw.ProjectionSummary {
			r.Fields = []string{"role", "name", "app", "window"}
		}
	}
	if r.Freshness.Mode == "" {
		r.Freshness.Mode = "refresh"
	}
	return r
}
func (a *actor) query(ctx context.Context, r dw.ObserveRequest, resume string, noContinuation bool) ([]dw.Object, dw.Coverage, string, error) {
	w := a.w
	cov := dw.Coverage{Scope: r.Scope, Fields: r.Fields, MaxDepth: r.Budget.MaxDepth, SampleStart: time.Now().UTC()}
	q := backend.Query{Fields: readFields(r), Desktop: r.Scope.Desktop, Depth: r.Budget.MaxDepth, MaxNodes: r.Budget.MaxVisitedNodes, Summary: r.Projection == dw.ProjectionSummary, Detail: r.Projection == dw.ProjectionDetail, Resume: resume, NoContinuation: noContinuation}
	w.mu.Lock()
	for _, ref := range r.Scope.Refs {
		rec := w.objects[ref]
		if rec == nil {
			w.mu.Unlock()
			return nil, cov, "", fault("ref_expired")
		}
		if rec.object.Lifecycle == dw.LifeGone || rec.object.Lifecycle == dw.LifeExpired {
			w.mu.Unlock()
			return nil, cov, "", fault("ref_" + string(rec.object.Lifecycle))
		}
		q.Roots = append(q.Roots, rec.key)
	}
	w.mu.Unlock()
	v, e := w.call(ctx, func() (any, error) {
		p, e := w.driver.Query(ctx, q)
		if e != nil {
			return nil, e
		}
		w.mu.Lock()
		defer w.mu.Unlock()
		refs := make([]dw.Ref, 0, len(p.Nodes))
		for _, n := range p.Nodes {
			r := w.commitLocked(n)
			if r == "" {
				p.Complete = false
				p.Unavailable = append(p.Unavailable, "registry_limit")
				break
			}
			refs = append(refs, r)
		}
		w.commitSeatLocked(p.Seat)
		objects := make([]dw.Object, 0, len(refs))
		for _, ref := range refs {
			objects = append(objects, copyOf(w.objects[ref].object))
		}
		return struct {
			objects []dw.Object
			p       backend.Page
		}{objects, p}, nil
	})
	if e != nil {
		cov.UnavailableSources = []string{asFault(e).Code}
		cov.Dirty = true
		cov.SampleEnd = time.Now().UTC()
		return nil, cov, "", e
	}
	result := v.(struct {
		objects []dw.Object
		p       backend.Page
	})
	cov.Complete = result.p.Complete
	cov.Dirty = result.p.Dirty
	cov.VisitedNodes = result.p.Visited
	cov.UnavailableSources = result.p.Unavailable
	cov.SampleEnd = time.Now().UTC()
	return result.objects, cov, result.p.ScanCursor, nil
}
func (a *actor) cached(r dw.ObserveRequest) ([]dw.Object, dw.Coverage) {
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	out := []dw.Object{}
	for _, rec := range a.w.objects {
		if a.objectInQueryLocked(rec.object, r) {
			out = append(out, copyOf(rec.object))
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Ref < out[j].Ref })
	return out, dw.Coverage{Scope: r.Scope, Fields: r.Fields, MaxDepth: r.Budget.MaxDepth, Complete: false, Dirty: true}
}
func (a *actor) objectInQueryLocked(o dw.Object, r dw.ObserveRequest) bool {
	if o.Lifecycle == dw.LifeGone || o.Lifecycle == dw.LifeExpired {
		return false
	}
	if !a.inScopeLocked(o.Ref, []dw.Scope{r.Scope}) {
		return false
	}
	if r.Projection == dw.ProjectionSummary && o.Kind == dw.KindUI {
		return false
	}
	if r.Projection == dw.ProjectionDetail && !r.Scope.Desktop {
		for _, ref := range r.Scope.Refs {
			if ref == o.Ref {
				return true
			}
		}
		return false
	}
	return true
}
func match(o dw.Object, l *dw.Locator) bool {
	if l == nil {
		return true
	}
	if l.Kind != "" && l.Kind != o.Kind || l.Role != "" && l.Role != o.Role {
		return false
	}
	if l.NameEquals != nil && (o.Name.Value == nil || o.Name.Status != dw.FactKnown || *o.Name.Value != *l.NameEquals) {
		return false
	}
	if l.NameContains != nil && (o.Name.Value == nil || o.Name.Status != dw.FactKnown || !strings.Contains(*o.Name.Value, *l.NameContains)) {
		return false
	}
	for k, v := range l.RequiredStates {
		f := o.States[k]
		if f.Status != dw.FactKnown || f.Value == nil || *f.Value != v {
			return false
		}
	}
	if l.RequiredCapability != "" {
		for _, c := range o.Capabilities {
			if c.Name == l.RequiredCapability && c.Support == "supported" {
				return true
			}
		}
		return false
	}
	return true
}
func project(o dw.Object, r dw.ObserveRequest) dw.Object {
	out := dw.Object{Ref: o.Ref, Kind: o.Kind, Lifecycle: o.Lifecycle, Version: o.Version, GeometryVersion: o.GeometryVersion, SampleStart: o.SampleStart, SampleEnd: o.SampleEnd}
	for _, f := range r.Fields {
		switch f {
		case "role":
			out.Role = o.Role
		case "name":
			out.Name = o.Name
		case "value_preview":
			out.ValuePreview = o.ValuePreview
		case "uri":
			out.URI = o.URI
		case "states":
			out.States = o.States
		case "bounds":
			out.Bounds = o.Bounds
		case "capabilities":
			out.Capabilities = o.Capabilities
		case "app":
			out.App = o.App
		case "window":
			out.Window = o.Window
		case "parent":
			out.Parent = o.Parent
		case "relations":
			out.Relations = o.Relations
		}
	}
	for _, f := range []*dw.Fact[string]{&out.Name, &out.ValuePreview} {
		if f.Value != nil {
			v := []rune(*f.Value)
			if len(v) > r.Budget.MaxTextRunes {
				v = v[:r.Budget.MaxTextRunes]
			}
			s := string(v)
			f.Value = &s
		}
	}
	return out
}
func (a *actor) selectObjects(all []dw.Object, r dw.ObserveRequest) []dw.Object {
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	return a.selectObjectsLocked(all, r)
}
func (a *actor) selectObjectsLocked(all []dw.Object, r dw.ObserveRequest) []dw.Object {
	out := []dw.Object{}
	seen := map[dw.Ref]bool{}
	for _, o := range all {
		if rec := a.w.objects[o.Ref]; rec != nil {
			o = copyOf(rec.object)
		}
		if seen[o.Ref] || !a.objectInQueryLocked(o, r) || !a.inScopeLocked(o.Ref, a.config.ReadScopes) || !match(o, r.Match) {
			continue
		}
		if r.Match != nil && !a.inScopeLocked(o.Ref, []dw.Scope{{Refs: []dw.Ref{r.Match.Within}}}) {
			continue
		}
		seen[o.Ref] = true
		p := project(o, r)
		for _, ref := range []*dw.Ref{&p.App, &p.Window, &p.Parent} {
			if *ref != "" && !a.inScopeLocked(*ref, a.config.ReadScopes) {
				*ref = ""
			}
		}
		relations := p.Relations
		p.Relations = nil
		for _, rel := range relations {
			if a.inScopeLocked(rel.Target, a.config.ReadScopes) {
				p.Relations = append(p.Relations, rel)
			}
		}
		out = append(out, p)
	}
	// Put explicitly requested roots first, then retain provider traversal order.
	// Lexicographic sorting of opaque Refs used to hide the root on later pages.
	roots := map[dw.Ref]bool{}
	for _, ref := range r.Scope.Refs {
		roots[ref] = true
	}
	sort.SliceStable(out, func(i, j int) bool { return roots[out[i].Ref] && !roots[out[j].Ref] })
	return out
}
func (a *actor) seatLocked() dw.SeatState {
	s := copyOf(a.w.seat)
	for _, f := range []*dw.Fact[dw.Ref]{&s.ForegroundApplication, &s.ForegroundWindow, &s.FocusedObject} {
		if f.Value != nil && !a.inScopeLocked(*f.Value, a.config.ReadScopes) {
			*f = dw.Fact[dw.Ref]{Status: dw.FactRedacted}
		}
	}
	desktop := false
	for _, scope := range a.config.ReadScopes {
		desktop = desktop || scope.Desktop
	}
	if !desktop {
		s.Pointer = dw.Fact[dw.Point]{Status: dw.FactRedacted}
	}
	if a.w.seatGate.health() == "fenced" {
		s.Health = "fenced"
	}
	return s
}
func (a *actor) Observe(ctx context.Context, req dw.ObserveRequest) (dw.Observation, error) {
	if e := req.Validate(); e != nil {
		return dw.Observation{}, e
	}
	r := defaults(req)
	ctx, cancel := context.WithTimeout(ctx, r.Budget.ReadDeadline)
	defer cancel()
	in := dw.Intent{Operation: "observe", Scope: r.Scope, Fields: r.Fields}
	if e := a.check(ctx, in, false); e != nil {
		return dw.Observation{}, e
	}
	_, e := a.w.Environment(ctx)
	if e != nil {
		return dw.Observation{}, e
	}
	if r.Continuation != "" {
		a.w.mu.Lock()
		p := a.pages[r.Continuation]
		a.w.mu.Unlock()
		if p == nil || time.Now().After(p.expires) {
			return dw.Observation{}, fault("continuation_expired")
		}
		want, got := p.request, r
		want.Continuation = ""
		got.Continuation = ""
		// The managed helper reserves envelope bytes from each wire response.
		// Its request ID grows over time, so an otherwise identical continuation
		// can have a slightly smaller presentation budget.
		want.Budget.MaxOutputBytes = 0
		got.Budget.MaxOutputBytes = 0
		if !reflect.DeepEqual(want, got) {
			return dw.Observation{}, dw.Invalid("continuation query mismatch")
		}
		if p.offset == len(p.objects) && p.scanCursor != "" {
			a.w.mu.Lock()
			permissionVersion := a.w.permissionVersion
			a.w.mu.Unlock()
			if p.permissionVersion != permissionVersion {
				return dw.Observation{}, fault("permission_changed")
			}
			all, cov, scanCursor, e := a.query(ctx, r, p.scanCursor, false)
			if e != nil {
				return dw.Observation{Epoch: a.w.epoch, Coverage: cov}, e
			}
			if e = a.check(ctx, in, false); e != nil {
				return dw.Observation{}, e
			}
			a.w.mu.Lock()
			objects := a.selectObjectsLocked(all, r)
			nextRequest := p.request
			nextRequest.Budget.MaxOutputBytes = r.Budget.MaxOutputBytes
			next := &page{permissionVersion: a.w.permissionVersion, request: nextRequest, objects: objects, coverage: cov, scanCursor: scanCursor, outputBytes: p.outputBytes, seat: a.seatLocked(), rev: a.w.revision, env: copyOf(a.w.env), expires: p.expires}
			a.w.mu.Unlock()
			return a.render(ctx, next, in)
		}
		effective := *p
		effective.request.Budget.MaxOutputBytes = r.Budget.MaxOutputBytes
		return a.render(ctx, &effective, in)
	}
	var all []dw.Object
	var cov dw.Coverage
	var scanCursor string
	if r.Freshness.Mode == "cached" {
		all, cov = a.cached(r)
	} else {
		all, cov, scanCursor, e = a.query(ctx, r, "", false)
		if e != nil {
			return dw.Observation{Epoch: a.w.epoch, Coverage: cov}, e
		}
	}
	if e = a.check(ctx, in, false); e != nil {
		return dw.Observation{}, e
	}
	a.w.mu.Lock()
	objects := a.selectObjectsLocked(all, r)
	p := &page{permissionVersion: a.w.permissionVersion, request: r, objects: objects, coverage: cov, scanCursor: scanCursor, seat: a.seatLocked(), rev: a.w.revision, env: copyOf(a.w.env), expires: time.Now().Add(a.w.opts.HistoryTTL)}
	a.w.mu.Unlock()
	return a.render(ctx, p, in)
}
func (a *actor) render(ctx context.Context, p *page, in dw.Intent) (dw.Observation, error) {
	if e := a.check(ctx, in, false); e != nil {
		return dw.Observation{}, e
	}
	r := p.request
	out := dw.Observation{Epoch: a.w.epoch, Revision: p.rev, Cursor: dw.Cursor(token("c-")), Seat: p.seat, Coverage: p.coverage}
	if r.Scope.Desktop {
		out.Environment = &p.env
	}
	end := p.offset + r.Budget.MaxResults
	if end > len(p.objects) {
		end = len(p.objects)
	}
	next := token("p-")
	// An outline can span many presentation pages even if the native driver
	// finishes its scan in one call (or does not support scan cursors). Bound
	// the whole model-facing page series on every platform.
	const scanOutputLimit = 24 * 1024
	outputCapped := r.Projection == dw.ProjectionOutline
	outputLimit := false
	for {
		out.Objects = copyOf(p.objects[p.offset:end])
		out.Coverage.Truncated = end < len(p.objects) || !p.coverage.Complete
		out.Coverage.Complete = p.coverage.Complete && end == len(p.objects)
		out.Coverage.Continuation = ""
		if end < len(p.objects) || p.scanCursor != "" {
			out.Coverage.Continuation = next
		}
		size := wire.Size(out.Epoch, out)
		if size <= r.Budget.MaxOutputBytes && (!outputCapped || p.outputBytes+size <= scanOutputLimit) {
			break
		}
		if end == p.offset {
			if !outputCapped || p.outputBytes == 0 {
				return dw.Observation{}, fault("budget_too_small")
			}
			outputLimit = true
			break
		}
		end--
	}
	// An empty page can still fit the remaining total budget when the next
	// object does not. Report the cumulative limit instead of a small-page
	// fault or an empty continuation loop.
	if !outputLimit && end == p.offset && end < len(p.objects) {
		if !outputCapped || p.outputBytes == 0 {
			return dw.Observation{}, fault("budget_too_small")
		}
		outputLimit = true
	}
	if outputLimit {
		out.Objects = nil
		out.Coverage.Complete = false
		out.Coverage.Truncated = true
		out.Coverage.Continuation = ""
		out.Coverage.UnavailableSources = append(append([]string{}, out.Coverage.UnavailableSources...), "ax_output_limit")
		if wire.Size(out.Epoch, out) > r.Budget.MaxOutputBytes {
			return dw.Observation{}, fault("budget_too_small")
		}
	}
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	if a.closed || a.w.closing {
		return dw.Observation{}, fault("actor_closed")
	}
	if p.permissionVersion != a.w.permissionVersion {
		return dw.Observation{}, fault("permission_changed")
	}
	a.pruneLocked()
	if len(a.views) >= a.w.opts.ViewLimit || len(a.pages) >= a.w.opts.ViewLimit {
		return dw.Observation{}, fault("resource_exhausted")
	}
	limit := end - p.offset
	if end == len(p.objects) {
		limit = r.Budget.MaxResults
	}
	a.views[out.Cursor] = &view{topology: p.env.Topology, request: r, objects: copyOf(out.Objects), seat: out.Seat, coverage: out.Coverage, rev: out.Revision, at: time.Now(), permissionVersion: a.w.permissionVersion, offset: p.offset, limit: limit}
	if !outputLimit && (end < len(p.objects) || p.scanCursor != "") {
		np := *p
		np.offset = end
		if outputCapped {
			np.outputBytes += wire.Size(out.Epoch, out)
		}
		a.pages[next] = &np
	}
	return out, nil
}
func (a *actor) pruneLocked() {
	now := time.Now()
	for k, v := range a.views {
		if now.Sub(v.at) > a.w.opts.HistoryTTL {
			delete(a.views, k)
		}
	}
	for k, p := range a.pages {
		if now.After(p.expires) {
			delete(a.pages, k)
		}
	}
	for k, t := range a.texts {
		if now.After(t.expires) {
			delete(a.texts, k)
		}
	}
	for k, t := range a.assets {
		if now.After(t.asset.ExpiresAt) {
			delete(a.assets, k)
		}
	}
}
func (a *actor) Changes(ctx context.Context, r dw.ChangeRequest) (dw.ChangeSet, error) {
	if r.Wait < 0 || r.Wait > 30*time.Second || r.MaxOutputBytes < 0 || r.MaxOutputBytes > 1<<20 {
		return dw.ChangeSet{}, dw.Invalid("invalid sync budget")
	}
	if r.MaxOutputBytes == 0 {
		r.MaxOutputBytes = 16384
	}
	deadline := time.Now().Add(r.Wait)
	for {
		c, e := a.changes(ctx, r)
		if e != nil || c.ResetRequired || c.To != c.From || r.Wait == 0 || !time.Now().Before(deadline) {
			return c, e
		}
		timer := time.NewTimer(a.w.opts.PollInterval)
		select {
		case <-ctx.Done():
			timer.Stop()
			return dw.ChangeSet{}, ctx.Err()
		case <-timer.C:
		}
	}
}
func (a *actor) changes(ctx context.Context, r dw.ChangeRequest) (dw.ChangeSet, error) {
	a.w.mu.Lock()
	v := a.views[r.Cursor]
	rev := a.w.revision
	perm := a.w.permissionVersion
	a.w.mu.Unlock()
	reset := func(reason string) (dw.ChangeSet, error) {
		return dw.ChangeSet{ResetRequired: true, ResetReason: reason}, nil
	}
	if v == nil {
		return reset("cursor_expired")
	}
	in := dw.Intent{Operation: "sync", Scope: v.request.Scope, Fields: v.request.Fields}
	if e := a.check(ctx, in, false); e != nil {
		return dw.ChangeSet{}, e
	}
	if time.Since(v.at) > a.w.opts.HistoryTTL || rev-v.rev > dw.Revision(a.w.opts.HistoryLimit) {
		return reset("history_expired")
	}
	if perm != v.permissionVersion {
		return reset("permission_changed")
	}
	rctx, cancel := context.WithTimeout(ctx, v.request.Budget.ReadDeadline)
	defer cancel()
	if _, e := a.w.Environment(rctx); e != nil {
		return dw.ChangeSet{}, e
	}
	all, cov, _, e := a.query(rctx, v.request, "", true)
	if e != nil {
		return reset("provider_unavailable")
	}
	if e := a.check(ctx, in, false); e != nil {
		return dw.ChangeSet{}, e
	}
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	all = a.selectObjectsLocked(all, v.request)
	start := v.offset
	if start > len(all) {
		start = len(all)
	}
	end := start + v.limit
	if end > len(all) {
		end = len(all)
	}
	now := all[start:end]
	if a.w.permissionVersion != v.permissionVersion {
		return reset("permission_changed")
	}
	if a.w.env.Topology != v.topology {
		return reset("topology_changed")
	}
	out := dw.ChangeSet{From: v.rev, To: a.w.revision, Cursor: r.Cursor, Coverage: cov}
	old := map[dw.Ref]dw.Object{}
	for _, o := range v.objects {
		old[o.Ref] = o
	}
	for _, o := range now {
		prev, ok := old[o.Ref]
		if !ok || !reflect.DeepEqual(material(o), material(prev)) || o.Version != prev.Version {
			out.Upserts = append(out.Upserts, o)
		}
		delete(old, o.Ref)
	}
	for ref := range old {
		reason := "out_of_view"
		if rec := a.w.objects[ref]; rec != nil {
			switch rec.object.Lifecycle {
			case dw.LifeGone:
				reason = "destroyed"
			case dw.LifeExpired:
				reason = "expired"
			}
		}
		out.Removed = append(out.Removed, dw.Removed{Ref: ref, Reason: reason})
	}
	sort.Slice(out.Removed, func(i, j int) bool { return out.Removed[i].Ref < out.Removed[j].Ref })
	s := a.seatLocked()
	if !reflect.DeepEqual(seatMaterial(s), seatMaterial(v.seat)) {
		out.Seat = &s
	}
	if !cov.Complete {
		// Absence from a partial traversal is not evidence of removal.
		out.Removed = nil
		out.InvalidatedScopes = []dw.Scope{v.request.Scope}
		out.ResetRequired = true
		out.ResetReason = "coverage_incomplete"
		out.Cursor = ""
		return out, nil
	}
	out.Cursor = dw.Cursor(token("c-"))
	if wire.Size(a.w.epoch, out) > r.MaxOutputBytes {
		return reset("delta_budget_exceeded")
	}
	a.pruneLocked()
	if len(a.views) >= a.w.opts.ViewLimit {
		delete(a.views, r.Cursor)
	}
	nv := *v
	nv.objects = copyOf(now)
	nv.seat = s
	nv.coverage = cov
	nv.rev = out.To
	nv.at = time.Now()
	a.views[out.Cursor] = &nv
	return out, nil
}
func seatMaterial(s dw.SeatState) dw.SeatState {
	s.ForegroundWindow.SampledAt = time.Time{}
	s.FocusedObject.SampledAt = time.Time{}
	s.Pointer.SampledAt = time.Time{}
	return s
}

type stream struct {
	actor  *actor
	cursor dw.Cursor
	ctx    context.Context
	cancel context.CancelFunc
}

func (a *actor) Watch(ctx context.Context, r dw.WatchRequest) (dw.Stream, error) {
	if r.Buffer < 0 || r.Buffer > 4096 {
		return nil, dw.Invalid("invalid watch buffer")
	}
	a.w.mu.Lock()
	_, ok := a.views[r.Cursor]
	a.w.mu.Unlock()
	if !ok {
		return nil, fault("reset_required")
	}
	c, cancel := context.WithCancel(ctx)
	return &stream{a, r.Cursor, c, cancel}, nil
}
func (s *stream) Next(ctx context.Context) (dw.ChangeSet, error) {
	c, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(s.ctx, cancel)
	defer stop()
	defer cancel()
	out, e := s.actor.Changes(c, dw.ChangeRequest{Cursor: s.cursor, Wait: 30 * time.Second})
	if e == nil && !out.ResetRequired {
		s.cursor = out.Cursor
	}
	return out, e
}
func (s *stream) Close() error { s.cancel(); return nil }

func readFields(r dw.ObserveRequest) []string {
	fields := append([]string{}, r.Fields...)
	fields = append(fields, "role") // provider kind and identity must always be known.
	if m := r.Match; m != nil {
		if m.NameEquals != nil || m.NameContains != nil {
			fields = append(fields, "name")
		}
		if len(m.RequiredStates) > 0 {
			fields = append(fields, "states")
		}
		if m.RequiredCapability != "" {
			fields = append(fields, "capabilities")
		}
	}
	sort.Strings(fields)
	out := fields[:0]
	for _, f := range fields {
		if len(out) == 0 || out[len(out)-1] != f {
			out = append(out, f)
		}
	}
	return out
}
