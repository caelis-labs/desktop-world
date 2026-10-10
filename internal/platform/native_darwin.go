//go:build darwin && cgo

package local

import (
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/platform/darwin"
)

func native(mode dw.InputMode) (backend.Driver, error) {
	if mode == dw.InputModeCooperative {
		return darwin.NewCooperative(), nil
	}
	return darwin.New(), nil
}
