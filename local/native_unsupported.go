//go:build (!darwin && !windows) || (darwin && !cgo)

package local

import (
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

func native(mode dw.InputMode) (backend.Driver, error) {
	return nil, dw.NewFault("platform_unsupported", "native backend requires macOS 14+ with cgo or Windows 11 amd64", "never_automatically")
}
