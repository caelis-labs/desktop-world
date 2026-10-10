//go:build windows && !amd64

package local

import (
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

func native(mode dw.InputMode) (backend.Driver, error) {
	return nil, dw.NewFault("platform_unsupported", "Windows native backend currently requires amd64", "never_automatically")
}
