//go:build windows && amd64

package local

import (
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/backend/windows"
)

func native(mode dw.InputMode) (backend.Driver, error) {
	if mode == dw.InputModeCooperative {
		return windows.NewCooperative(), nil
	}
	return windows.New(), nil
}
