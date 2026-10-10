package contract_test

import (
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/testutil"
	"testing"
)

func TestNoSharedInputRejectsEntireMixedPlanAndReconciles(t *testing.T) {
	h := setup(t)
	a, err := h.w.NewActor(ctx, dw.ActorConfig{ID: "background", InputPolicy: dw.InputNoShared, ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: allOps})
	if err != nil {
		t.Fatal(err)
	}
	p := h.plan("mixed-policy", dw.Step{ID: "write", Op: "set_value", Target: target(h.refs["内容"]), SetValue: &dw.SetValue{Text: "must not happen"}}, dw.Step{ID: "enter", Op: "keyboard.press", Target: target(h.refs["内容"]), Press: &dw.KeyChord{Key: "Enter"}})
	r, err := a.Execute(ctx, p)
	if code(err) != "requires_shared_input" || r.Outcome != "stopped" || r.RunID == "" || len(h.f.Events()) != 0 {
		t.Fatalf("%+v %v events=%v", r, err, h.f.Events())
	}
	again, err := a.Execute(ctx, p)
	if code(err) != "requires_shared_input" || again.RunID != r.RunID || len(h.f.Events()) != 0 {
		t.Fatal("policy retry changed original receipt")
	}
	for i, step := range []dw.Step{
		{ID: "focus", Op: "focus", Target: target(h.refs["内容"])},
		{ID: "move", Op: "pointer.move", Target: target(h.refs["内容"])},
	} {
		r, err = a.Execute(ctx, h.plan(string(r.RunID)+string(rune('a'+i)), step))
		if code(err) != "requires_shared_input" || len(h.f.Events()) != 0 {
			t.Fatal(r, err)
		}
	}
	p = h.plan("semantic-policy", dw.Step{ID: "write", Op: "set_value", Target: target(h.refs["内容"]), SetValue: &dw.SetValue{Text: "background"}}, dw.Step{ID: "submit", Op: "invoke", Target: target(h.refs["提交"])})
	r, err = a.Execute(ctx, p)
	if err != nil || r.Outcome != "completed" || r.Steps[0].Channel != "semantic" || r.Steps[1].Channel != "semantic" {
		t.Fatal(r, err)
	}
}

func TestExpandedDesiredStateNoopVerificationAndUnknownNeverReplay(t *testing.T) {
	h := setup(t)
	h.f.Update("field", func(n *dwtest.Node) {
		n.Object.States["expanded"] = dw.Known(false)
		n.Object.Capabilities = append(n.Object.Capabilities, dw.Capability{Name: "set_expanded", Support: "supported", Availability: "available"})
	})
	ops := append(append([]string{}, allOps...), "set_expanded")
	a, err := h.w.NewActor(ctx, dw.ActorConfig{ID: "expander", InputPolicy: dw.InputNoShared, ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: ops})
	if err != nil {
		t.Fatal(err)
	}
	yes := true
	step := dw.Step{ID: "expand", Op: "set_expanded", Target: target(h.refs["内容"]), SetExpanded: &dw.SetExpanded{Expanded: &yes}}
	p := h.plan("expand", step)
	r, err := a.Execute(ctx, p)
	if err != nil || r.Steps[0].Verification != dw.VerifyVerified || len(h.f.Events()) != 1 {
		t.Fatal(r, err, h.f.Events())
	}
	r, err = a.Execute(ctx, h.plan("already-expanded", step))
	if err != nil || r.Steps[0].Delivery != dw.DeliveryNA || len(h.f.Events()) != 1 {
		t.Fatal(r, err, h.f.Events())
	}
	no := false
	step.SetExpanded = &dw.SetExpanded{Expanded: &no}
	p = h.plan("collapse", step)
	r, err = a.Execute(ctx, p)
	if err != nil || len(h.f.Events()) != 2 {
		t.Fatal(r, err)
	}
	again, err := a.Execute(ctx, p)
	if err != nil || again.RunID != r.RunID || len(h.f.Events()) != 2 {
		t.Fatal("collapse replayed")
	}
	h.f.Update("field", func(n *dwtest.Node) { n.Object.States["expanded"] = dw.Unknown[bool]() })
	h.f.Enqueue("set_expanded", dwtest.Behavior{Delivery: dw.DeliveryUnknown, Apply: true, Fault: dw.NewFault("native_timeout", "unknown", "never_automatically")})
	p = h.plan("unknown-expand", step)
	r, err = a.Execute(ctx, p)
	if err == nil || r.Outcome != "unknown" {
		t.Fatal(r, err)
	}
	before := len(h.f.Events())
	again, _ = a.Execute(ctx, p)
	if again.RunID != r.RunID || len(h.f.Events()) != before {
		t.Fatal("unknown replayed")
	}
	step.SetExpanded = &dw.SetExpanded{}
	if _, err = a.Execute(ctx, h.plan("missing-state", step)); code(err) != "invalid_argument" {
		t.Fatal(err)
	}
}
