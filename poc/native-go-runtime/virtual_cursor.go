package main

import (
	"bytes"
	"encoding/json"
	"io"
	"math"
	"os"
	"os/exec"
	"sync"
)

// virtualCursorOverlay consumes only native POC point metadata. The process
// paints a click-through cursor and has no input-posting or desktop grants.
type virtualCursorOverlay struct {
	mu     sync.Mutex
	helper string
	buffer []byte
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	trace  *os.File
}

func newVirtualCursorOverlay(helper string) *virtualCursorOverlay {
	v := &virtualCursorOverlay{helper: helper}
	if path := os.Getenv("DTW_POC_CURSOR_TRACE"); path != "" {
		v.trace, _ = os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	}
	return v
}

func (v *virtualCursorOverlay) Write(p []byte) (int, error) {
	v.mu.Lock()
	defer v.mu.Unlock()
	v.buffer = append(v.buffer, p...)
	for {
		index := bytes.IndexByte(v.buffer, '\n')
		if index < 0 {
			break
		}
		line := append([]byte(nil), v.buffer[:index]...)
		v.buffer = v.buffer[index+1:]
		var event struct {
			POC string  `json:"poc"`
			X   float64 `json:"x"`
			Y   float64 `json:"y"`
		}
		if json.Unmarshal(line, &event) != nil || event.POC != "virtual_pointer" ||
			math.IsNaN(event.X) || math.IsNaN(event.Y) || math.IsInf(event.X, 0) || math.IsInf(event.Y, 0) {
			continue
		}
		if v.cmd == nil {
			cmd := exec.Command(v.helper, "cursor-overlay")
			stdin, err := cmd.StdinPipe()
			if err != nil || cmd.Start() != nil {
				if stdin != nil {
					_ = stdin.Close()
				}
				continue
			}
			v.cmd, v.stdin = cmd, stdin
			v.note(map[string]any{"event": "overlay_started", "pid": cmd.Process.Pid})
		}
		command, _ := json.Marshal(map[string]any{"op": "show", "point": map[string]any{"frame": "desktop", "x": event.X, "y": event.Y}})
		if _, err := v.stdin.Write(append(command, '\n')); err == nil {
			v.note(map[string]any{"event": "show", "x": event.X, "y": event.Y})
		}
	}
	if len(v.buffer) > 4096 {
		v.buffer = nil
	}
	return len(p), nil
}

func (v *virtualCursorOverlay) note(row map[string]any) {
	if v.trace != nil {
		_ = json.NewEncoder(v.trace).Encode(row)
	}
}

func (v *virtualCursorOverlay) Close() {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.stdin != nil {
		_ = v.stdin.Close()
	}
	if v.cmd != nil {
		_ = v.cmd.Wait()
	}
	if v.trace != nil {
		_ = v.trace.Close()
	}
}
