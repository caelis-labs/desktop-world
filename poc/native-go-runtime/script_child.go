package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strings"
	"sync"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
)

// The POC child has no MCP, native helper, grants or receipt ledger. Its only
// durable state is the JS context. The parent retains control when JS wedges.
type childMessage struct {
	Type        string          `json:"type"`
	ExecutionID string          `json:"execution_id,omitempty"`
	ID          string          `json:"id,omitempty"`
	Code        string          `json:"code,omitempty"`
	Operation   string          `json:"operation,omitempty"`
	Body        json.RawMessage `json:"body,omitempty"`
	Reply       *host.Reply     `json:"reply,omitempty"`
	State       string          `json:"state,omitempty"`
	Output      []string        `json:"output,omitempty"`
	Error       string          `json:"error,omitempty"`
}

type scriptChild struct {
	cmd     *exec.Cmd
	stdin   io.WriteCloser
	writeMu sync.Mutex
}

func (c *scriptChild) write(msg childMessage) error {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	return json.NewEncoder(c.stdin).Encode(msg)
}

func (s *supervisor) startScriptChild() {
	args := []string{"--script-child"}
	if os.Getenv("DTW_POC_CHILD") == "1" {
		args = []string{"-test.run=^TestStdioChild$"}
	}
	cmd := exec.Command(os.Args[0], args...)
	cmd.Env = append(os.Environ(), "DTW_POC_SCRIPT_CHILD=1")
	cmd.Stderr = os.Stderr
	stdin, err := cmd.StdinPipe()
	if err != nil {
		s.child = nil
		s.childDead = true
		go s.rejectCommands(err)
		return
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		_ = stdin.Close()
		s.childDead = true
		go s.rejectCommands(err)
		return
	}
	if err = cmd.Start(); err != nil {
		_ = stdin.Close()
		s.childDead = true
		go s.rejectCommands(err)
		return
	}
	s.child = &scriptChild{cmd: cmd, stdin: stdin}
	go s.forwardCommands()
	go s.readChild(stdout)
	go func() {
		_ = cmd.Wait()
		s.scriptChildExited()
	}()
}

func (s *supervisor) rejectCommands(err error) {
	for command := range s.commands {
		s.finishLost(command.Record, "script child unavailable: "+err.Error())
	}
}

func (s *supervisor) forwardCommands() {
	for command := range s.commands {
		if err := s.child.write(childMessage{Type: "run", ExecutionID: command.Record.ID, Code: command.Code}); err != nil {
			s.finishLost(command.Record, "script child write failed: "+err.Error())
		}
	}
}

func (s *supervisor) finishLost(r *record, why string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.active != r || r.State == "completed" || r.State == "failed" || r.State == "cancelled" || r.State == "state_lost" {
		return
	}
	r.State, r.Error = "state_lost", why
	s.active = nil
	close(r.Done)
}

func (s *supervisor) scriptChildExited() {
	s.mu.Lock()
	s.childDead = true
	r := s.active
	s.mu.Unlock()
	if r != nil {
		s.finishLost(r, "script subprocess exited; this Session's JavaScript state is lost; inspect original native request IDs before any new action")
	}
}

func (s *supervisor) cancelScript(id string) {
	if s.child == nil {
		return
	}
	_ = s.child.write(childMessage{Type: "cancel", ExecutionID: id})
	time.AfterFunc(300*time.Millisecond, func() {
		s.mu.Lock()
		r := s.records[id]
		pending := r != nil && (r.State == "running" || r.State == "cancelling")
		s.mu.Unlock()
		if pending {
			_ = s.child.cmd.Process.Kill()
		}
	})
}

func (s *supervisor) cancelRecord(r *record) {
	s.mu.Lock()
	if r.State == "running" {
		r.State = "cancelling"
	}
	r.Cancel()
	s.mu.Unlock()
	s.cancelScript(r.ID)
}

