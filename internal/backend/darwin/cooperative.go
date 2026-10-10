//go:build darwin && cgo

package darwin

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"time"
)

// cooperative keeps semantic operations on their original channel. Physical
// input is an explicit, plan-bounded foreground transaction, never a retry of
// a failed semantic/PID event. All methods run on the engine's native worker.
type cooperative struct{ *Driver }

func NewCooperative() backend.Driver { return &cooperative{&Driver{}} }
func (*cooperative) TargetsInput(op string) bool {
	return dw.ActionChannel(op) == "shared_input" || op == "focus"
}
func (*cooperative) InputChannel() string { return "foreground_transaction" }
func (d *cooperative) BeginInput(ctx context.Context) error {
	var v bool
	return d.call(ctx, "input_begin", nil, &v)
}
func (d *cooperative) EndInput(ctx context.Context) (v dw.InputReport, err error) {
	err = d.call(ctx, "input_end", nil, &v)
	return
}
func (d *cooperative) guard(ctx context.Context) error {
	var v bool
	return d.call(ctx, "input_guard", nil, &v)
}
func (d *cooperative) Environment(ctx context.Context) (v dw.Environment, err error) {
	if err = d.guard(ctx); err != nil {
		return
	}
	v, err = d.Driver.Environment(ctx)
	v.InputMode = dw.InputModeCooperative
	available := false
	if e := d.call(ctx, "input_available", nil, &available); e != nil {
		return v, e
	}
	support, availability := "supported", "available"
	if !available {
		support, availability = "unsupported", "blocked"
	}
	for _, p := range v.Permissions {
		if (p.Name == "input" || p.Name == "accessibility") && p.State != "granted" {
			availability = "blocked"
		}
	}
	v.Capabilities = append(v.Capabilities, dw.Capability{Name: "foreground_transaction", Support: support, Availability: availability, Reason: "Opt-in same-desktop scoped transactions; private exact-window key-focus SPI. One-second input budget plus bounded native handoff/cleanup, <=256 UTF-16 text units and <=500ms drag. No independent physical devices. Verify task results separately."})
	return
}
func (d *cooperative) Query(ctx context.Context, q backend.Query) (v backend.Page, err error) {
	if err = d.guard(ctx); err != nil {
		return
	}
	return d.Driver.Query(ctx, q)
}
func (d *cooperative) Read(ctx context.Context, k backend.Key) (v backend.Node, s backend.Seat, err error) {
	if err = d.guard(ctx); err != nil {
		return
	}
	return d.Driver.Read(ctx, k)
}
func (d *cooperative) ReadText(ctx context.Context, k backend.Key) (v backend.Text, err error) {
	if err = d.guard(ctx); err != nil {
		return
	}
	return d.Driver.ReadText(ctx, k)
}
func (d *cooperative) Perform(ctx context.Context, op backend.Operation) (v backend.Outcome) {
	if err := d.guard(ctx); err != nil {
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("input_transaction_stopped", err.Error(), "reobserve")}
	}
	if !d.TargetsInput(op.Step.Op) {
		return d.Driver.Perform(ctx, op)
	}
	if err := d.call(ctx, "cooperative_perform", op, &v); err != nil {
		if f, ok := preDispatchFault(err); ok {
			return backend.Outcome{Delivery: dw.DeliveryNone, Fault: f}
		}
		return backend.Outcome{Delivery: dw.DeliveryUnknown, Unsafe: true, Fault: dw.NewFault("provider_unavailable", err.Error(), "never_automatically")}
	}
	return
}

func (d *cooperative) Close(ctx context.Context) error {
	if d.ptr == nil {
		return nil
	}
	cleanup, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, err := d.EndInput(cleanup)
	closeErr := d.Driver.Close(ctx)
	if err != nil {
		return err
	}
	return closeErr
}
