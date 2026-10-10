//go:build darwin && cgo

package local

import (
	"context"
	"github.com/caelis-labs/desktop-world/internal/engine"
	"github.com/caelis-labs/desktop-world/internal/platform/darwin"
	dw "github.com/caelis-labs/desktop-world/internal/world"
)

func OpenAutomaticInput(ctx context.Context, o Options) (dw.World, error) {
	if err := o.InputMode.Validate(); err != nil {
		return nil, err
	}
	return engine.Open(ctx, darwin.NewAutomaticInput(), engine.Options{HistoryLimit: o.HistoryLimit, RequestLimit: o.RequestLimit, ObjectLimit: o.ObjectLimit, ViewLimit: o.ViewLimit})
}
