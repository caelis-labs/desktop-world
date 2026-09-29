// Package local connects Desktop World to the current interactive desktop.
// Open probes permissions without prompting and does not own the host UI loop.
package local

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/engine"
)

// Options bounds in-memory retention. Zero values select conservative defaults.
type Options struct{ HistoryLimit, RequestLimit, ObjectLimit, ViewLimit int }

func Open(ctx context.Context, o Options) (dw.World, error) {
	d, e := native()
	if e != nil {
		return nil, e
	}
	return engine.Open(ctx, d, engine.Options{HistoryLimit: o.HistoryLimit, RequestLimit: o.RequestLimit, ObjectLimit: o.ObjectLimit, ViewLimit: o.ViewLimit})
}
