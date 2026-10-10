//go:build darwin && cgo

package darwin

/*
#cgo CFLAGS: -DDTW_BACKGROUND_INPUT
*/
import "C"

import (
	"context"
	"github.com/caelis-labs/desktop-world/internal/backend"
	dw "github.com/caelis-labs/desktop-world/internal/world"
)

type backgroundInput struct {
	*Driver
	mode string
}

func NewBackgroundInput(mode string) backend.Driver {
	return &backgroundInput{Driver: &Driver{}, mode: mode}
}
func (d *backgroundInput) TargetsInput(op string) bool {
	switch op {
	case "pointer.move", "pointer.click", "pointer.scroll", "keyboard.press", "keyboard.type_text":
		return true
	default:
		return false
	}
}
func (d *backgroundInput) Environment(ctx context.Context) (dw.Environment, error) {
	e, err := d.Driver.Environment(ctx)
	e.Capabilities = append(e.Capabilities, dw.Capability{Name: "targeted_background", Support: "unknown", Availability: "unknown", Reason: "Same-desktop window-targeted mouse, wheel and keyboard delivery. Drag and explicit focus require foreground; posted input still needs application-effect verification."})
	return e, err
}
func (d *backgroundInput) Perform(ctx context.Context, op backend.Operation) (v backend.Outcome) {
	if !d.TargetsInput(op.Step.Op) {
		if dw.ActionChannel(op.Step.Op) == "semantic" {
			return d.Driver.Perform(ctx, op)
		}
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("background_unavailable", "operation has no background route", "never_automatically")}
	}
	if ctx.Err() != nil {
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("cancelled", "cancelled before native dispatch", "reobserve")}
	}
	if err := d.call(ctx, "background_input", map[string]any{"Operation": op, "Mode": d.mode}, &v); err != nil {
		if f, ok := preDispatchFault(err); ok {
			return backend.Outcome{Delivery: dw.DeliveryNone, Fault: f}
		}
		v = backend.Outcome{Delivery: dw.DeliveryUnknown, Unsafe: true, Fault: dw.NewFault("provider_unavailable", err.Error(), "never_automatically")}
	}
	return
}
