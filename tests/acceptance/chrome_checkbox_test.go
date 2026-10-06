package acceptance_test

import (
	"encoding/json"
	"os"
	"strconv"
	"testing"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/protocol"
)

// Opt-in direct-host regression on a fresh private Chrome profile and page.
// The caller supplies an exclusive once marker and independently checks the
// page's DOM/event log. This test never uses pointer or keyboard fallback.
func TestNativeChromeCheckboxDesiredState(t *testing.T) {
	title := os.Getenv("DTW_CHROME_TITLE")
	name := os.Getenv("DTW_CHROME_CHECKBOX")
	disabledName := os.Getenv("DTW_CHROME_DISABLED")
	once := os.Getenv("DTW_CHROME_ONCE")
	evidencePath := os.Getenv("DTW_CHROME_EVIDENCE")
	if title == "" || name == "" || disabledName == "" || once == "" || evidencePath == "" {
		t.Skip("fresh private Chrome page, exact targets, once marker and evidence path required")
	}
	s := featureStart(t, dw.InputNoShared)
	window := s.window(title)
	check := s.findRole(window.Ref, name, "checkbox", "name", "role", "states", "capabilities")
	disabled := s.findRole(window.Ref, disabledName, "checkbox", "name", "role", "states", "capabilities")
	if f := check.States["checked"]; f.Status != dw.FactKnown || f.Value == nil || *f.Value {
		t.Fatalf("checkbox must start known false: %+v", f)
	}
	capability := func(o dw.Object) string {
		for _, c := range o.Capabilities {
			if c.Name == "set_checked" {
				return c.Support + "/" + c.Availability
			}
		}
		return "absent"
	}
	if capability(check) != "supported/available" || capability(disabled) == "supported/available" {
		t.Fatalf("incorrect checkbox capabilities: enabled=%s disabled=%s", capability(check), capability(disabled))
	}
	s.grant(window.App)
	step := func(ref dw.Ref, desired bool) dw.Step {
		return dw.Step{ID: "check", Op: "set_checked", Target: dw.Target{Ref: ref}, SetChecked: &dw.SetChecked{Checked: &desired}, Completion: "verify"}
	}
	// A disabled target is refused before native delivery.
	bad, err := s.c.Call(s.ctx, "feature", "chrome-disabled", "act", struct{ Steps []dw.Step }{[]dw.Step{step(disabled.Ref, true)}})
	var refused dw.Receipt
	if err != nil || bad.Error == nil || bad.Error.Code != "capability_unavailable" || protocol.Decode(bad.Result, &refused) != nil || len(refused.Steps) != 1 || refused.Steps[0].Delivery != dw.DeliveryNone {
		t.Fatalf("disabled checkbox not refused before delivery: %+v %v", refused, err)
	}
	marker, err := os.OpenFile(once, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		t.Fatal("checkbox input already attempted or marker unavailable: ", err)
	}
	if _, err = marker.WriteString(title + "\n"); err != nil {
		marker.Close()
		t.Fatal(err)
	}
	if err = marker.Close(); err != nil {
		t.Fatal(err)
	}
	on := s.act(step(check.Ref, true))
	if len(on.Steps) != 1 || on.Steps[0].Delivery != dw.DeliveryComplete || on.Steps[0].Verification != dw.VerifyVerified {
		t.Fatal("true transition not verified: ", on)
	}
	onID := "f-" + strconv.Itoa(s.seq)
	recovered, err := s.c.Reconcile(s.ctx, "feature", onID)
	var original dw.Receipt
	if err != nil || protocol.Decode(recovered.Result, &original) != nil || original.RunID != on.RunID {
		t.Fatalf("original request changed: %+v %v", original, err)
	}
	noop := s.act(step(check.Ref, true))
	if len(noop.Steps) != 1 || noop.Steps[0].Delivery != dw.DeliveryNA || noop.Steps[0].Verification != dw.VerifyVerified {
		t.Fatal("already checked target was not a verified no-op: ", noop)
	}
	off := s.act(step(check.Ref, false))
	if len(off.Steps) != 1 || off.Steps[0].Delivery != dw.DeliveryComplete || off.Steps[0].Verification != dw.VerifyVerified {
		t.Fatal("false transition not verified: ", off)
	}
	evidence := map[string]any{"window": title, "enabled_capability": capability(check), "disabled_capability": capability(disabled), "disabled_receipt": refused, "on": on, "original_reconcile_same_run": original.RunID == on.RunID, "noop": noop, "off": off, "input_mode": s.c.Hello.InputMode}
	data, err := json.MarshalIndent(evidence, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(evidencePath, append(data, '\n'), 0600); err != nil {
		t.Fatal(err)
	}
	t.Logf("Chrome semantic checked true/true/false: %s/%s/%s, original run %s", on.Steps[0].Delivery, noop.Steps[0].Delivery, off.Steps[0].Delivery, on.RunID)
}

// Chrome's reduced AX mode can claim AXValue is settable while exposing an
// empty value and no AXPress. A read-only checkbox can advertise AXPress but
// deny AXValue writes. Neither is sufficient evidence for desired-state input.
func TestNativeChromeCheckboxUnavailable(t *testing.T) {
	title := os.Getenv("DTW_CHROME_UNAVAILABLE_TITLE")
	name := os.Getenv("DTW_CHROME_UNAVAILABLE_CHECKBOX")
	once := os.Getenv("DTW_CHROME_UNAVAILABLE_ONCE")
	evidencePath := os.Getenv("DTW_CHROME_UNAVAILABLE_EVIDENCE")
	if title == "" || name == "" || once == "" || evidencePath == "" {
		t.Skip("fresh private Chrome page, unavailable target, once marker and evidence path required")
	}
	s := featureStart(t, dw.InputNoShared)
	window := s.window(title)
	check := s.findRole(window.Ref, name, "checkbox", "name", "role", "states", "capabilities")
	if f := check.States["checked"]; os.Getenv("DTW_CHROME_UNAVAILABLE_KNOWN_FALSE") == "1" {
		if f.Status != dw.FactKnown || f.Value == nil || *f.Value {
			t.Fatalf("read-only checkbox must start known false: %+v", f)
		}
		if ro := check.States["read_only"]; ro.Status != dw.FactKnown || ro.Value == nil || !*ro.Value {
			t.Fatalf("read-only checkbox must advertise read_only=true: %+v", ro)
		}
	} else if f.Status != dw.FactUnknown || f.Value != nil {
		t.Fatalf("reduced AX checkbox state must remain unknown: %+v", f)
	}
	for _, c := range check.Capabilities {
		if c.Name == "set_checked" && c.Support == "supported" && c.Availability == "available" {
			t.Fatal("unusable checkbox was advertised as actionable")
		}
	}
	s.grant(window.App)
	marker, err := os.OpenFile(once, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		t.Fatal("unavailable checkbox request already attempted or marker unavailable: ", err)
	}
	marker.Close()
	yes := true
	reply, err := s.c.Call(s.ctx, "feature", "chrome-unavailable-once", "act", struct{ Steps []dw.Step }{[]dw.Step{{ID: "check", Op: "set_checked", Target: dw.Target{Ref: check.Ref}, SetChecked: &dw.SetChecked{Checked: &yes}}}})
	var receipt dw.Receipt
	if err != nil || reply.Error == nil || reply.Error.Code != "capability_unavailable" || protocol.Decode(reply.Result, &receipt) != nil || len(receipt.Steps) != 1 || receipt.Steps[0].Delivery != dw.DeliveryNone {
		t.Fatalf("unusable checkbox was dispatched: %+v %v", receipt, err)
	}
	recovered, err := s.c.Reconcile(s.ctx, "feature", "chrome-unavailable-once")
	var same dw.Receipt
	if err != nil || protocol.Decode(recovered.Result, &same) != nil || same.RunID != receipt.RunID {
		t.Fatalf("refusal receipt changed: %+v %v", same, err)
	}
	data, err := json.MarshalIndent(map[string]any{"window": title, "checked_state": check.States["checked"], "capabilities": check.Capabilities, "receipt": receipt, "reconcile_same_run": same.RunID == receipt.RunID}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(evidencePath, append(data, '\n'), 0600); err != nil {
		t.Fatal(err)
	}
	t.Log("reduced AX checkbox refused before dispatch; original receipt preserved")
}
