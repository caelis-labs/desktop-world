//go:build darwin && cgo && dtw_poc_exactgrant

package darwin

/*
#cgo CFLAGS: -DDTW_POC_EXACTGRANT
*/
import "C"

import (
	"context"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

// POCWindowIdentity resolves an engine-owned key inside the same native worker
// that executes actions. A new process or window cannot inherit this key.
func (d *Driver) POCWindowIdentity(ctx context.Context, key backend.Key) (v backend.POCWindowIdentity, err error) {
	err = d.call(ctx, "poc_window_identity", map[string]any{"Key": key}, &v)
	return
}
