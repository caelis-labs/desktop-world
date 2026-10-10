//go:build dtw_background_poc && dtw_virtual_input_poc && darwin && cgo

package darwin

/*
#cgo CFLAGS: -DDTW_VIRTUAL_INPUT_POC
#cgo LDFLAGS: -framework ApplicationServices
#include <ApplicationServices/ApplicationServices.h>
static double dtwSecondsSincePhysicalInput(void) {
  CGEventType kinds[] = {kCGEventKeyDown, kCGEventLeftMouseDown,
                         kCGEventRightMouseDown, kCGEventOtherMouseDown,
                         kCGEventMouseMoved, kCGEventScrollWheel};
  double youngest = 1e9;
  for (int i = 0; i < 6; i++) {
    double age = CGEventSourceSecondsSinceLastEventType(
        kCGEventSourceStateCombinedSessionState, kinds[i]);
    if (age >= 0 && age < youngest) youngest = age;
  }
  return youngest;
}
*/
import "C"

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
	"sync"
	"syscall"
	"time"
)

type virtualInputPOC struct {
	*cooperative
	background        *backgroundPOC
	mu                sync.Mutex
	foregroundRelease func()
}

func NewVirtualInputPOC() backend.Driver {
	base := &Driver{}
	return &virtualInputPOC{cooperative: &cooperative{base}, background: &backgroundPOC{Driver: base, mode: "skylight"}}
}

func (d *virtualInputPOC) InputChannel() string { return "automatic_input" }

func (d *virtualInputPOC) Perform(ctx context.Context, op backend.Operation) backend.Outcome {
	if !d.TargetsInput(op.Step.Op) {
		return d.Driver.Perform(ctx, op)
	}
	if d.background.TargetsInput(op.Step.Op) {
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
	if err := d.borrowForeground(ctx); err != nil {
		code := "coordination_unavailable"
		if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			code = "cancelled"
		} else if errors.Is(err, errUserActive) {
			code = "user_active"
		}
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault(code, err.Error(), "never_automatically")}
	}
	out := d.cooperative.Perform(ctx, op)
	out.Channel = "targeted_foreground"
	return out
}

func (d *virtualInputPOC) EndInput(ctx context.Context) (dw.InputReport, error) {
	defer d.releaseForeground()
	return d.cooperative.EndInput(ctx)
}

func (d *virtualInputPOC) Close(ctx context.Context) error {
	defer d.releaseForeground()
	return d.cooperative.Close(ctx)
}

func (d *virtualInputPOC) borrowForeground(ctx context.Context) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.foregroundRelease != nil {
		return nil // One native plan keeps one lease through its final restoration.
	}
	traceForegroundLease("front_waiting")
	release, err := acquireForegroundFile(ctx)
	if err != nil {
		return err
	}
	traceForegroundLease("front_acquired")
	if err := waitForPhysicalInputQuiet(ctx, 600*time.Millisecond, 3*time.Second); err != nil {
		traceForegroundLease("front_released")
		release()
		return err
	}
	d.foregroundRelease = func() {
		traceForegroundLease("front_released")
		release()
	}
	return nil
}

func traceForegroundLease(event string) {
	path := os.Getenv("DTW_POC_COORD_TRACE")
	if path == "" {
		return
	}
	file, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer file.Close()
	row, _ := json.Marshal(map[string]any{"event": event, "pid": os.Getpid(), "time": time.Now().UnixNano()})
	_, _ = file.Write(append(row, '\n'))
}

func (d *virtualInputPOC) releaseForeground() {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.foregroundRelease != nil {
		d.foregroundRelease()
		d.foregroundRelease = nil
	}
}

var errUserActive = errors.New("user_active: physical input did not become quiet before the foreground borrow deadline")

func waitForPhysicalInputQuiet(ctx context.Context, quiet, limit time.Duration) error {
	deadline := time.Now().Add(limit)
	for {
		age := time.Duration(float64(C.dtwSecondsSincePhysicalInput()) * float64(time.Second))
		if age >= quiet {
			return nil
		}
		if time.Now().After(deadline) {
			return errUserActive
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(20 * time.Millisecond):
		}
	}
}

func acquireForegroundFile(ctx context.Context) (func(), error) {
	root := os.Getenv("DTW_POC_COORD_DIR")
	if root == "" {
		root = filepath.Join(os.TempDir(), fmt.Sprintf("dtw-native-go-poc-%d", os.Getuid()))
	}
	if err := os.Mkdir(root, 0700); err != nil && !os.IsExist(err) {
		return nil, err
	}
	info, err := os.Lstat(root)
	if err != nil {
		return nil, err
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || !info.IsDir() || info.Mode().Perm() != 0700 || stat.Uid != uint32(os.Getuid()) {
		return nil, errors.New("coordination directory must be a private owned directory")
	}
	fd, err := unix.Open(filepath.Join(root, "foreground.lock"), unix.O_CREAT|unix.O_RDWR|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0600)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(fd), "foreground.lock")
	for {
		if err := unix.Flock(fd, unix.LOCK_EX|unix.LOCK_NB); err == nil {
			return func() { _ = unix.Flock(fd, unix.LOCK_UN); _ = file.Close() }, nil
		} else if err != unix.EWOULDBLOCK && err != unix.EAGAIN {
			_ = file.Close()
			return nil, err
		}
		select {
		case <-ctx.Done():
			_ = file.Close()
			return nil, ctx.Err()
		case <-time.After(10 * time.Millisecond):
		}
	}
}
