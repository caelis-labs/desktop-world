//go:build dtw_background_poc && dtw_virtual_input_poc && darwin && cgo

package main

import (
	"flag"
	"io"
	"testing"
)

func TestVirtualPOCHasNoTransportSelector(t *testing.T) {
	f := flag.NewFlagSet("serve", flag.ContinueOnError)
	f.SetOutput(io.Discard)
	backgroundPOCFlag(f)
	if err := f.Parse([]string{"--experimental-background-input", "public_pid"}); err == nil {
		t.Fatal("automatic POC exposed a transport selector")
	}
}
