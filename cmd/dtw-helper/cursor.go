package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"runtime"
	"time"
)

type cursorCommand struct {
	Op    string `json:"op"`
	Point struct {
		Frame string  `json:"frame"`
		X     float64 `json:"x"`
		Y     float64 `json:"y"`
	} `json:"point"`
}

// The overlay process has no desktop authorization and never posts input.
// EOF and all normal shutdown paths remove its nonactivating window.
func runCursorOverlay() error {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	if err := cursorOverlayInit(); err != nil {
		return err
	}
	defer cursorOverlayClose()
	commands := make(chan cursorCommand, 8)
	var lastShow time.Time
	visible := false
	go func() {
		defer close(commands)
		scan := bufio.NewScanner(os.Stdin)
		scan.Buffer(make([]byte, 256), 4096)
		for scan.Scan() {
			var command cursorCommand
			if json.Unmarshal(scan.Bytes(), &command) == nil {
				select {
				case commands <- command:
				default:
				}
			}
		}
	}()
	for {
		select {
		case command, ok := <-commands:
			if !ok {
				return nil
			}
			switch command.Op {
			case "hide":
				cursorOverlayHide()
				visible = false
			case "show":
				p := command.Point
				if p.Frame != "desktop" || math.IsNaN(p.X) || math.IsNaN(p.Y) || math.IsInf(p.X, 0) || math.IsInf(p.Y, 0) {
					cursorOverlayHide()
					visible = false
					continue
				}
				if err := cursorOverlayShow(p.X, p.Y); err != nil {
					fmt.Fprintln(os.Stderr, "cursor overlay:", err)
					cursorOverlayHide()
					visible = false
				} else {
					lastShow = time.Now()
					visible = true
				}
			}
		default:
			if visible && time.Since(lastShow) >= 5*time.Second {
				cursorOverlayHide()
				visible = false
			}
			cursorOverlayPoll()
			time.Sleep(16 * time.Millisecond)
		}
	}
}
