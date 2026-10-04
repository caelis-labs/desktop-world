package engine

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"reflect"
	"runtime"
	"sync"
	"time"
)

type Options struct {
	HistoryLimit, RequestLimit, ObjectLimit, ViewLimit int
	HistoryTTL, ReceiptTTL, AssetTTL, PollInterval     time.Duration
	VerificationPollInterval                           time.Duration
	SeatID                                             string
}
type record struct {
	key    backend.Key
	object dw.Object
}
type World struct {
	mu                sync.Mutex
	driver            backend.Driver
	opts              Options
	epoch             dw.Epoch
	revision          dw.Revision
	env               dw.Environment
	seat              dw.SeatState
	objects           map[dw.Ref]*record
	keys              map[backend.Key]dw.Ref
	actors            map[dw.ActorID]*actor
	runs              map[dw.RequestID]*run
	runIDs            map[dw.RunID]*run
	jobs              chan job
	stop              chan struct{}
	done              chan struct{}
	closing           bool
	closeOnce         sync.Once
	seatGate          *seatGate
	next              uint64
	permissionVersion uint64
}
type job struct {
	ctx   context.Context
	fn    func() (any, error)
	reply chan answer
	start chan struct{}
}
type answer struct {
	value any
	err   error
}
type seatGate struct {
	token  chan struct{}
	mu     sync.Mutex
	fenced bool
}

var seatsMu sync.Mutex
var seats = map[string]*seatGate{}

