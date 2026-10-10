//go:build (!darwin && !windows) || (darwin && !cgo) || (darwin && cgo && (!dtw_background_poc || !dtw_virtual_input_poc))

package main

import (
	"context"
	"errors"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
)

func openNativeWorld(context.Context, local.Options) (dw.World, error) {
	return nil, errors.New("native DTW route is unavailable for this build")
}
