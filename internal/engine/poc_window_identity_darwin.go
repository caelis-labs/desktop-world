//go:build darwin && cgo && dtw_poc_exactgrant

package engine

import (
	"context"
	"errors"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

// POCWindowIdentity is a trusted, same-world Ref join for the isolated helper.
// No title, geometry or script-supplied native ID participates in the join.
type POCWindowIdentity struct {
	PID            int32
	StartSec       int64
	StartUSec      int64
	NativeWindowID uint32
	Application    dw.Ref
	Window         dw.Ref
}

func (w *World) POCWindowIdentity(ctx context.Context, ref dw.Ref) (POCWindowIdentity, error) {
	w.mu.Lock()
	rec := w.objects[ref]
	var key backend.Key
	if rec != nil {
		key = rec.key
	}
	w.mu.Unlock()
	if key == "" {
		return POCWindowIdentity{}, errors.New("unobserved target Ref")
	}
	driver, ok := w.driver.(interface {
		POCWindowIdentity(context.Context, backend.Key) (backend.POCWindowIdentity, error)
	})
	if !ok {
		return POCWindowIdentity{}, errors.New("native window identity unavailable")
	}
	value, err := w.call(ctx, func() (any, error) { return driver.POCWindowIdentity(ctx, key) })
	if err != nil {
		return POCWindowIdentity{}, err
	}
	native := value.(backend.POCWindowIdentity)
	w.mu.Lock()
	defer w.mu.Unlock()
	app, window := w.keys[native.AppKey], w.keys[native.WindowKey]
	current := w.objects[ref]
	if app == "" || window == "" || current == nil || current.key != key ||
		current.object.App != app || (current.object.Window != "" && current.object.Window != window) {
		return POCWindowIdentity{}, errors.New("native/AX Ref join unresolved")
	}
	return POCWindowIdentity{PID: native.PID, StartSec: native.StartSec,
		StartUSec: native.StartUSec, NativeWindowID: native.NativeWindowID,
		Application: app, Window: window}, nil
}
