//go:build darwin && cgo

package local

import (
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/backend/darwin"
)

func native() (backend.Driver, error) { return darwin.New(), nil }