func (s *supervisor) readChild(stdout io.Reader) {
	scanner := bufio.NewScanner(stdout)
	scanner.Buffer(make([]byte, 64*1024), 8<<20)
	for scanner.Scan() {
		var msg childMessage
		if json.Unmarshal(scanner.Bytes(), &msg) != nil {
			continue
		}
		switch msg.Type {
		case "native":
			go s.handleChildNative(msg)
		case "done":
			s.mu.Lock()
			r := s.records[msg.ExecutionID]
			if r != nil && s.active == r {
				r.State, r.Output, r.Error = msg.State, msg.Output, msg.Error
				s.active = nil
				close(r.Done)
			}
			s.mu.Unlock()
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "script child IPC:", err)
	}
}

func (s *supervisor) handleChildNative(msg childMessage) {
	s.mu.Lock()
	r := s.records[msg.ExecutionID]
	if r == nil || len(r.NativeIDs) >= 32 {
		s.mu.Unlock()
		return
	}
	r.NativeIDs = append(r.NativeIDs, msg.ID)
	s.mu.Unlock()
	reply := host.Reply{ID: msg.ID}
	var release func()
	var route string
	if msg.Operation == "act" {
		var err error
		release, route, err = s.coordinateAction(r.RunCtx, msg.ID, msg.Body)
		if err != nil {
			code := "coordination_unavailable"
			if r.RunCtx.Err() != nil {
				code = "cancelled"
			}
			if strings.HasPrefix(err.Error(), "user_active:") {
				code = "user_active"
			}
			reply.Error = dw.NewFault(code, err.Error(), "never_automatically")
		}
	}
	if release != nil {
		defer release()
		if delay := os.Getenv("DTW_POC_COORD_HOLD_MS"); delay != "" {
			if duration, err := time.ParseDuration(delay + "ms"); err == nil && duration <= time.Second {
				time.Sleep(duration)
			}
		}
		// A contended seat can be held before native dispatch. Recheck physical
		// activity at that boundary so input that begins during the POC hold
		// cannot be treated as the earlier quiet interval.
		if route == "foreground_transaction" && reply.Error == nil {
			if err := waitForUserInputQuiet(r.RunCtx, 600*time.Millisecond, 3*time.Second); err != nil {
				code := "coordination_unavailable"
				if r.RunCtx.Err() != nil {
					code = "cancelled"
				} else if strings.HasPrefix(err.Error(), "user_active:") {
					code = "user_active"
				}
				reply.Error = dw.NewFault(code, err.Error(), "never_automatically")
			}
		}
	}
	if reply.Error != nil {
		// The action never reached the native helper.
	} else if s.testNative != nil {
		// The isolated test transport can return an original partial/unknown/late
		// receipt without driving any OS UI. It is never installed by runServer.
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		got, err := s.testNative(ctx, msg.ID, msg.Operation, msg.Body)
		if err != nil {
			reply.Error = dw.NewFault("native_unknown", err.Error(), "never_automatically")
		} else {
			reply = got
		}
	} else if s.native == nil {
		reply.Error = dw.NewFault("native_unavailable", "native helper is not configured", "never_automatically")
	} else {
		// A cancelled/wedged script must not destroy the original native reply.
		// Reconcile reads the same request ID; it never redispatches a write.
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		s.nativeMu.Lock()
		var got host.Reply
		var err error
		got, err = s.native.Call(ctx, "session", msg.ID, msg.Operation, msg.Body)
		if err != nil {
			got, err = s.native.Reconcile(ctx, "session", msg.ID)
		}
		s.nativeMu.Unlock()
		if err != nil {
			var fault *dw.Fault
			if errors.As(err, &fault) {
				reply.Error = fault
			} else {
				reply.Error = dw.NewFault("native_unknown", err.Error(), "never_automatically")
			}
		} else {
			reply = got
		}
		if msg.Operation == os.Getenv("DTW_POC_NATIVE_DELAY_OP") {
			if delay := os.Getenv("DTW_POC_NATIVE_REPLY_DELAY_MS"); delay != "" {
				if duration, parseErr := time.ParseDuration(delay + "ms"); parseErr == nil && duration <= 2*time.Second {
					time.Sleep(duration)
				}
			}
		}
	}
	s.mu.Lock()
	if r.NativeReceipts == nil {
		r.NativeReceipts = make(map[string]host.Reply)
	}
	r.NativeReceipts[msg.ID] = reply
	if reply.Error != nil {
		r.NativeError = reply.Error
	}
	if reply.Error == nil {
		if msg.Operation == "observe" {
			s.rememberObservation(reply.Result, msg.Body)
		}
		facts := nativeFacts(msg.Operation, msg.ID, reply.Result)
		if facts != nil {
			switch msg.Operation {
			case "observe":
				r.Observations = append(r.Observations, facts)
			case "act":
				r.Actions = append(r.Actions, facts)
			case "capture":
				r.Captures = append(r.Captures, facts)
			}
		}
	}
	s.mu.Unlock()
	if s.child != nil {
		_ = s.child.write(childMessage{Type: "native_reply", ID: msg.ID, Reply: &reply})
	}
}

