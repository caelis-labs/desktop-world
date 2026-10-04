//go:build dtw_background_poc && darwin && cgo

package main

import (
	"context"
	"flag"
	"github.com/caelis-labs/desktop-world/local"
	"testing"
)

func TestBackgroundPOCInvalidModeFailsBeforeOpeningDesktop(t *testing.T) {
	f := flag.NewFlagSet("serve", flag.ContinueOnError)
	open := backgroundPOCFlag(f)
	if err := f.Parse([]string{"--experimental-background-input", "invalid"}); err != nil {
		t.Fatal(err)
	}
	if _, err := open(context.Background(), local.Options{}); err == nil {
		t.Fatal("invalid input mode accepted")
	}
}
