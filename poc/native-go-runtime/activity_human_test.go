//go:build darwin

package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
	"time"
)

// A physical-input probe only. It acquires the same cross-process foreground
// coordinator as a pointer action, but never dispatches that action to native
// OS input. Use only with the exact owned Human fixture log and an operator.
func TestOwnedHumanInputDefersForegroundCoordinator(t *testing.T) {
	path := os.Getenv("DTW_POC_HUMAN_LOG")
	ready := os.Getenv("DTW_POC_HUMAN_READY")
	if path == "" || ready == "" {
		t.Skip("requires exact owned Human fixture log and ready path")
	}
	countKeys := func() (int, error) {
		data, err := os.ReadFile(path)
		if err != nil {
			return 0, err
		}
		count := 0
		for _, line := range strings.Split(string(data), "\n") {
			if line == "" {
				continue
			}
			var row struct {
				Event     string `json:"event"`
				EventType int    `json:"event_type"`
			}
			if json.Unmarshal([]byte(line), &row) == nil && row.Event == "window_event" && row.EventType == 10 {
				count++
			}
		}
		return count, nil
	}
	baseline, err := countKeys()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(ready, []byte("ready\n"), 0600); err != nil {
		t.Fatal(err)
	}
	defer os.Remove(ready)
	deadline := time.Now().Add(20 * time.Second)
	for time.Now().Before(deadline) {
		got, err := countKeys()
		if err != nil {
			t.Fatal(err)
		}
		if got > baseline {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}
	got, err := countKeys()
	if err != nil || got <= baseline {
		t.Fatalf("no new owned Human key event: baseline=%d now=%d err=%v", baseline, got, err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	s := &supervisor{}
	start := time.Now()
	release, route, err := s.coordinateAction(ctx, "human-input-gate-r2", json.RawMessage(`{"steps":[{"op":"pointer.click","target":{"ref":"owned-probe-not-dispatched"}}]}`))
	elapsed := time.Since(start)
	if release != nil {
		release()
	}
	finalKeys, _ := countKeys()
	t.Logf("route=%s wait_ms=%d user_active=%v keydown_count_before=%d after=%d error=%v; no native action dispatched", route, elapsed.Milliseconds(), errors.Is(err, context.DeadlineExceeded) || err != nil && strings.Contains(err.Error(), "user_active"), got, finalKeys, err)
}