func gate(id string) *seatGate {
	seatsMu.Lock()
	defer seatsMu.Unlock()
	s := seats[id]
	if s == nil {
		s = &seatGate{token: make(chan struct{}, 1)}
		s.token <- struct{}{}
		seats[id] = s
	}
	return s
}
func (s *seatGate) health() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fenced {
		return "fenced"
	}
	return "ready"
}
func (s *seatGate) fence(v bool) { s.mu.Lock(); s.fenced = v; s.mu.Unlock() }
func token(prefix string) string {
	var b [12]byte
	if _, e := rand.Read(b[:]); e != nil {
		panic(e)
	}
	return prefix + hex.EncodeToString(b[:])
}
func fault(code string) *dw.Fault {
	if code == "seat_fenced" {
		return dw.NewFault(code, "Native delivery remains unresolved; input is disabled. Inspect the original receipt. Observation/cancel cannot reset the fence. If the receipt remains terminal unknown, stop and let the host reconcile effects; do not restart or replay automatically.", "never_automatically")
	}
	return dw.NewFault(code, code, "reobserve")
}
func asFault(e error) *dw.Fault {
	if e == nil {
		return nil
	}
	var f *dw.Fault
	if errors.As(e, &f) {
		return f
	}
	if errors.Is(e, context.DeadlineExceeded) {
		return fault("deadline_exceeded")
	}
	if errors.Is(e, context.Canceled) {
		return fault("cancelled")
	}
	return fault("provider_unavailable")
}
func copyOf[T any](v T) T {
	b, _ := json.Marshal(v)
	var out T
	_ = json.Unmarshal(b, &out)
	return out
}
func Open(ctx context.Context, d backend.Driver, o Options) (dw.World, error) {
	if o.HistoryLimit <= 0 {
		o.HistoryLimit = 4096
	}
	if o.RequestLimit <= 0 {
		o.RequestLimit = 65536
	}
	if o.ObjectLimit <= 0 {
		o.ObjectLimit = 10000
	}
	if o.ViewLimit <= 0 {
		o.ViewLimit = 128
	}
	if o.HistoryTTL <= 0 {
		o.HistoryTTL = time.Minute
	}
	if o.ReceiptTTL <= 0 {
		o.ReceiptTTL = 5 * time.Minute
	}
	if o.AssetTTL <= 0 {
		o.AssetTTL = time.Minute
	}
	if o.PollInterval <= 0 {
		o.PollInterval = 500 * time.Millisecond
	}
	if o.VerificationPollInterval <= 0 {
		o.VerificationPollInterval = 20 * time.Millisecond
	}
	if o.SeatID == "" {
		o.SeatID = "native-desktop"
	}
	w := &World{driver: d, opts: o, epoch: dw.Epoch(token("e-")), objects: map[dw.Ref]*record{}, keys: map[backend.Key]dw.Ref{}, actors: map[dw.ActorID]*actor{}, runs: map[dw.RequestID]*run{}, runIDs: map[dw.RunID]*run{}, jobs: make(chan job), stop: make(chan struct{}), done: make(chan struct{}), seatGate: gate(o.SeatID)}
	go w.worker()
	_, err := w.call(ctx, func() (any, error) {
		if e := d.Open(ctx); e != nil {
			return nil, e
		}
		env, e := d.Environment(ctx)
		if e == nil {
			w.mu.Lock()
			w.updateEnvironment(env)
			w.mu.Unlock()
		}
		return nil, e
	})
	if err != nil {
		_ = w.Close(ctx)
		return nil, err
	}
	return w, nil
}
func (w *World) worker() {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	defer close(w.done)
	for {
		var j job
		select {
		case <-w.stop:
			_ = w.driver.Close(context.Background())
			return
		case j = <-w.jobs:
		}
		if e := j.ctx.Err(); e != nil {
			j.reply <- answer{err: e}
			continue
		}
		close(j.start)
		v, e := j.fn()
		j.reply <- answer{v, e}
	}
}
func (w *World) submit(ctx context.Context, fn func() (any, error)) (job, error) {
	j := job{ctx: ctx, fn: fn, reply: make(chan answer, 1), start: make(chan struct{})}
	w.mu.Lock()
	closed := w.closing
	w.mu.Unlock()
	if closed {
		return j, fault("world_closed")
	}
	select {
	case w.jobs <- j:
		return j, nil
	case <-ctx.Done():
		return j, ctx.Err()
	case <-w.done:
		return j, fault("world_closed")
	}
}
func (w *World) call(ctx context.Context, fn func() (any, error)) (any, error) {
	j, e := w.submit(ctx, fn)
	if e != nil {
		return nil, e
	}
	select {
	case a := <-j.reply:
		return a.value, a.err
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}
func (w *World) updateEnvironment(e dw.Environment) {
	e.Epoch = w.epoch
	if !reflect.DeepEqual(w.env, e) {
		if !reflect.DeepEqual(w.env.Permissions, e.Permissions) && w.env.Epoch != "" {
			w.permissionVersion++
			for _, a := range w.actors {
				a.invalidateLocked()
			}
			for _, r := range w.objects {
				r.object.ValuePreview = dw.Fact[string]{Status: dw.FactRedacted}
				r.object.URI = dw.Fact[string]{Status: dw.FactRedacted}
				r.object.Name = dw.Fact[string]{Status: dw.FactRedacted}
			}
		}
		w.env = copyOf(e)
		w.revision++
	}
}
func (w *World) Environment(ctx context.Context) (dw.Environment, error) {
	v, e := w.call(ctx, func() (any, error) {
		env, e := w.driver.Environment(ctx)
		if e != nil {
			return nil, e
		}
		w.mu.Lock()
		defer w.mu.Unlock()
		w.updateEnvironment(env)
		return copyOf(w.env), nil
	})
	if e != nil {
		return dw.Environment{}, e
	}
	return v.(dw.Environment), nil
}
func (w *World) RequestPermissions(ctx context.Context, r dw.PermissionRequest) ([]dw.Permission, error) {
	if len(r.Names) == 0 {
		e, err := w.Environment(ctx)
		return e.Permissions, err
	}
	seen := map[string]bool{}
	for _, name := range r.Names {
		switch name {
		case "accessibility", "input", "screen_capture", "user_input_observation":
		default:
			return nil, dw.Invalid("unknown permission")
		}
		if seen[name] {
			return nil, dw.Invalid("duplicate permission")
		}
		seen[name] = true
	}
	v, e := w.call(ctx, func() (any, error) { return w.driver.Permissions(ctx, r) })
	if e != nil {
		return nil, e
	}
	_, e = w.Environment(ctx)
	return v.([]dw.Permission), e
}
func (w *World) NewActor(ctx context.Context, c dw.ActorConfig) (dw.Actor, error) {
	if e := ctx.Err(); e != nil {
		return nil, e
	}
	if e := c.InputPolicy.Validate(); e != nil {
		return nil, e
	}
	if c.ID == "" {
		return nil, dw.Invalid("actor id required")
	}
	for _, s := range append(append([]dw.Scope{}, c.ReadScopes...), c.WriteScopes...) {
		if e := s.Validate(); e != nil {
			return nil, e
		}
	}
	for _, op := range c.Operations {
		if !dw.ValidOperation(op) {
			return nil, dw.Invalid("unknown actor operation")
		}
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.closing {
		return nil, fault("world_closed")
	}
	if _, ok := w.actors[c.ID]; ok {
		return nil, dw.Invalid("duplicate actor id")
	}
	authorizer := c.Authorizer
	c.Authorizer = nil
	c = copyOf(c)
	c.Authorizer = authorizer
	a := &actor{w: w, config: c, views: map[dw.Cursor]*view{}, pages: map[string]*page{}, texts: map[string]textPage{}, assets: map[dw.AssetID]assetRecord{}}
	w.actors[c.ID] = a
	return a, nil
}
func (w *World) Close(ctx context.Context) error {
	w.closeOnce.Do(func() {
		w.mu.Lock()
		w.closing = true
		for _, r := range w.runs {
			r.cancel()
		}
		for _, a := range w.actors {
			a.closed = true
			a.invalidateLocked()
		}
		w.mu.Unlock()
		go func() { // Wait for admitted runs and native calls before closing the worker.
			w.mu.Lock()
			rs := make([]*run, 0, len(w.runs))
			for _, r := range w.runs {
				rs = append(rs, r)
			}
			w.mu.Unlock()
			for _, r := range rs {
				<-r.done
			}
			close(w.stop)
		}()
	})
	select {
	case <-w.done:
		return nil
	case <-ctx.Done():
		return fault("close_incomplete")
	}
}
func (w *World) refLocked(k backend.Key) dw.Ref {
	if k == "" {
		return ""
	}
	if r := w.keys[k]; r != "" {
		return r
	}
	if len(w.keys) >= w.opts.ObjectLimit {
		return ""
	}
	w.next++
	r := dw.Ref(fmt.Sprintf("r-%s-%x", string(w.epoch)[2:], w.next))
	w.keys[k] = r
	w.objects[r] = &record{key: k, object: dw.Object{Ref: r, Lifecycle: dw.LifeStale}}
	return r
}
func material(o dw.Object) dw.Object {
	o.SampleStart = time.Time{}
	o.SampleEnd = time.Time{}
	o.Version = 0
	o.GeometryVersion = 0
	o.Name.SampledAt = time.Time{}
	o.ValuePreview.SampledAt = time.Time{}
	o.URI.SampledAt = time.Time{}
	o.Bounds.SampledAt = time.Time{}
	states := map[string]dw.Fact[bool]{}
	for k, v := range o.States {
		v.SampledAt = time.Time{}
		states[k] = v
	}
	o.States = states
	return o
}
func (w *World) commitLocked(n backend.Node) dw.Ref {
	r := w.refLocked(n.Key)
	if r == "" {
		return ""
	}
	old := w.objects[r].object
	if old.Lifecycle == dw.LifeGone || old.Lifecycle == dw.LifeExpired {
		return r
	}
	o := mergeFields(old, n)
	o.Ref = r
	o.App = w.refLocked(n.App)
	o.Window = w.refLocked(n.Window)
	o.Parent = w.refLocked(n.Parent)
	if o.Lifecycle == "" {
		o.Lifecycle = dw.LifeLive
	}
	if protectedFact(o.States["protected"]) || protectedFact(n.Object.States["protected"]) {
		o.ValuePreview = dw.Fact[string]{Status: dw.FactRedacted}
		o.URI = dw.Fact[string]{Status: dw.FactRedacted}
	}
	o.Version = old.Version
	o.GeometryVersion = old.GeometryVersion
	if !reflect.DeepEqual(material(o), material(old)) {
		o.Version++
		w.revision++
	}
	if !reflect.DeepEqual(material(dw.Object{Bounds: o.Bounds}), material(dw.Object{Bounds: old.Bounds})) {
		o.GeometryVersion++
	}
	if o.SampleEnd.IsZero() {
		o.SampleEnd = time.Now().UTC()
		o.SampleStart = o.SampleEnd
	}
	w.objects[r].object = o
	return r
}
func (w *World) commitSeatLocked(s backend.Seat) {
	for _, n := range s.Nodes {
		w.commitLocked(n)
	}
	next := dw.SeatState{ForegroundApplication: dw.Unknown[dw.Ref](), ForegroundWindow: dw.Unknown[dw.Ref](), FocusedObject: dw.Unknown[dw.Ref](), Pointer: s.Pointer, Health: s.Health, InterventionDetection: s.Intervention}
	if s.Application != "" {
		next.ForegroundApplication = dw.Known(w.refLocked(s.Application))
	}
	if s.Foreground != "" {
		next.ForegroundWindow = dw.Known(w.refLocked(s.Foreground))
	}
	if s.Focused != "" {
		next.FocusedObject = dw.Known(w.refLocked(s.Focused))
	}
	a, b := copyOf(next), copyOf(w.seat)
	a.ForegroundApplication.SampledAt = time.Time{}
	b.ForegroundApplication.SampledAt = time.Time{}
	a.ForegroundWindow.SampledAt = time.Time{}
	a.FocusedObject.SampledAt = time.Time{}
	a.Pointer.SampledAt = time.Time{}
	b.ForegroundWindow.SampledAt = time.Time{}
	b.FocusedObject.SampledAt = time.Time{}
	b.Pointer.SampledAt = time.Time{}
	if !reflect.DeepEqual(a, b) {
		w.revision++
	}
	w.seat = next
}
func (w *World) read(ctx context.Context, r dw.Ref) (dw.Object, error) {
	w.mu.Lock()
	rec := w.objects[r]
	if rec == nil {
		w.mu.Unlock()
		return dw.Object{}, fault("ref_expired")
	}
	k := rec.key
	l := rec.object.Lifecycle
	w.mu.Unlock()
	if l == dw.LifeGone || l == dw.LifeExpired {
		return dw.Object{}, fault("ref_" + string(l))
	}
	v, e := w.call(ctx, func() (any, error) {
		n, s, e := w.driver.Read(ctx, k)
		w.mu.Lock()
		defer w.mu.Unlock()
		if e != nil {
			f := asFault(e)
			life := dw.LifeUnavailable
			if f.Code == "ref_gone" {
				life = dw.LifeGone
			}
			if rec.object.Lifecycle != life {
				rec.object.Lifecycle = life
				rec.object.Version++
				w.revision++
			}
			return nil, e
		}
		if n.Key != k {
			return nil, fault("ref_stale")
		}
		w.commitLocked(n)
		w.commitSeatLocked(s)
		return copyOf(rec.object), nil
	})
	if e != nil {
		return dw.Object{}, e
	}
	return v.(dw.Object), nil
}

// A partial native refresh cannot erase unrequested fields or refresh their times.
func mergeFields(old dw.Object, n backend.Node) dw.Object {
	next := copyOf(n.Object)
	if len(n.Fields) != 0 {
		next = copyOf(old)
		next.Kind, next.Role, next.Lifecycle = n.Object.Kind, n.Object.Role, n.Object.Lifecycle
		next.SampleStart, next.SampleEnd = n.Object.SampleStart, n.Object.SampleEnd
		for _, f := range n.Fields {
			switch f {
			case "name":
				next.Name = n.Object.Name
			case "value_preview":
				next.ValuePreview = n.Object.ValuePreview
			case "uri":
				next.URI = n.Object.URI
			case "states":
				next.States = copyOf(n.Object.States)
			case "bounds":
				next.Bounds = n.Object.Bounds
			case "capabilities":
				next.Capabilities = copyOf(n.Object.Capabilities)
			case "relations":
				next.Relations = copyOf(n.Object.Relations)
			}
		}
	}
	at := n.Object.SampleEnd
	if at.IsZero() {
		at = time.Now().UTC()
	}
	sampled := func(name string) bool {
		if len(n.Fields) == 0 {
			return true
		}
		for _, f := range n.Fields {
			if f == name {
				return true
			}
		}
		return false
	}
	if sampled("name") && next.Name.Status != "" {
		next.Name.SampledAt = at
	}
	if sampled("value_preview") && next.ValuePreview.Status != "" {
		next.ValuePreview.SampledAt = at
	}
	if sampled("uri") && next.URI.Status != "" {
		next.URI.SampledAt = at
	}
	if sampled("bounds") && next.Bounds.Status != "" {
		next.Bounds.SampledAt = at
	}
	if sampled("states") {
		for k, v := range next.States {
			v.SampledAt = at
			next.States[k] = v
		}
	}
	return next
}

func protectedFact(v dw.Fact[bool]) bool {
	return v.Status == dw.FactKnown && v.Value != nil && *v.Value
}
