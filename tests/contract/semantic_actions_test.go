package contract_test

import (
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"testing"
	"time"
)

func TestSemanticDesiredStates(t *testing.T) {
	for _, op := range []string{"set_checked", "set_selected", "scroll_into_view"} {
		t.Run(op, func(t *testing.T) {
			h := setup(t)
			property := map[string]string{"set_checked": "checked", "set_selected": "selected", "scroll_into_view": "offscreen"}[op]
			initial := op == "scroll_into_view"
			h.f.Update("field", func(n *dwtest.Node) {
				n.Object.States[property] = dw.Known(initial)
				n.Object.Capabilities = append(n.Object.Capabilities, dw.Capability{Name: op, Support: "supported", Availability: "available"})
			})
			a, err := h.w.NewActor(ctx, dw.ActorConfig{ID: dw.ActorID(op), InputPolicy: dw.InputNoShared, ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: append(append([]string{}, allOps...), op)})
			if err != nil {
				t.Fatal(err)
			}
			makeStep := func(value bool) dw.Step {
				s := dw.Step{ID: "desired", Op: op, Target: target(h.refs["内容"]), Completion: "dispatch"}
				if op == "set_checked" {
					s.SetChecked = &dw.SetChecked{Checked: &value}
				}
				if op == "set_selected" {
					s.SetSelected = &dw.SetSelected{Selected: &value}
				}
				return s
			}
			s := makeStep(true)
			r, err := a.Execute(ctx, h.plan("first", s))
			if err != nil || r.Outcome != "completed" || r.Steps[0].Channel != "semantic" || r.Steps[0].Verification != dw.VerifyVerified || len(h.f.Events()) != 1 {
				t.Fatal(r, err, h.f.Events())
			}
			r, err = a.Execute(ctx, h.plan("noop", s))
			if err != nil || r.Steps[0].Delivery != dw.DeliveryNA || r.Steps[0].Verification != dw.VerifyVerified || len(h.f.Events()) != 1 {
				t.Fatal(r, err, h.f.Events())
			}
			if op != "scroll_into_view" {
				s = makeStep(false)
				p := h.plan("explicit-false", s)
				r, err = a.Execute(ctx, p)
				if err != nil || r.Steps[0].Verification != dw.VerifyVerified || len(h.f.Events()) != 2 {
					t.Fatal(r, err)
				}
				again, err := a.Execute(ctx, p)
				if err != nil || again.RunID != r.RunID || len(h.f.Events()) != 2 {
					t.Fatal("false replayed", again, err)
				}
			}
			// Unknown dispatch may have applied the state. Recover the original
			// receipt even after the state is now known; never dispatch it again.
			h.f.Update("field", func(n *dwtest.Node) { n.Object.States[property] = dw.Unknown[bool]() })
			h.f.Enqueue(op, dwtest.Behavior{Delivery: dw.DeliveryUnknown, Apply: true, Fault: dw.NewFault("native_timeout", "uncertain delivery", "never_automatically")})
			p := h.plan("unknown", s)
			r, err = a.Execute(ctx, p)
			if err == nil || r.Outcome != "unknown" {
				t.Fatal(r, err)
			}
			count := len(h.f.Events())
			again, _ := a.Execute(ctx, p)
			if again.RunID != r.RunID || len(h.f.Events()) != count {
				t.Fatal("unknown dispatch replayed")
			}
			// A provider's complete delivery does not prove the desired state.
			h.f.Update("field", func(n *dwtest.Node) { n.Object.States[property] = dw.Known(initial) })
			h.f.Enqueue(op, dwtest.Behavior{Delivery: dw.DeliveryComplete, Apply: false})
			s = makeStep(true)
			s.Timeout = 30 * time.Millisecond
			r, err = a.Execute(ctx, h.plan("not-applied", s))
			if err == nil || r.Outcome == "completed" || r.Steps[0].Verification == dw.VerifyVerified {
				t.Fatal("dispatch bypassed verification", r, err)
			}
			// Capability absence refuses a desired-state action before delivery.
			h.f.Update("field", func(n *dwtest.Node) { n.Object.Capabilities = nil })
			count = len(h.f.Events())
			r, err = a.Execute(ctx, h.plan("unsupported", s))
			if code(err) != "capability_unavailable" || r.Steps[0].Delivery != dw.DeliveryNone || len(h.f.Events()) != count {
				t.Fatal(r, err)
			}
			if op != "scroll_into_view" {
				s = makeStep(true)
				if op == "set_checked" {
					s.SetChecked = &dw.SetChecked{}
				} else {
					s.SetSelected = &dw.SetSelected{}
				}
				if _, err = a.Execute(ctx, h.plan("omitted", s)); code(err) != "invalid_argument" {
					t.Fatal(err)
				}
			}
		})
	}
}

func TestNewSemanticActionsRejectMixedInputBeforeDelivery(t *testing.T) {
	for _, op := range []string{"set_checked", "set_selected", "scroll_into_view"} {
		t.Run(op, func(t *testing.T) {
			h := setup(t)
			a, err := h.w.NewActor(ctx, dw.ActorConfig{ID: dw.ActorID(op), InputPolicy: dw.InputNoShared, ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: append(append([]string{}, allOps...), op)})
			if err != nil {
				t.Fatal(err)
			}
			yes := true
			s := dw.Step{ID: "desired", Op: op, Target: target(h.refs["内容"])}
			if op == "set_checked" {
				s.SetChecked = &dw.SetChecked{Checked: &yes}
			}
			if op == "set_selected" {
				s.SetSelected = &dw.SetSelected{Selected: &yes}
			}
			r, err := a.Execute(ctx, h.plan("mixed", s, dw.Step{ID: "enter", Op: "keyboard.press", Target: s.Target, Press: &dw.KeyChord{Key: "Enter"}}))
			if code(err) != "requires_shared_input" || len(h.f.Events()) != 0 || r.Steps[0].Delivery != dw.DeliveryNone {
				t.Fatal(r, err)
			}
		})
	}
}
