//go:build dtw_poc_noauth

package helper

import (
	"context"
	"testing"
)

func TestPOCNoGrantGateRequiresTaggedManagedHelper(t *testing.T) {
	if !pocCoreWithoutGrants(Config{Managed: true}) {
		t.Fatal("tagged managed POC helper still has the DTW grant gate")
	}
	if pocCoreWithoutGrants(Config{Managed: false}) {
		t.Fatal("tagged unmanaged helper unexpectedly bypasses its startup scope")
	}
	s, _ := setup(t, Config{Managed: true})
	if out := s.Control(context.Background(), ControlRequest{ID: "grant", Op: "grant", Turn: "session"}); out.Error == nil {
		t.Fatal("tagged core helper accepted a DTW grant")
	}
	if out := s.Control(context.Background(), ControlRequest{ID: "begin", Op: "begin_turn", Turn: "session"}); out.Error != nil {
		t.Fatalf("core helper lost Session lifecycle: %v", out.Error)
	}
}
