//go:build windows && amd64

package local

import (
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/backend/windows"
)

func native() (backend.Driver, error) { return windows.New(), nil }
