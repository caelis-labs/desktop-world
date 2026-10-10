//go:build darwin

package main

/*
#cgo LDFLAGS: -framework ApplicationServices
#include <ApplicationServices/ApplicationServices.h>
static double dtwSecondsSinceSessionInput(void) {
  CGEventType types[] = {kCGEventKeyDown, kCGEventLeftMouseDown,
                         kCGEventRightMouseDown, kCGEventOtherMouseDown,
                         kCGEventMouseMoved, kCGEventScrollWheel};
  double age = 1e9;
  for (int i = 0; i < 6; ++i) {
    double sample = CGEventSourceSecondsSinceLastEventType(
        kCGEventSourceStateCombinedSessionState, types[i]);
    if (sample >= 0 && sample < age) age = sample;
  }
  return age;
}
*/
import "C"

import (
	"context"
	"errors"
	"time"
)

// Reads only event age. No key code, text, or other application's UI is read.
func waitForUserInputQuiet(ctx context.Context, quiet, limit time.Duration) error {
	deadline := time.Now().Add(limit)
	for {
		if time.Duration(float64(C.dtwSecondsSinceSessionInput())*float64(time.Second)) >= quiet {
			return nil
		}
		if time.Now().After(deadline) {
			return errors.New("user_active: physical input did not become quiet before the foreground borrow deadline")
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(20 * time.Millisecond):
		}
	}
}
