//go:build !darwin && !windows

package main

import "fmt"

func cursorOverlayInit() error                 { return fmt.Errorf("cursor overlay unsupported on this platform") }
func cursorOverlayShow(float64, float64) error { return fmt.Errorf("cursor overlay unsupported") }
func cursorOverlayHide()                       {}
func cursorOverlayPoll()                       {}
func cursorOverlayClose()                      {}
