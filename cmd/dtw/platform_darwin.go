//go:build darwin

package main

import (
	"context"
	"os"

	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/ipc/host"
)

// macOS owns its native helper route and cursor process. Other platforms add
// their implementation in their own platform file.
func startNativePlatform(ctx context.Context, helper, assetsDir string) (*host.Client, func(), error) {
	options := host.Options{Executable: helper, AssetsDir: assetsDir, InputMode: dw.InputModeCooperative}
	var cursor *virtualCursorOverlay
	if os.Getenv("DTW_TEST_VIRTUAL") == "1" || os.Getenv("DTW_TEST_CHILD") != "1" && os.Getenv("DTW_TEST_VIRTUAL") != "0" {
		cursor = newVirtualCursorOverlay(helper)
		options.Stderr = cursor
	} else if os.Getenv("DTW_TEST_NATIVE_TRACE") == "1" {
		options.Stderr = os.Stderr
	}
	native, err := host.Start(ctx, options)
	if err != nil {
		if cursor != nil {
			cursor.Close()
		}
		return nil, nil, err
	}
	return native, func() {
		native.Close()
		if cursor != nil {
			cursor.Close()
		}
	}, nil
}
