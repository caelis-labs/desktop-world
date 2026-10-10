//go:build dtw_poc_noauth

package helper

import "testing"

func TestPOCNoGrantGateRequiresTaggedManagedHelper(t *testing.T) {
	if !pocCoreWithoutGrants(Config{Managed: true}) {
		t.Fatal("tagged managed POC helper still has the DTW grant gate")
	}
	if pocCoreWithoutGrants(Config{Managed: false}) {
		t.Fatal("tagged unmanaged helper unexpectedly bypasses its startup scope")
	}
}
