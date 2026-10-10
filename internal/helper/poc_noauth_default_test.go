//go:build !dtw_poc_noauth

package helper

import "testing"

func TestNormalHelperKeepsGrantGate(t *testing.T) {
	if pocCoreWithoutGrants(Config{Managed: true}) {
		t.Fatal("normal managed helper must retain its DTW grant gate")
	}
}
