//go:build windows

package main

import (
	"context"
	"errors"
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/platform"
)

// Windows receives its own world and route implementation here. Until it is
// validated on Windows, startup fails before any desktop action is dispatched.
func openNativeWorld(context.Context, local.Options) (dw.World, error) {
	return nil, errors.New("Windows native DTW route is not yet validated")
}
