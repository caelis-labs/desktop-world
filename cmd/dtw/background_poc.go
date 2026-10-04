//go:build dtw_background_poc && darwin && cgo

package main

import (
	"context"
	"flag"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
)

func backgroundPOCFlag(f *flag.FlagSet) funcOpen {
	mode := f.String("experimental-background-input", "", "POC only: public_pid, skylight or no_raise; no automatic foreground/HID fallback")
	return func(ctx context.Context, o local.Options) (dw.World, error) {
		if *mode != "" && o.InputMode.Effective() != dw.InputModeShared {
			return nil, fmt.Errorf("choose one input mode")
		}
		if *mode == "" {
			return local.Open(ctx, o)
		}
		if *mode != "public_pid" && *mode != "skylight" && *mode != "no_raise" {
			return nil, fmt.Errorf("unknown experimental input mode")
		}
		return local.OpenBackgroundPOC(ctx, o, *mode)
	}
}
