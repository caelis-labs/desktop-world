//go:build dtw_background_poc && dtw_virtual_input_poc && darwin && cgo

package main

import (
	"context"
	"flag"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
)

// The isolated POC has one automatic route. Scripts and MCP callers do not
// choose a transport or a foreground mode.
func backgroundPOCFlag(*flag.FlagSet) funcOpen {
	return func(ctx context.Context, o local.Options) (dw.World, error) {
		return local.OpenVirtualInputPOC(ctx, o)
	}
}
