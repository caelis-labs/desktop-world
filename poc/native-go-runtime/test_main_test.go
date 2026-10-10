package main

import (
	"os"
	"testing"
)

func TestMain(m *testing.M) {
	// Historical protocol tests assert the older structured projection. The
	// standalone POC server now defaults to one concise text result.
	if os.Getenv("DTW_POC_LEGACY_OUTPUT") == "" {
		_ = os.Setenv("DTW_POC_LEGACY_OUTPUT", "1")
	}
	os.Exit(m.Run())
}
