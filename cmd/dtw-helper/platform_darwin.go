//go:build darwin && cgo

package main

import (
	"context"
	"github.com/caelis-labs/desktop-world/internal/platform"
	dw "github.com/caelis-labs/desktop-world/internal/world"
)

func openNativeWorld(ctx context.Context, options local.Options) (dw.World, error) {
	return local.OpenAutomaticInput(ctx, options)
}
