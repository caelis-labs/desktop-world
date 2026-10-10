//go:build dtw_background_poc && dtw_virtual_input_poc && darwin && cgo

package local

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend/darwin"
	"github.com/caelis-labs/desktop-world/internal/engine"
)

func OpenVirtualInputPOC(ctx context.Context, o Options) (dw.World, error) {
	if err := o.InputMode.Validate(); err != nil {
		return nil, err
	}
	return engine.Open(ctx, darwin.NewVirtualInputPOC(), engine.Options{HistoryLimit: o.HistoryLimit, RequestLimit: o.RequestLimit, ObjectLimit: o.ObjectLimit, ViewLimit: o.ViewLimit})
}
