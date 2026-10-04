//go:build windows && amd64

package local

import (
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/backend/windows"
)

func native(mode dw.InputMode) (backend.Driver, error) {
	if mode != dw.InputModeShared {
		return nil, dw.NewFault("capability_unavailable", "cooperative input is currently macOS only", "never_automatically")
	}
	return windows.New(), nil
}
