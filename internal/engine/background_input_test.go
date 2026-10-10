package engine_test

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/engine"
	"github.com/caelis-labs/desktop-world/internal/testutil"
	dw "github.com/caelis-labs/desktop-world/internal/world"
)

type occludedDriver struct{ *dwtest.Fixture }

func (*occludedDriver) HitTest(context.Context, dw.Point, backend.Key) (bool, error) {
	return false, nil
}

type targetedDriver struct{ *occludedDriver }

func (*targetedDriver) TargetsInput(op string) bool {
	return op == "pointer.click" || op == "keyboard.type_text"
}

func pocWorld(t *testing.T, targeted bool) (dw.World, *dwtest.Fixture, dw.Epoch, map[string]dw.Ref) {
	return pocWorldDriver(t, func(f *occludedDriver) backend.Driver {
		if targeted {
			return &targetedDriver{f}
		}
		return f
	})
}
func pocWorldDriver(t *testing.T, factory func(*occludedDriver) backend.Driver) (dw.World, *dwtest.Fixture, dw.Epoch, map[string]dw.Ref) {
	t.Helper()
	ctx := context.Background()
	w0, f, err := dwtest.New(ctx)
	if err != nil {
		t.Fatal(err)
	}
	w0.Close(ctx)
	f.Form()
	f.Add(dwtest.Node{ID: "human-app", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("human")}})
	f.Add(dwtest.Node{ID: "human-window", App: "human-app", Parent: "human-app", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("human window")}})
	f.Add(dwtest.Node{ID: "human-field", App: "human-app", Window: "human-window", Parent: "human-window", Object: dw.Object{Kind: dw.KindUI, Role: "text_field", Name: dw.Known("human field")}})
	f.Focus("human-field")
	driver := factory(&occludedDriver{f})
	w, err := engine.Open(ctx, driver, engine.Options{SeatID: t.Name()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { w.Close(ctx) })
	a, _ := w.NewActor(ctx, dw.ActorConfig{ID: "reader", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	ob, err := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline})
	if err != nil {
		t.Fatal(err)
	}
	refs := map[string]dw.Ref{}
	for _, o := range ob.Objects {
		if o.Name.Value != nil {
			refs[*o.Name.Value] = o.Ref
		}
	}
	env, _ := w.Environment(ctx)
	return w, f, env.Epoch, refs
}
func faultCode(err error) string {
	var f *dw.Fault
	if errors.As(err, &f) {
		return f.Code
	}
	return ""
}

func TestBackgroundPOCGuardsAndReceipts(t *testing.T) {
	for _, targeted := range []bool{false, true} {
		name := "ordinary"
		if targeted {
			name = "targeted"
		}
		t.Run(name, func(t *testing.T) {
			w, f, epoch, refs := pocWorld(t, targeted)
			ctx := context.Background()
			a, err := w.NewActor(ctx, dw.ActorConfig{ID: "writer", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{refs["Desktop World Fixture"]}}}, Operations: []string{"pointer.click", "keyboard.type_text"}})
			if err != nil {
				t.Fatal(err)
			}
			p := dw.Plan{Epoch: epoch, RequestID: dw.RequestID(string(epoch) + ":click"), Steps: []dw.Step{{ID: "click", Op: "pointer.click", Target: dw.Target{Ref: refs["内容"]}, Click: &dw.Click{Button: "left", Count: 1}}}}
			r, err := a.Execute(ctx, p)
			if !targeted {
				if faultCode(err) != "target_not_hittable" || r.Steps[0].Channel != "shared_input" || len(f.Events()) != 0 {
					t.Fatal(r, err, f.Events())
				}
				p.RequestID = dw.RequestID(string(epoch) + ":type")
				p.Steps = []dw.Step{{ID: "type", Op: "keyboard.type_text", Target: dw.Target{Ref: refs["内容"]}, TypeText: &dw.TypeText{Text: "text"}}}
				if _, err = a.Execute(ctx, p); faultCode(err) != "needs_user_focus" {
					t.Fatal(err)
				}
				return
			}
			if err != nil || r.Steps[0].Channel != "targeted_background" || len(f.Events()) != 1 {
				t.Fatal(r, err)
			}
			again, err := a.Execute(ctx, p)
			if err != nil || again.RunID != r.RunID || len(f.Events()) != 1 {
				t.Fatal("replayed", again, err)
			}
			p.RequestID = dw.RequestID(string(epoch) + ":other-scope")
			p.Steps[0].Target.Ref = refs["human field"]
			if r, err = a.Execute(ctx, p); faultCode(err) != "permission_denied" || r.Steps[0].Delivery != dw.DeliveryNone || len(f.Events()) != 1 {
				t.Fatal(r, err)
			}
			p.RequestID = dw.RequestID(string(epoch) + ":unknown")
			p.Steps[0].Target.Ref = refs["内容"]
			f.Enqueue("pointer.click", dwtest.Behavior{Delivery: dw.DeliveryUnknown, Apply: true, Unsafe: true, Fault: dw.NewFault("native_timeout", "unknown", "never_automatically")})
			r, err = a.Execute(ctx, p)
			if err == nil || r.Outcome != "unknown" || r.SeatHealth != "fenced" {
				t.Fatal(r, err)
			}
			again, _ = a.Execute(ctx, p)
			if again.RunID != r.RunID || len(f.Events()) != 2 {
				t.Fatal("unknown replayed")
			}
			p.RequestID = dw.RequestID(string(epoch) + ":fenced")
			if _, err = a.Execute(ctx, p); faultCode(err) != "seat_fenced" || len(f.Events()) != 2 {
				t.Fatal(err)
			}
		})
	}
}

func TestBackgroundPOCDoesNotOverridePolicyOrStaleIdentity(t *testing.T) {
	w, f, epoch, refs := pocWorld(t, true)
	ctx := context.Background()
	config := dw.ActorConfig{ID: "strict", InputPolicy: dw.InputNoShared, ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"set_value", "pointer.click"}}
	a, _ := w.NewActor(ctx, config)
	p := dw.Plan{Epoch: epoch, RequestID: dw.RequestID(string(epoch) + ":strict"), Steps: []dw.Step{
		{ID: "write", Op: "set_value", Target: dw.Target{Ref: refs["内容"]}, SetValue: &dw.SetValue{Text: "must not happen"}},
		{ID: "click", Op: "pointer.click", Target: dw.Target{Ref: refs["内容"]}, Click: &dw.Click{Button: "left", Count: 1}},
	}}
	if r, err := a.Execute(ctx, p); faultCode(err) != "requires_shared_input" || r.Steps[0].Delivery != dw.DeliveryNone || len(f.Events()) != 0 {
		t.Fatal(r, err)
	}
	config.ID, config.InputPolicy = "stale", dw.InputShared
	a, _ = w.NewActor(ctx, config)
	f.Remove("field")
	p.RequestID = dw.RequestID(string(epoch) + ":stale")
	p.Steps = p.Steps[1:]
	p.Timeout = time.Second
	if r, err := a.Execute(ctx, p); err == nil || r.Steps[0].Delivery != dw.DeliveryNone || len(f.Events()) != 0 {
		t.Fatal(r, err)
	}
}
