//go:build dtw_background_poc && darwin && cgo

package local

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend/darwin"
	"github.com/caelis-labs/desktop-world/internal/engine"
)

// OpenBackgroundPOC is experimental and absent from ordinary builds.
func OpenBackgroundPOC(ctx context.Context, o Options, mode string) (dw.World, error) {
	if err := o.InputMode.Validate(); err != nil {
		return nil, err
	}
	if o.InputMode.Effective() != dw.InputModeShared {
		return nil, dw.Invalid("choose one input mode")
	}
	if mode != "public_pid" && mode != "skylight" && mode != "no_raise" {
		return nil, dw.Invalid("unknown experimental input mode")
	}
	return engine.Open(ctx, darwin.NewBackgroundPOC(mode), engine.Options{HistoryLimit: o.HistoryLimit, RequestLimit: o.RequestLimit, ObjectLimit: o.ObjectLimit, ViewLimit: o.ViewLimit})
}
