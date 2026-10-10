//go:build (!darwin && !windows) || (darwin && !cgo)

package main

import (
	"context"
	"errors"
	"github.com/caelis-labs/desktop-world/internal/platform"
	dw "github.com/caelis-labs/desktop-world/internal/world"
)

func openNativeWorld(context.Context, local.Options) (dw.World, error) {
	return nil, errors.New("native DTW route is unavailable for this build")
}
