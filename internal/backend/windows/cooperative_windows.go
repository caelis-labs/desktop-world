//go:build windows && amd64

package windows

import (
	"context"
	"strings"
	"time"
	"unicode/utf16"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

// All transaction state and retained COM objects stay on the native MTA worker.
// SetForegroundWindow is deliberately used without Alt injection, thread input
// attachment or changes to the system's foreground lock policy.
type cooperative struct {
	*Driver
	active                     bool
	started                    time.Time
	previous, target           uintptr
	previousPID, targetPID     uint32
	previousStart, targetStart uint64
	previousFocus              *com
	pointer                    [2]int32
	lastPointer                *[2]int32
	borrowed                   bool
	fault                      error
	report                     dw.InputReport
}

func NewCooperative() backend.Driver {
	d := &cooperative{Driver: New().(*Driver)}
	d.Driver.inputGuard = d.guard
	return d
}
func (*cooperative) TargetsInput(op string) bool {
	return dw.ActionChannel(op) == "shared_input" || op == "focus"
}
func (*cooperative) InputChannel() string { return "foreground_transaction" }
func (d *cooperative) BeginInput(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if d.active {
		return dw.NewFault("input_transaction_scope", "transaction already active", "never_automatically")
	}
	d.active = true
	d.report = dw.InputReport{Mode: "cooperative", Restoration: "not_borrowed"}
	return nil
}
func currentWindowIdentity(hwnd uintptr) (uint32, uint64) {
	var pid uint32
	windowPID.Call(hwnd, ptr(&pid))
	return pid, processStart(pid)
}
func (d *cooperative) guard(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if d.fault != nil {
		return d.fault
	}
	if !d.active || d.started.IsZero() {
		return nil
	}
	hwnd, _, _ := foreground.Call()
	pid, start := currentWindowIdentity(hwnd)
	if pid != d.targetPID || start != d.targetStart {
		d.fault = dw.NewFault("user_interrupted", "foreground application changed during input", "reobserve")
	} else if time.Since(d.started) >= time.Second {
		d.fault = dw.NewFault("input_burst_limit", "one-second foreground input budget exhausted", "reobserve")
	}
	return d.fault
}
func (d *cooperative) Environment(ctx context.Context) (dw.Environment, error) {
	if err := d.guard(ctx); err != nil {
		return dw.Environment{}, err
	}
	v, err := d.Driver.Environment(ctx)
	v.InputMode = dw.InputModeCooperative
	v.Capabilities = append(v.Capabilities, dw.Capability{Name: "foreground_transaction", Support: "supported", Availability: "unknown", Reason: "Scoped SetForegroundWindow transactions; OS foreground restrictions and UIPI apply. One-second budget, 256 UTF-16 text units, 500ms drag; no independent input devices."})
	return v, err
}
func (d *cooperative) Query(ctx context.Context, q backend.Query) (backend.Page, error) {
	if err := d.guard(ctx); err != nil {
		return backend.Page{}, err
	}
	return d.Driver.Query(ctx, q)
}
func (d *cooperative) Read(ctx context.Context, k backend.Key) (backend.Node, backend.Seat, error) {
	if err := d.guard(ctx); err != nil {
		return backend.Node{}, backend.Seat{}, err
	}
	return d.Driver.Read(ctx, k)
}
func (d *cooperative) ReadText(ctx context.Context, k backend.Key) (backend.Text, error) {
	if err := d.guard(ctx); err != nil {
		return backend.Text{}, err
	}
	return d.Driver.ReadText(ctx, k)
}

func inputBurstError(s dw.Step) error {
	if s.TypeText != nil && len(utf16.Encode([]rune(s.TypeText.Text))) > 256 || s.Drag != nil && s.Drag.Duration > 500*time.Millisecond {
		return dw.NewFault("input_burst_limit", "split text above 256 UTF-16 units or drag above 500ms into short plans", "reobserve")
	}
	return nil
}
func (d *cooperative) activate(ctx context.Context, hwnd uintptr) error {
	if _, err := windowCaptureRect(hwnd); err != nil {
		return err
	}
	current, _, _ := foreground.Call()
	if current == hwnd {
		return nil
	}
	if ok, _, _ := setForeground.Call(hwnd); ok == 0 {
		return dw.NewFault("needs_user_focus", "Windows denied foreground activation; focus the target explicitly", "reobserve")
	}
	deadline := time.Now().Add(200 * time.Millisecond)
	for {
		current, _, _ = foreground.Call()
		if current == hwnd {
			return nil
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if time.Now().After(deadline) {
			return dw.NewFault("needs_user_focus", "foreground activation was not acknowledged", "reobserve")
		}
		time.Sleep(5 * time.Millisecond)
	}
}
func (d *cooperative) borrow(ctx context.Context, win *entry) error {
	if !d.active {
		return dw.NewFault("input_transaction_scope", "input requires an active plan", "never_automatically")
	}
	if err := d.guard(ctx); err != nil {
		return err
	}
	if !d.started.IsZero() {
		if d.targetPID != win.pid || d.targetStart != win.start {
			return dw.NewFault("input_transaction_scope", "one input plan cannot change application", "reobserve")
		}
		d.target = win.hwnd
		return d.activate(ctx, win.hwnd)
	}
	d.previous, _, _ = foreground.Call()
	d.previousPID, d.previousStart = currentWindowIdentity(d.previous)
	if d.previous == 0 || d.previousStart == 0 {
		return dw.NewFault("seat_unavailable", "no live foreground window", "reobserve")
	}
	if ok, _, _ := cursorPos.Call(ptr(&d.pointer)); ok == 0 {
		return dw.NewFault("seat_unavailable", "pointer position unavailable", "reobserve")
	}
	_ = d.uia.call(8, ptr(&d.previousFocus))
	d.started = time.Now()
	d.target, d.targetPID, d.targetStart = win.hwnd, win.pid, win.start
	err := d.activate(ctx, win.hwnd)
	current, _, _ := foreground.Call()
	d.borrowed = d.previous != win.hwnd && current == win.hwnd
	return err
}
func transactionOutcome(err error) backend.Outcome {
	if f, ok := err.(*dw.Fault); ok {
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: f}
	}
	return result(dw.DeliveryNone, "cancelled")
}
func (d *cooperative) Perform(ctx context.Context, op backend.Operation) backend.Outcome {
	restore := dpiScope()
	defer restore()
	if err := d.guard(ctx); err != nil {
		return transactionOutcome(err)
	}
	if !d.TargetsInput(op.Step.Op) {
		return d.Driver.Perform(ctx, op)
	}
	if err := inputBurstError(op.Step); err != nil {
		return transactionOutcome(err)
	}
	if err := heldInputError(); err != nil {
		return transactionOutcome(err)
	}
	e, err := d.lookup(op.Key)
	if err != nil {
		return transactionOutcome(err)
	}
	if e.application {
		return result(dw.DeliveryNone, "background_unavailable")
	}
	if e.hwnd == 0 {
		if err = d.refreshOwnership(e); err != nil {
			return transactionOutcome(err)
		}
	}
	win, err := d.lookup(e.window)
	if err != nil {
		return transactionOutcome(err)
	}
	if win.hwnd == 0 {
		return result(dw.DeliveryNone, "background_unavailable")
	}
	if err = d.borrow(ctx, win); err != nil {
		return transactionOutcome(err)
	}
	if strings.HasPrefix(op.Step.Op, "keyboard.") || op.Step.Op == "focus" {
		if e.hwnd == 0 {
			if err = d.focusElement(ctx, e.el); err != nil {
				return result(dw.DeliveryNone, "background_target_not_focused")
			}
		}
		var focus *com
		if err = d.uia.call(8, ptr(&focus)); err != nil || focus == nil {
			return result(dw.DeliveryNone, "background_target_not_focused")
		}
		defer focus.release()
		if e.hwnd == 0 {
			var same int32
			if d.uia.call(3, ptr(focus), ptr(e.el), ptr(&same)) != nil || same == 0 {
				return result(dw.DeliveryNone, "background_target_not_focused")
			}
		} else {
			// A Window target authorizes only focus inside this exact window.
			var native uintptr
			cur := focus
			_ = cur.call(1)
			for depth := 0; cur != nil && depth < 64; depth++ {
				_ = cur.call(36, ptr(&native))
				if native != 0 {
					break
				}
				var parent *com
				_ = d.walker.call(3, ptr(cur), ptr(&parent))
				cur.release()
				cur = parent
			}
			cur.release()
			root, _, _ := proc(user32, "GetAncestor").Call(native, 2)
			if root != win.hwnd {
				return result(dw.DeliveryNone, "background_target_not_focused")
			}
		}
		protected := boolProp(focus, 35)
		if protected.Status != dw.FactKnown {
			return result(dw.DeliveryNone, "state_unknown")
		}
		if isTrue(protected) {
			return result(dw.DeliveryNone, "protected_target")
		}
		if op.Step.Op == "focus" {
			return result(dw.DeliveryComplete, "")
		}
	}
	if strings.HasPrefix(op.Step.Op, "pointer.") {
		if op.Point == nil {
			return result(dw.DeliveryNone, "background_unavailable")
		}
		if err = d.hit(ctx, *op.Point, op.Key); err != nil {
			return transactionOutcome(err)
		}
		if op.Step.Drag != nil {
			to, er := d.lookup(op.ToKey)
			if er != nil || op.To == nil {
				return result(dw.DeliveryNone, "background_unavailable")
			}
			if er = d.refreshOwnership(to); er != nil {
				return transactionOutcome(er)
			}
			if to.window != win.key {
				return result(dw.DeliveryNone, "input_transaction_scope")
			}
			if er = d.hit(ctx, *op.To, op.ToKey); er != nil {
				return transactionOutcome(er)
			}
		}
	}
	if err = d.guard(ctx); err != nil {
		return transactionOutcome(err)
	}
	v := d.Driver.Perform(ctx, op)
	// Give the target a bounded opportunity to consume injected input before
	// restoring the foreground. This is not a second dispatch or verification.
	if v.Delivery != dw.DeliveryNone {
		timer := time.NewTimer(40 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
		case <-timer.C:
		}
	}
	if strings.HasPrefix(op.Step.Op, "pointer.") {
		p := [2]int32{}
		if ok, _, _ := cursorPos.Call(ptr(&p)); ok != 0 {
			d.lastPointer = &p
		}
	}
	return v
}
func (d *cooperative) focusElement(ctx context.Context, target *com) error {
	deadline := time.Now().Add(120 * time.Millisecond)
	requested := false
	for {
		if err := d.guard(ctx); err != nil {
			return err
		}
		var actual *com
		var same int32
		err := d.uia.call(8, ptr(&actual))
		if err == nil && actual != nil {
			err = d.uia.call(3, ptr(actual), ptr(target), ptr(&same))
		}
		actual.release()
		if err == nil && same != 0 {
			return nil
		}
		if !requested {
			requested = true
			// An already-focused editor must not receive a redundant focus
			// request between select-all, typing and its save shortcut.
			_ = target.call(3)
		}
		if time.Now().After(deadline) {
			return dw.NewFault("background_target_not_focused", "UIA focus acknowledgement did not converge", "reobserve")
		}
		time.Sleep(5 * time.Millisecond)
	}
}
func (d *cooperative) hit(ctx context.Context, point dw.Point, key backend.Key) error {
	deadline := time.Now().Add(120 * time.Millisecond)
	for {
		if err := d.guard(ctx); err != nil {
			return err
		}
		ok, err := d.Driver.HitTest(ctx, point, key)
		if err != nil {
			return err
		}
		if ok {
			return nil
		}
		if time.Now().After(deadline) {
			return dw.NewFault("target_not_hittable", "point does not hit the retained target", "reobserve")
		}
		time.Sleep(5 * time.Millisecond)
	}
}
func (d *cooperative) EndInput(ctx context.Context) (dw.InputReport, error) {
	restore := dpiScope()
	defer restore()
	if !d.active {
		return d.report, nil
	}
	d.active = false // Cleanup is independent of cancelled input/burst guards.
	defer func() {
		d.previousFocus.release()
		d.previousFocus = nil
		d.started = time.Time{}
		d.lastPointer, d.fault = nil, nil
		d.borrowed = false
	}()
	if d.started.IsZero() {
		return d.report, nil
	}
	current, _, _ := foreground.Call()
	pid, start := currentWindowIdentity(current)
	if !d.borrowed && current == d.previous {
		d.report.Restoration = "not_borrowed"
	} else if pid != d.targetPID || start != d.targetStart {
		d.report.Restoration = "user_superseded"
	} else if d.borrowed {
		p, s := currentWindowIdentity(d.previous)
		if p != d.previousPID || s != d.previousStart || d.activate(ctx, d.previous) != nil {
			d.report.Restoration = "failed"
		} else {
			d.report.Restoration = "restored"
			if d.previousFocus != nil {
				// Windows normally restores a window's last focus itself. Some
				// providers reject SetFocus on that already-focused element. First
				// confirm the actual focus; only request focus if it differs.
				if !d.focusRestored(ctx) {
					current, _, _ = foreground.Call()
					if current == d.previous && ctx.Err() == nil {
						_ = d.previousFocus.call(3)
					}
				}
				if !d.focusRestored(ctx) {
					current, _, _ = foreground.Call()
					if current != d.previous {
						d.report.Restoration = "user_superseded"
					} else {
						d.report.Restoration = "failed"
					}
				}
			}
		}
	}
	if d.lastPointer != nil && d.report.Restoration != "failed" && d.report.Restoration != "user_superseded" {
		var p [2]int32
		if ok, _, _ := cursorPos.Call(ptr(&p)); ok != 0 && p == *d.lastPointer {
			if ok, _, _ := proc(user32, "SetCursorPos").Call(uintptr(d.pointer[0]), uintptr(d.pointer[1])); ok == 0 {
				d.report.Restoration = "failed"
			}
		}
	}
	if d.borrowed {
		d.report.ForegroundMS = time.Since(d.started).Milliseconds()
	}
	return d.report, nil
}

// Foreground acknowledgement can precede UIA's focused-element update. Poll
// the retained identity for a bounded interval without reactivating or retrying
// any task input; a user's new foreground always wins.
func (d *cooperative) focusRestored(ctx context.Context) bool {
	deadline := time.Now().Add(200 * time.Millisecond)
	for {
		hwnd, _, _ := foreground.Call()
		if hwnd != d.previous || ctx.Err() != nil {
			return false
		}
		var actual *com
		var same int32
		err := d.uia.call(8, ptr(&actual))
		if err == nil && actual != nil {
			err = d.uia.call(3, ptr(actual), ptr(d.previousFocus), ptr(&same))
		}
		actual.release()
		if err == nil && same != 0 {
			return true
		}
		if time.Now().After(deadline) {
			return false
		}
		time.Sleep(5 * time.Millisecond)
	}
}
func (d *cooperative) Close(ctx context.Context) error {
	cleanup, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, err := d.EndInput(cleanup)
	closeErr := d.Driver.Close(ctx)
	if err != nil {
		return err
	}
	return closeErr
}
