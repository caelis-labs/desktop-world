package engine

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"reflect"
	"strings"
	"time"
)

type run struct {
	actor    *actor
	digest   string
	receipt  dw.Receipt
	cancel   context.CancelFunc
	done     chan struct{}
	progress chan dw.Progress
}

func normalizePlan(p dw.Plan) dw.Plan {
	p = copyOf(p)
	if p.Timeout == 0 {
		p.Timeout = 10 * time.Second
	}
	for i := range p.Steps {
		s := &p.Steps[i]
		if len(s.Before) == 0 {
			s.Before = nil
		}
		if len(s.After) == 0 {
			s.After = nil
		}
		if s.Press != nil && len(s.Press.Modifiers) == 0 {
			s.Press.Modifiers = nil
		}
		if s.Bind != nil && len(s.Bind.Locator.RequiredStates) == 0 {
			s.Bind.Locator.RequiredStates = nil
		}
		if s.Timeout == 0 {
			s.Timeout = 2 * time.Second
		}
		if s.Completion == "" {
			s.Completion = "dispatch"
			if s.Op == "focus" || s.Op == "set_value" || s.Op == "wait" || s.Op == "bind" {
				s.Completion = "verify"
			}
		}
	}
	return p
}
func (a *actor) Execute(ctx context.Context, p dw.Plan) (dw.Receipt, error) {
	if e := p.Validate(); e != nil {
		return dw.Receipt{}, e
	}
	if p.Epoch != a.w.epoch {
		return dw.Receipt{}, fault("epoch_mismatch")
	}
	p = normalizePlan(p)
	body := p
	body.RequestID = ""
	b, _ := json.Marshal(body)
	sum := sha256.Sum256(b)
	digest := hex.EncodeToString(sum[:])
	w := a.w
	w.mu.Lock()
	if prev := w.runs[p.RequestID]; prev != nil {
		if prev.actor != a || prev.digest != digest {
			w.mu.Unlock()
			return dw.Receipt{}, fault("request_conflict")
		}
		out, e := a.receiptLocked(prev)
		w.mu.Unlock()
		return out, e
	}
	if a.closed || w.closing {
		w.mu.Unlock()
		return dw.Receipt{}, fault("actor_closed")
	}
	if len(w.runs) >= w.opts.RequestLimit {
		w.mu.Unlock()
		return dw.Receipt{}, fault("resource_exhausted")
	}
	pending := 0
	for _, r := range w.runs {
		if r.receipt.State != "terminal" {
			pending++
		}
	}
	if pending >= 64 {
		w.mu.Unlock()
		return dw.Receipt{}, fault("resource_exhausted")
	}
	c, cancel := context.WithTimeout(ctx, p.Timeout)
	r := &run{actor: a, digest: digest, cancel: cancel, done: make(chan struct{}), progress: make(chan dw.Progress, 32), receipt: dw.Receipt{Epoch: w.epoch, RunID: dw.RunID(token("run-")), RequestID: p.RequestID, State: "queued", Outcome: "pending", Bindings: map[string]dw.Ref{}, StartRevision: w.revision, SeatHealth: w.seatGate.health()}}
	for _, s := range p.Steps {
		r.receipt.Steps = append(r.receipt.Steps, dw.StepResult{ID: s.ID, State: "skipped", Delivery: dw.DeliveryNone, Verification: dw.VerifyNotRequested})
	}
	w.runs[p.RequestID] = r
	w.runIDs[r.receipt.RunID] = r
	w.mu.Unlock()
	go a.execute(c, r, p)
	<-r.done
	w.mu.Lock()
	out, e := a.receiptLocked(r)
	w.mu.Unlock()
	return out, e
}
func (a *actor) receiptLocked(r *run) (dw.Receipt, error) {
	if r.actor != a {
		return dw.Receipt{}, fault("permission_denied")
	}
	if !r.receipt.ExpiresAt.IsZero() && time.Now().After(r.receipt.ExpiresAt) {
		r.receipt = dw.Receipt{RunID: r.receipt.RunID, RequestID: r.receipt.RequestID, Epoch: r.receipt.Epoch, State: "terminal", Outcome: r.receipt.Outcome, ExpiresAt: r.receipt.ExpiresAt}
		return copyOf(r.receipt), fault("receipt_expired")
	}
	out := copyOf(r.receipt)
	if out.Fault != nil {
		return out, out.Fault
	}
	return out, nil
}
func (a *actor) GetReceipt(ctx context.Context, id dw.RunID) (dw.Receipt, error) {
	if e := ctx.Err(); e != nil {
		return dw.Receipt{}, e
	}
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	if a.closed {
		return dw.Receipt{}, fault("actor_closed")
	}
	r := a.w.runIDs[id]
	if r == nil {
		return dw.Receipt{}, fault("receipt_not_found")
	}
	return a.receiptLocked(r)
}
func (a *actor) Cancel(ctx context.Context, id dw.RunID) (dw.Receipt, error) {
	if e := ctx.Err(); e != nil {
		return dw.Receipt{}, e
	}
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	r := a.w.runIDs[id]
	if r == nil {
		return dw.Receipt{}, fault("receipt_not_found")
	}
	if r.actor != a {
		return dw.Receipt{}, fault("permission_denied")
	}
	r.cancel()
	return a.receiptLocked(r)
}
func (a *actor) Progress(ctx context.Context, id dw.RunID) (<-chan dw.Progress, error) {
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	r := a.w.runIDs[id]
	if r == nil {
		return nil, fault("receipt_not_found")
	}
	if a.closed || r.actor != a {
		return nil, fault("permission_denied")
	}
	return r.progress, ctx.Err()
}
func (a *actor) progress(r *run, s dw.Step, target dw.Ref, phase string) {
	p := dw.Progress{Run: r.receipt.RunID, StepID: s.ID, Target: target, Phase: phase, At: time.Now().UTC()}
	select {
	case r.progress <- p:
	default:
	}
}
func (a *actor) execute(ctx context.Context, r *run, p dw.Plan) {
	w := a.w
	held := false
	late := false
	defer func() {
		r.cancel()
		if held && !late {
			w.seatGate.token <- struct{}{}
		}
		w.mu.Lock()
		r.receipt.State = "terminal"
		r.receipt.EndRevision = w.revision
		r.receipt.SeatHealth = w.seatGate.health()
		r.receipt.ExpiresAt = time.Now().Add(w.opts.ReceiptTTL)
		w.mu.Unlock()
		close(r.progress)
		close(r.done)
	}()
	stop := func(e error) { w.mu.Lock(); r.receipt.Fault = asFault(e); r.receipt.Outcome = "stopped"; w.mu.Unlock() }
	if w.seatGate.health() == "fenced" {
		stop(fault("seat_fenced"))
		return
	}
	select {
	case <-ctx.Done():
		stop(ctx.Err())
		return
	case <-w.seatGate.token:
		held = true
	}
	if w.seatGate.health() == "fenced" {
		stop(fault("seat_fenced"))
		return
	}
	w.mu.Lock()
	r.receipt.State = "running"
	w.mu.Unlock()
	bindings := map[string]dw.Ref{}
	effect := false
	for i, s := range p.Steps {
		sctx, cancel := context.WithTimeout(ctx, s.Timeout)
		res, isLate := a.step(sctx, r, s, bindings)
		cancel()
		late = late || isLate
		w.mu.Lock()
		r.receipt.Steps[i] = res
		r.receipt.Bindings = copyOf(bindings)
		w.mu.Unlock()
		effect = effect || res.Delivery == dw.DeliveryComplete || res.Delivery == dw.DeliveryPartial
		if res.Fault != nil || res.State == "unknown" || res.State == "failed" {
			outcome := "stopped"
			if effect {
				outcome = "partial"
			}
			if res.State == "unknown" || res.Delivery == dw.DeliveryUnknown || res.Verification == dw.VerifyUnknown {
				outcome = "unknown"
			}
			w.mu.Lock()
			r.receipt.Fault = res.Fault
			r.receipt.Outcome = outcome
			w.mu.Unlock()
			a.progress(r, s, res.Target, "stopped")
			return
		}
	}
	w.mu.Lock()
	r.receipt.Outcome = "completed"
	w.mu.Unlock()
}
func resolve(t dw.Target, b map[string]dw.Ref) (dw.Target, error) {
	if t.Bound != "" {
		ref, ok := b[t.Bound]
		if !ok {
			return t, dw.Invalid("unknown alias")
		}
		t = dw.Target{Ref: ref}
	}
	return t, nil
}
func (a *actor) step(ctx context.Context, r *run, s dw.Step, bindings map[string]dw.Ref) (res dw.StepResult, late bool) {
	w := a.w
	res = dw.StepResult{ID: s.ID, State: "failed", Delivery: dw.DeliveryNone, Verification: dw.VerifyNotRequested, StartedAt: time.Now().UTC()}
	w.mu.Lock()
	res.StartRevision = w.revision
	w.mu.Unlock()
	defer func() { res.FinishedAt = time.Now().UTC(); w.mu.Lock(); res.EndRevision = w.revision; w.mu.Unlock() }()
	fail := func(e error) { res.Fault = asFault(e) }
	if e := ctx.Err(); e != nil {
		fail(e)
		return
	}
	a.progress(r, s, "", "resolving")
	if _, e := w.Environment(ctx); e != nil {
		fail(e)
		return
	}
	if s.Op == "bind" || s.Op == "wait" {
		res.Delivery = dw.DeliveryNA
		if e := a.predicates(ctx, s.Before, bindings, s.Op); e != nil {
			fail(e)
			return
		}
	}
	if s.Op == "bind" {
		res.Delivery = dw.DeliveryNA
		in := dw.Intent{Operation: "bind", Scope: dw.Scope{Refs: []dw.Ref{s.Bind.Locator.Within}}, Fields: []string{"name", "role", "states", "capabilities"}}
		if e := a.check(ctx, in, false); e != nil {
			fail(e)
			return
		}
		req := defaults(dw.ObserveRequest{Scope: in.Scope, Projection: dw.ProjectionOutline, Match: &s.Bind.Locator, Budget: dw.Budget{MaxDepth: s.Bind.Locator.MaxDepth}})
		all, cov, e := a.query(ctx, req)
		if e != nil {
			fail(e)
			return
		}
		if !cov.Complete {
			fail(fault("search_incomplete"))
			return
		}
		all = a.selectObjects(all, req)
		if len(all) != 1 {
			fail(fault("ambiguous_target"))
			return
		}
		in.Targets = []dw.Ref{all[0].Ref}
		if e = a.check(ctx, in, false); e != nil {
			fail(e)
			return
		}
		bindings[s.Bind.Name] = all[0].Ref
		res.Target = all[0].Ref
		if len(s.After) > 0 {
			v, e := a.poll(ctx, s.After, bindings, s.Op)
			res.Verification = v
			if e != nil {
				fail(e)
				return
			}
			res.Evidence = copyOf(s.After)
		}
		res.State = "satisfied"
		res.Verification = dw.VerifyVerified
		return
	}
	if s.Op == "wait" {
		res.Delivery = dw.DeliveryNA
		if e := a.check(ctx, dw.Intent{Operation: "wait"}, false); e != nil {
			fail(e)
			return
		}
		v, e := a.poll(ctx, s.After, bindings, "wait")
		res.Verification = v
		if e != nil {
			fail(e)
			return
		}
		res.State = "satisfied"
		res.Evidence = copyOf(s.After)
		return
	}
	target, e := resolve(s.Target, bindings)
	if e != nil {
		fail(e)
		return
	}
	s.Target = target
	if s.Drag != nil {
		to, e := resolve(s.Drag.To, bindings)
		if e != nil {
			fail(e)
			return
		}
		s.Drag.To = to
	}
	res.Target = target.Ref
	if target.Anchor != nil {
		res.Target = target.Anchor.Target
	}
	refs := []dw.Ref{}
	if res.Target != "" {
		refs = append(refs, res.Target)
	}
	if s.Drag != nil {
		if s.Drag.To.Ref != "" {
			refs = append(refs, s.Drag.To.Ref)
		}
		if s.Drag.To.Anchor != nil {
			refs = append(refs, s.Drag.To.Anchor.Target)
		}
	}
	in := dw.Intent{Operation: s.Op, Targets: refs, Step: &s, PlanDigest: r.digest, HasSensitivePayload: s.SetValue != nil || s.TypeText != nil}
	if e = a.check(ctx, in, true); e != nil {
		fail(e)
		return
	}
	if target.Point != nil || (s.Drag != nil && s.Drag.To.Point != nil) {
		if e = a.check(ctx, dw.Intent{Operation: "raw_input", Scope: dw.Scope{Desktop: true}, Step: &s, PlanDigest: r.digest}, true); e != nil {
			fail(e)
			return
		}
	}
	var object dw.Object
	var key backend.Key
	if res.Target != "" {
		object, e = w.read(ctx, res.Target)
		if e != nil {
			fail(e)
			return
		}
		if e = a.actionable(object, s.Op); e != nil {
			fail(e)
			return
		}
		w.mu.Lock()
		key = w.objects[res.Target].key
		w.mu.Unlock()
	}
	if e = a.predicates(ctx, s.Before, bindings, s.Op); e != nil {
		fail(e)
		return
	}
	op := backend.Operation{Step: s, Key: key}
	if strings.HasPrefix(s.Op, "pointer.") {
		point, e := a.point(ctx, target)
		if e != nil {
			fail(e)
			return
		}
		op.Point = &point
		if target.Point == nil {
			v, e := w.call(ctx, func() (any, error) { return w.driver.HitTest(ctx, point, key) })
			if e != nil || !v.(bool) {
				fail(fault("target_not_hittable"))
				return
			}
		}
		if s.Drag != nil {
			to, e := a.point(ctx, s.Drag.To)
			if e != nil {
				fail(e)
				return
			}
			op.To = &to
			if s.Drag.To.Point == nil {
				toRef := s.Drag.To.Ref
				if s.Drag.To.Anchor != nil {
					toRef = s.Drag.To.Anchor.Target
				}
				w.mu.Lock()
				toRec := w.objects[toRef]
				w.mu.Unlock()
				if toRec == nil {
					fail(fault("ref_expired"))
					return
				}
				v, e := w.call(ctx, func() (any, error) { return w.driver.HitTest(ctx, to, toRec.key) })
				if e != nil || !v.(bool) {
					fail(fault("target_not_hittable"))
					return
				}
			}
		}
	}
	if strings.HasPrefix(s.Op, "keyboard.") || strings.HasPrefix(s.Op, "pointer.") && target.Point == nil {
		w.mu.Lock()
		seat := copyOf(w.seat)
		w.mu.Unlock()
		expected := object.Window
		if object.Kind == dw.KindWindow {
			expected = object.Ref
		}
		// Finder's inline editor is a real focused application child with no AX
		// window. Require exact live focus AND foreground app evidence; never
		// invent a window. a.check still rejects actors scoped to another window.
		windowlessKeyboard := strings.HasPrefix(s.Op, "keyboard.") && object.Kind == dw.KindUI && expected == "" && object.App != "" && seat.ForegroundApplication.Status == dw.FactKnown && seat.ForegroundApplication.Value != nil && *seat.ForegroundApplication.Value == object.App && seat.FocusedObject.Status == dw.FactKnown && seat.FocusedObject.Value != nil && *seat.FocusedObject.Value == object.Ref
		if !windowlessKeyboard && (expected == "" || seat.ForegroundWindow.Value == nil || *seat.ForegroundWindow.Value != expected) {
			fail(dw.NewFault("needs_user_focus", "target has no verified foreground window; observe target detail and seat, then explicitly focus the intended window", "reobserve"))
			return
		}
		if strings.HasPrefix(s.Op, "keyboard.") && (seat.FocusedObject.Value == nil || *seat.FocusedObject.Value != object.Ref) {
			fail(dw.NewFault("user_interrupted", "keyboard target must be the currently focused UI object, not its window; observe seat.focused_object and its detail before building a new action", "reobserve"))
			return
		}
	}
	if e = a.check(ctx, in, true); e != nil {
		fail(e)
		return
	}
	a.progress(r, s, res.Target, "dispatching")
	j, e := w.submit(ctx, func() (any, error) {
		if e := ctx.Err(); e != nil {
			return backend.Outcome{Delivery: dw.DeliveryNone, Fault: asFault(e)}, nil
		}
		return w.driver.Perform(ctx, op), nil
	})
	if e != nil {
		fail(e)
		return
	}
	var outcome backend.Outcome
	select {
	case ans := <-j.reply:
		if ans.err != nil {
			fail(ans.err)
			return
		}
		outcome = ans.value.(backend.Outcome)
	case <-ctx.Done():
		select {
		case ans := <-j.reply:
			if ans.err != nil {
				fail(ans.err)
				return
			}
			outcome = ans.value.(backend.Outcome)
		default:
			select {
			case <-j.start:
				res.Delivery = dw.DeliveryUnknown
				res.Verification = dw.VerifyUnknown
				res.State = "unknown"
				res.Fault = dw.NewFault("native_timeout", "native call may still execute", "never_automatically")
				w.seatGate.fence(true)
				late = true
				go func() {
					ans := <-j.reply
					unsafe := false
					if ans.err == nil {
						unsafe = ans.value.(backend.Outcome).Unsafe
					}
					if !unsafe {
						w.seatGate.fence(false)
					}
					w.seatGate.token <- struct{}{}
				}()
				return
			default:
				fail(ctx.Err())
				return
			}
		}
	}
	res.Delivery = outcome.Delivery
	res.AcceptedInputEvents = outcome.Accepted
	res.RequestedInputEvents = outcome.Requested
	if outcome.Unsafe {
		w.seatGate.fence(true)
	}
	if outcome.Fault != nil || outcome.Delivery != dw.DeliveryComplete {
		res.Fault = outcome.Fault
		if res.Fault == nil {
			res.Fault = fault("input_rejected")
		}
		if outcome.Delivery == dw.DeliveryUnknown || outcome.Unsafe {
			res.State = "unknown"
			res.Fault.RetryClass = "never_automatically"
		}
		return
	}
	predicates := append([]dw.Predicate{}, s.After...)
	if s.Op == "focus" {
		prop := "focused"
		if object.Kind == dw.KindWindow {
			prop = "foreground"
		}
		yes := true
		predicates = append(predicates, dw.Predicate{Target: dw.Target{Ref: object.Ref}, Property: prop, EqualsBool: &yes})
	}
	if s.Op == "set_value" {
		predicates = append(predicates, dw.Predicate{Target: dw.Target{Ref: object.Ref}, Property: "value", EqualsString: &s.SetValue.Text})
	}
	if s.Completion == "verify" || s.Op == "focus" || s.Op == "set_value" {
		a.progress(r, s, res.Target, "verifying")
		v, e := a.poll(ctx, predicates, bindings, s.Op)
		res.Verification = v
		res.Evidence = copyOf(predicates)
		if e != nil {
			res.Fault = asFault(e)
			res.Fault.RetryClass = "never_automatically"
			if v == dw.VerifyUnknown {
				res.State = "unknown"
			}
			return
		}
		res.State = "satisfied"
	} else {
		res.State = "dispatched"
	}
	return
}
func (a *actor) actionable(o dw.Object, op string) error {
	if o.Lifecycle != dw.LifeLive {
		return fault("ref_stale")
	}
	if f := o.States["protected"]; f.Status == dw.FactKnown && f.Value != nil && *f.Value {
		return fault("permission_denied")
	}
	if f := o.States["enabled"]; f.Status == dw.FactKnown && f.Value != nil && !*f.Value {
		return fault("capability_unavailable")
	}
	if op == "set_value" {
		f := o.States["read_only"]
		if f.Status != dw.FactKnown || f.Value == nil || *f.Value {
			return fault("capability_unavailable")
		}
	}
	if op == "focus" || op == "invoke" || op == "set_value" {
		for _, c := range o.Capabilities {
			if c.Name == op && c.Support == "supported" && c.Availability == "available" {
				return nil
			}
		}
		return fault("capability_unavailable")
	}
	return nil
}
func (a *actor) point(ctx context.Context, t dw.Target) (dw.Point, error) {
	if t.Point != nil {
		a.w.mu.Lock()
		env := copyOf(a.w.env)
		a.w.mu.Unlock()
		p := *t.Point
		if p.Topology != env.Topology {
			return p, fault("topology_changed")
		}
		for _, d := range env.Displays {
			if d.Frame == p.Frame && p.X >= d.Bounds.X && p.Y >= d.Bounds.Y && p.X < d.Bounds.X+d.Bounds.Width && p.Y < d.Bounds.Y+d.Bounds.Height {
				return p, nil
			}
		}
		return p, fault("point_out_of_bounds")
	}
	an := dw.Anchor{Target: t.Ref, U: .5, V: .5}
	if t.Anchor != nil {
		an = *t.Anchor
	}
	o, e := a.w.read(ctx, an.Target)
	if e != nil {
		return dw.Point{}, e
	}
	if o.Bounds.Status != dw.FactKnown || o.Bounds.Value == nil {
		return dw.Point{}, fault("geometry_unavailable")
	}
	b := *o.Bounds.Value
	p := dw.Point{Frame: b.Frame, Topology: b.Topology, X: b.Rect.X + an.U*b.Rect.Width, Y: b.Rect.Y + an.V*b.Rect.Height, ObservedAt: o.SampleEnd}
	return a.point(ctx, dw.Target{Point: &p})
}
func (a *actor) predicates(ctx context.Context, ps []dw.Predicate, b map[string]dw.Ref, op string) error {
	for _, p := range ps {
		t, e := resolve(p.Target, b)
		if e != nil {
			return e
		}
		if e = a.check(ctx, dw.Intent{Operation: op, Targets: []dw.Ref{t.Ref}, Fields: []string{p.Property}}, false); e != nil {
			return e
		}
		o, e := a.w.read(ctx, t.Ref)
		if e != nil {
			if p.Property == "exists" && p.EqualsBool != nil && !*p.EqualsBool && asFault(e).Code == "ref_gone" {
				continue
			}
			return e
		}
		ok := false
		switch p.Property {
		case "exists":
			ok = p.EqualsBool != nil && *p.EqualsBool
		case "name":
			ok = o.Name.Status == dw.FactKnown && o.Name.Value != nil && reflect.DeepEqual(o.Name.Value, p.EqualsString)
		case "value":
			v, e := a.readTextNative(ctx, t.Ref)
			if e != nil {
				return e
			}
			if v.Source != "value" || v.Value.Status != dw.FactKnown || v.Value.Value == nil {
				return fault("fact_unknown")
			}
			ok = reflect.DeepEqual(v.Value.Value, p.EqualsString)
		case "role":
			ok = p.EqualsString != nil && o.Role == *p.EqualsString
		case "lifecycle":
			ok = p.EqualsString != nil && string(o.Lifecycle) == *p.EqualsString
		case "version":
			ok = p.EqualsVersion != nil && o.Version == *p.EqualsVersion
		case "geometry_version":
			ok = p.EqualsVersion != nil && o.GeometryVersion == *p.EqualsVersion
		case "foreground":
			a.w.mu.Lock()
			f := a.w.seat.ForegroundWindow
			a.w.mu.Unlock()
			if f.Value == nil {
				return fault("fact_unknown")
			}
			v := *f.Value == o.Ref
			ok = p.EqualsBool != nil && v == *p.EqualsBool
		default:
			f := o.States[p.Property]
			if f.Status != dw.FactKnown || f.Value == nil {
				return fault("fact_unknown")
			}
			ok = reflect.DeepEqual(f.Value, p.EqualsBool)
		}
		if !ok {
			return fault("precondition_failed")
		}
	}
	return nil
}
func (a *actor) poll(ctx context.Context, ps []dw.Predicate, b map[string]dw.Ref, op string) (dw.Verification, error) {
	last := dw.VerifyUnknown
	for {
		if ctx.Err() != nil {
			return last, fault("verification_timeout")
		}
		e := a.predicates(ctx, ps, b, op)
		if e == nil {
			return dw.VerifyVerified, nil
		}
		f := asFault(e)
		if f.Code == "precondition_failed" {
			last = dw.VerifyNotMet
		} else {
			last = dw.VerifyUnknown
			if f.Code == "permission_denied" || f.Code == "ref_gone" || f.Code == "ref_expired" {
				return last, e
			}
		}
		timer := time.NewTimer(a.w.opts.VerificationPollInterval)
		select {
		case <-ctx.Done():
			timer.Stop()
			return last, fault("verification_timeout")
		case <-timer.C:
		}
	}
}
