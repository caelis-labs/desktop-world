//go:build darwin && cgo && dtw_background_poc && dtw_virtual_input_poc

package main

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
)

func openNativeWorld(ctx context.Context, options local.Options) (dw.World, error) {
	return local.OpenVirtualInputPOC(ctx, options)
}