func runScriptChild() error {
	commands := make(chan scriptCommand)
	var writeMu sync.Mutex
	write := func(msg childMessage) error {
		writeMu.Lock()
		defer writeMu.Unlock()
		return json.NewEncoder(os.Stdout).Encode(msg)
	}
	var pendingMu sync.Mutex
	pending := make(map[string]chan host.Reply)
	s := &supervisor{commands: commands}
	s.remoteNative = func(ctx context.Context, id, op string, body json.RawMessage) (host.Reply, error) {
		result := make(chan host.Reply, 1)
		pendingMu.Lock()
		pending[id] = result
		pendingMu.Unlock()
		defer func() { pendingMu.Lock(); delete(pending, id); pendingMu.Unlock() }()
		if err := write(childMessage{Type: "native", ExecutionID: executionFromNativeID(id), ID: id, Operation: op, Body: body}); err != nil {
			return host.Reply{}, err
		}
		select {
		case reply := <-result:
			return reply, nil
		case <-ctx.Done():
			return host.Reply{}, ctx.Err()
		}
	}
	s.onDone = func(r *record) {
		_ = write(childMessage{Type: "done", ExecutionID: r.ID, State: r.State, Output: r.Output, Error: r.Error})
	}
	var activeMu sync.Mutex
	var active *record
	go func() {
		scanner := bufio.NewScanner(os.Stdin)
		scanner.Buffer(make([]byte, 64*1024), 8<<20)
		for scanner.Scan() {
			var msg childMessage
			if json.Unmarshal(scanner.Bytes(), &msg) != nil {
				continue
			}
			switch msg.Type {
			case "run":
				ctx, cancel := context.WithCancel(context.Background())
				r := &record{ID: msg.ExecutionID, State: "running", Done: make(chan struct{}), Cancel: cancel}
				activeMu.Lock()
				active = r
				activeMu.Unlock()
				commands <- scriptCommand{Ctx: ctx, Code: msg.Code, Record: r}
			case "cancel":
				activeMu.Lock()
				if active != nil && active.ID == msg.ExecutionID {
					active.Cancel()
				}
				activeMu.Unlock()
			case "native_reply":
				pendingMu.Lock()
				ch := pending[msg.ID]
				pendingMu.Unlock()
				if ch != nil && msg.Reply != nil {
					ch <- *msg.Reply
				}
			}
		}
		close(commands)
	}()
	s.scriptLoop()
	return nil
}

func executionFromNativeID(id string) string {
	// The script loop appends this fixed suffix, preserving arbitrary execution IDs.
	if i := strings.LastIndex(id, "-native-"); i >= 0 {
		return id[:i]
	}
	return id
}
