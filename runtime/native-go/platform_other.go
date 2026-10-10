//go:build !darwin && !windows

package main

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/caelis-labs/desktop-world/host"
)

func startNativePlatform(context.Context, string, string) (*host.Client, func(), error) {
	return nil, nil, errors.New("native DTW backend is unavailable on this platform")
}

func (s *supervisor) coordinateAction(context.Context, string, json.RawMessage) (func(), string, error) {
	return nil, "", errors.New("native DTW action coordination is unavailable on this platform")
}

func waitForUserInputQuiet(context.Context, time.Duration, time.Duration) error {
	return errors.New("native DTW input activity gate is unavailable on this platform")
}
