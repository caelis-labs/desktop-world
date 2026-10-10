//go:build dtw_background_poc && dtw_virtual_input_poc && darwin && cgo

package darwin

/*
#cgo CFLAGS: -DDTW_VIRTUAL_INPUT_POC
*/
import "C"

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

type virtualInputPOC struct {
	*cooperative
	background *backgroundPOC
}

func NewVirtualInputPOC() backend.Driver {
	base := &Driver{}
	return &virtualInputPOC{cooperative: &cooperative{base}, background: &backgroundPOC{Driver: base, mode: "public_pid"}}
}

func (d *virtualInputPOC) InputChannel() string { return "automatic_input" }

func (d *virtualInputPOC) Perform(ctx context.Context, op backend.Operation) backend.Outcome {
	if !d.TargetsInput(op.Step.Op) {
		return d.Driver.Perform(ctx, op)
	}
	backgroundEligible := d.background.TargetsInput(op.Step.Op)
	if op.Step.Op == "pointer.click" {
		// The PID-only route is accepted for standard AppKit controls. Custom
		// canvases may consume a posted pair without invoking their handler;
		// select the short foreground path before dispatch for those roles.
		node, _, err := d.Driver.Read(ctx, op.Key)
		backgroundEligible = err == nil && (node.Object.Role == "button" || node.Object.Role == "text_field" || node.Object.Role == "text_area")
	}
	if backgroundEligible {
		out := d.background.Perform(ctx, op)
		out.Channel = "targeted_background"
		if out.Delivery != dw.DeliveryNone || out.Fault == nil {
			return out
		}
		// Only proven pre-dispatch unavailability may select a foreground route.
		// Unknown or partial effects always retain their original receipt.
		switch out.Fault.Code {
		case "background_unavailable", "background_target_not_focused":
		default:
			return out
		}
	}
	out := d.cooperative.Perform(ctx, op)
	out.Channel = "targeted_foreground"
	return out
}
