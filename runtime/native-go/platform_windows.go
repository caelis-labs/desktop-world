//go:build windows

package main

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/caelis-labs/desktop-world/host"
)

// Windows has an explicit route and coordinator boundary. The implementation
// can be supplied here without changing Darwin's native bridge or seat locks.
func startNativePlatform(context.Context, string, string) (*host.Client, func(), error) {
	return nil, nil, errors.New("Windows DTW backend has not passed native validation")
}

func (s *supervisor) coordinateAction(context.Context, string, json.RawMessage) (func(), string, error) {
	return nil, "", errors.New("Windows DTW action coordination has not passed native validation")
}

func waitForUserInputQuiet(context.Context, time.Duration, time.Duration) error {
	return errors.New("Windows DTW input activity gate has not passed native validation")
}
