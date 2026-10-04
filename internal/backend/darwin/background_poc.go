//go:build dtw_background_poc && darwin && cgo

package darwin

/*
#cgo CFLAGS: -DDTW_BACKGROUND_POC
*/
import "C"

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

type backgroundPOC struct {
	*Driver
	mode string
}

func NewBackgroundPOC(mode string) backend.Driver {
	return &backgroundPOC{Driver: &Driver{}, mode: mode}
}
func (d *backgroundPOC) TargetsInput(op string) bool {
	return op == "pointer.click" || op == "keyboard.type_text"
}
func (d *backgroundPOC) Environment(ctx context.Context) (dw.Environment, error) {
	e, err := d.Driver.Environment(ctx)
	e.Capabilities = append(e.Capabilities, dw.Capability{Name: "targeted_input_poc", Support: "unknown", Availability: "unknown", Reason: "Experimental same-desktop " + d.mode + ": scoped left single click and <=256 UTF-16 text units only. Private window routing; app-specific acceptance and verification required."})
	return e, err
}
func (d *backgroundPOC) Perform(ctx context.Context, op backend.Operation) (v backend.Outcome) {
	if !d.TargetsInput(op.Step.Op) {
		if dw.ActionChannel(op.Step.Op) == "semantic" {
			return d.Driver.Perform(ctx, op)
		}
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("background_unavailable", "POC allows scoped left click, short text and semantic actions only", "never_automatically")}
	}
	if ctx.Err() != nil {
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("cancelled", "cancelled before native dispatch", "reobserve")}
	}
	if err := d.call(ctx, "background_poc", map[string]any{"Operation": op, "Mode": d.mode}, &v); err != nil {
		v = backend.Outcome{Delivery: dw.DeliveryUnknown, Unsafe: true, Fault: dw.NewFault("provider_unavailable", err.Error(), "never_automatically")}
	}
	return
}
