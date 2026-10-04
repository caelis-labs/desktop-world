package engine_test

import (
	"context"
	"errors"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"sync/atomic"
	"testing"
	"time"
)

type transactionDriver struct {
	*occludedDriver
	begins, ends     atomic.Int32
	restore          string
	cleanupError     bool
	started, release chan struct{}
}

func (*transactionDriver) TargetsInput(op string) bool {
	return dw.ActionChannel(op) == "shared_input" || op == "focus"
}
func (*transactionDriver) InputChannel() string               { return "foreground_transaction" }
func (d *transactionDriver) BeginInput(context.Context) error { d.begins.Add(1); return nil }
func (d *transactionDriver) EndInput(context.Context) (dw.InputReport, error) {
	d.ends.Add(1)
	if d.cleanupError {
		return dw.InputReport{}, errors.New("cleanup failed")
	}
	return dw.InputReport{Mode: "cooperative", ForegroundMS: 100, Restoration: d.restore}, nil
}
func (d *transactionDriver) Perform(ctx context.Context, op backend.Operation) backend.Outcome {
	if d.started != nil {
		close(d.started)
		<-d.release
		d.started = nil
	}
	return d.Fixture.Perform(ctx, op)
}
func TestInputTransactionPolicyCleanupAndReconciliation(t *testing.T) {
	for _, restore := range []string{"restored", "user_superseded", "failed"} {
		t.Run(restore, func(t *testing.T) {
			var driver *transactionDriver
			w, f, epoch, refs := pocWorldDriver(t, func(base *occludedDriver) backend.Driver {
				driver = &transactionDriver{occludedDriver: base, restore: restore}
				return driver
			})
			ctx := context.Background()
			config := dw.ActorConfig{ID: "strict", InputPolicy: dw.InputNoShared, ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Refs: []dw.Ref{refs["Desktop World Fixture"]}}}, Operations: []string{"set_value", "pointer.click"}}
			actor, _ := w.NewActor(ctx, config)
			plan := dw.Plan{Epoch: epoch, RequestID: dw.RequestID(string(epoch) + ":strict"), Steps: []dw.Step{{ID: "write", Op: "set_value", Target: dw.Target{Ref: refs["内容"]}, SetValue: &dw.SetValue{Text: "must not happen"}}, {ID: "click", Op: "pointer.click", Target: dw.Target{Ref: refs["提交"]}, Click: &dw.Click{Button: "left", Count: 1}}}}
			if _, err := actor.Execute(ctx, plan); faultCode(err) != "requires_shared_input" || driver.begins.Load() != 0 || driver.ends.Load() != 0 || len(f.Events()) != 0 {
				t.Fatal(err)
			}
			config.ID = "cooperative"
			config.InputPolicy = dw.InputShared
			actor, _ = w.NewActor(ctx, config)
			plan.RequestID = dw.RequestID(string(epoch) + ":transaction")
			receipt, err := actor.Execute(ctx, plan)
			if restore == "failed" {
				if faultCode(err) != "input_restoration_failed" || receipt.Outcome != "unknown" || receipt.SeatHealth != "fenced" {
					t.Fatal(receipt, err)
				}
			} else if err != nil {
				t.Fatal(err)
			}
			if driver.begins.Load() != 1 || driver.ends.Load() != 1 || receipt.Input == nil || receipt.Input.Restoration != restore || receipt.Steps[1].Channel != "foreground_transaction" {
				t.Fatal(receipt)
			}
			count := len(f.Events())
			again, _ := actor.Execute(ctx, plan)
			if again.RunID != receipt.RunID || len(f.Events()) != count || driver.ends.Load() != 1 {
				t.Fatal("transaction replayed")
			}
		})
	}
}
func TestLateInputRestoresBeforeReleasingSeat(t *testing.T) {
	var driver *transactionDriver
	w, _, epoch, refs := pocWorldDriver(t, func(base *occludedDriver) backend.Driver {
		driver = &transactionDriver{occludedDriver: base, restore: "restored", started: make(chan struct{}), release: make(chan struct{})}
		return driver
	})
	ctx := context.Background()
	a, _ := w.NewActor(ctx, dw.ActorConfig{ID: "late", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"pointer.click"}})
	p := dw.Plan{Epoch: epoch, RequestID: dw.RequestID(string(epoch) + ":late"), Steps: []dw.Step{{ID: "click", Op: "pointer.click", Target: dw.Target{Ref: refs["提交"]}, Click: &dw.Click{Button: "left", Count: 1}, Timeout: 50 * time.Millisecond}}}
	r, err := a.Execute(ctx, p)
	if err == nil || r.Outcome != "unknown" || r.SeatHealth != "fenced" || driver.ends.Load() != 0 {
		t.Fatal(r, err)
	}
	close(driver.release)
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		r, _ = a.GetReceipt(ctx, r.RunID)
		if r.Input != nil {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if r.Input == nil || r.Input.Restoration != "restored" || r.SeatHealth != "ready" || r.Outcome != "unknown" || driver.ends.Load() != 1 {
		t.Fatal(r)
	}
}

func TestInputCleanupFailureFencesWithoutReplay(t *testing.T) {
	var driver *transactionDriver
	w, f, epoch, refs := pocWorldDriver(t, func(base *occludedDriver) backend.Driver {
		driver = &transactionDriver{occludedDriver: base, cleanupError: true}
		return driver
	})
	ctx := context.Background()
	a, _ := w.NewActor(ctx, dw.ActorConfig{ID: "cleanup", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"pointer.click"}})
	p := dw.Plan{Epoch: epoch, RequestID: dw.RequestID(string(epoch) + ":cleanup"), Steps: []dw.Step{{ID: "click", Op: "pointer.click", Target: dw.Target{Ref: refs["提交"]}, Click: &dw.Click{Button: "left", Count: 1}}}}
	r, err := a.Execute(ctx, p)
	if faultCode(err) != "input_restoration_failed" || r.Outcome != "unknown" || r.SeatHealth != "fenced" || len(f.Events()) != 1 {
		t.Fatal(r, err)
	}
	again, _ := a.Execute(ctx, p)
	if again.RunID != r.RunID || len(f.Events()) != 1 || driver.ends.Load() != 1 {
		t.Fatal("cleanup failure replayed")
	}
	p.RequestID += "-new"
	if _, err = a.Execute(ctx, p); faultCode(err) != "seat_fenced" || len(f.Events()) != 1 {
		t.Fatal(err)
	}
}

func TestCooperativeDragCannotCrossWindow(t *testing.T) {
	w, f, epoch, refs := pocWorldDriver(t, func(base *occludedDriver) backend.Driver {
		return &transactionDriver{occludedDriver: base, restore: "not_borrowed"}
	})
	ctx := context.Background()
	a, _ := w.NewActor(ctx, dw.ActorConfig{ID: "drag", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"pointer.drag"}})
	p := dw.Plan{Epoch: epoch, RequestID: dw.RequestID(string(epoch) + ":cross-window"), Steps: []dw.Step{{ID: "drag", Op: "pointer.drag", Target: dw.Target{Ref: refs["内容"]}, Drag: &dw.Drag{To: dw.Target{Ref: refs["human field"]}, Duration: 100 * time.Millisecond}}}}
	if _, err := a.Execute(ctx, p); faultCode(err) != "background_unavailable" || len(f.Events()) != 0 {
		t.Fatal(err, f.Events())
	}
}
