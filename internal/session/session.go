// Package session is the trusted, language-neutral owner facade over host.Client.
// Desktop arguments never become host control requests.
package session

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"sync"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
)

const Version = "desktop-world/session-v0.1"

type Request struct {
	ID, Channel, Op string
	Args            json.RawMessage
}
type OwnerArgs struct {
	Turn                       string
	Application                dw.Ref
	Name, WindowTitle, GrantID string
}
type preflightKey struct{}

type record struct {
	body  string
	done  chan struct{}
	reply host.Reply
}
type Session struct {
	Client   *host.Client
	mu       sync.Mutex
	turn     string
	lastTurn string
	records  map[string]*record
}

func New(ctx context.Context, c *host.Client) (*Session, error) {
	if err := c.BeginTurn(ctx, "session"); err != nil {
		return nil, err
	}
	return &Session{Client: c, turn: "session", lastTurn: "session", records: map[string]*record{}}, nil
}
func (s *Session) Handle(ctx context.Context, r Request) host.Reply {
	out := host.Reply{ID: r.ID, Protocol: Version, World: s.Client.Hello.Environment.Epoch}
	if r.ID == "" || len(r.ID) > 128 || (r.Channel != "desktop" && r.Channel != "host") {
		out.Error = dw.Invalid("session requires stable id and desktop or host channel")
		return out
	}
	if len(r.Args) == 0 {
		r.Args = json.RawMessage(`{}`)
	}
	b, err := protocol.Marshal(r)
	if err != nil {
		out.Error = dw.Invalid(err.Error())
		return out
	}
	s.mu.Lock()
	if previous := s.records[r.ID]; previous != nil {
		s.mu.Unlock()
		if previous.body != string(b) {
			out.Error = dw.NewFault("request_conflict", "reuse id only with the same channel, operation and body", "never_automatically")
			return out
		}
		select {
		case <-previous.done:
			return previous.reply
		default:
		}
		select {
		case <-previous.done:
			return previous.reply
		case <-ctx.Done():
			out.Error = dw.NewFault("session_pending", "original request still pending; reconcile its identical id and body", "never_automatically")
			return out
		}
	}
	if len(s.records) >= 4096 {
		s.mu.Unlock()
		out.Error = dw.NewFault("resource_exhausted", "session request limit reached; retain receipts before ending session", "never_automatically")
		return out
	}
	entry := &record{body: string(b), done: make(chan struct{})}
	s.records[r.ID] = entry
	turn := s.turn
	if turn == "" && (r.Op == "get" || r.Op == "cancel") {
		turn = s.lastTurn
	}
	s.mu.Unlock()
	defer func() { s.mu.Lock(); entry.reply = out; close(entry.done); s.mu.Unlock() }()
	if fault, ok := ctx.Value(preflightKey{}).(*dw.Fault); ok {
		out.Error = fault
		return out
	}
	if r.Channel == "desktop" {
		// These are the complete model-facing verbs. Forged grant/turn is rejected here.
		switch r.Op {
		case "observe", "read", "sync", "act", "capture", "get", "cancel":
		default:
			out.Error = dw.Invalid("desktop channel supports observe, read, sync, act, capture, get, cancel")
			return out
		}
		if turn == "" {
			out.Error = dw.NewFault("turn_expired", "trusted owner must begin a new turn", "never_automatically")
			return out
		}
		reply, e := s.Client.Call(ctx, turn, r.ID, r.Op, r.Args)
		out.Result, out.Error = reply.Result, reply.Error
		if e != nil {
			out.Error = dw.NewFault("session_unknown", fmt.Sprintf("original request %s may have effects: %v; do not restart or replay", r.ID, e), "never_automatically")
		}
		return out
	}
	var a OwnerArgs
	if err = protocol.Decode(r.Args, &a); err != nil {
		out.Error = dw.Invalid(err.Error())
		return out
	}
	if a.Turn != "" {
		turn = a.Turn
	}
	if a.Application != "" || a.Name != "" || a.WindowTitle != "" || a.GrantID != "" {
		if r.Op != "grant" && r.Op != "declare" && r.Op != "revoke" {
			out.Error = dw.Invalid("this owner operation accepts turn only")
			return out
		}
	}
	if r.Op == "declare" && (a.Application != "" || a.GrantID != "") {
		out.Error = dw.Invalid("declare accepts name or window_title only")
		return out
	}
	var result any = map[string]any{"acknowledged": true, "operation": r.Op}
	switch r.Op {
	case "grant":
		if a.Application == "" || a.Name != "" || a.WindowTitle != "" || a.GrantID != "" {
			err = dw.Invalid("grant requires application only")
		} else {
			err = s.Client.Grant(ctx, turn, a.Application)
		}
	case "declare":
		err = s.Client.Declare(ctx, turn, a.Name, a.WindowTitle)
	case "revoke":
		if (a.Application == "") == (a.GrantID == "") || a.Name != "" || a.WindowTitle != "" {
			err = dw.Invalid("revoke requires application or grant_id")
		} else if a.Application != "" {
			err = s.Client.Revoke(ctx, turn, a.Application)
		} else {
			err = s.Client.RevokeGrant(ctx, turn, a.GrantID)
		}
	case "grants":
		result, err = s.Client.Grants(ctx, turn)
	case "end_turn":
		err = s.Client.EndTurn(ctx, turn)
		if err == nil {
			s.mu.Lock()
			if s.turn == turn {
				s.turn = ""
			}
			s.mu.Unlock()
		}
	case "begin_turn":
		if a.Turn == "" {
			err = dw.Invalid("begin_turn requires a fresh turn id")
		} else {
			err = s.Client.BeginTurn(ctx, a.Turn)
			if err == nil {
				s.mu.Lock()
				s.turn = a.Turn
				s.lastTurn = a.Turn
				s.mu.Unlock()
			}
		}
	case "status":
		result = map[string]any{"epoch": out.World, "turn": turn, "input_mode": s.Client.Hello.InputMode, "input_policy": s.Client.Hello.InputPolicy}
	default:
		err = dw.Invalid("host channel supports declare, grant, revoke, grants, begin_turn, end_turn, status")
	}
	if err != nil {
		if f, ok := err.(*dw.Fault); ok {
			out.Error = f
		} else {
			out.Error = dw.NewFault("host_control_failed", err.Error(), "never_automatically")
		}
	} else {
		out.Result, _ = protocol.Marshal(result)
	}
	return out
}
func (s *Session) Serve(ctx context.Context, in io.Reader, out io.Writer) (serveErr error) {
	ctx, sessionCancel := context.WithCancel(ctx)
	scan := bufio.NewScanner(in)
	scan.Buffer(make([]byte, 4096), 1<<20)
	if closer, ok := in.(io.Closer); ok {
		go func() { <-ctx.Done(); closer.Close() }()
	}
	var writes sync.Mutex
	var wg sync.WaitGroup
	defer func() {
		sessionCancel()
		closeErr := s.Client.CloseWithError()
		wg.Wait()
		if serveErr == nil {
			serveErr = closeErr
		}
	}()
	dataLimits := make(chan struct{}, 2)
	controlLimits := make(chan struct{}, 2)
	for scan.Scan() {
		var r Request
		if e := protocol.Decode(scan.Bytes(), &r); e != nil {
			writes.Lock()
			writeErr := json.NewEncoder(out).Encode(host.Reply{Protocol: Version, World: s.Client.Hello.Environment.Epoch, Error: dw.Invalid(e.Error())})
			writes.Unlock()
			if writeErr != nil {
				return writeErr
			}
			continue
		}
		limits := dataLimits
		if r.Channel == "host" || r.Op == "get" || r.Op == "cancel" {
			limits = controlLimits
		}
		select {
		case limits <- struct{}{}:
		default:
			writes.Lock()
			busyCtx, busyCancel := context.WithCancel(ctx)
			busyCancel()
			reply := s.Handle(context.WithValue(busyCtx, preflightKey{}, dw.NewFault("session_busy", "session capacity reached before helper submission", "never_automatically")), r)
			data, _ := protocol.Marshal(reply)
			_, writeErr := fmt.Fprintln(out, string(data))
			writes.Unlock()
			if writeErr != nil {
				return writeErr
			}
			continue
		}
		wg.Add(1)
		go func(r Request) {
			defer wg.Done()
			defer func() { <-limits }()
			callCtx, stopCall := context.WithTimeout(ctx, 12*time.Second)
			defer stopCall()
			reply := s.Handle(callCtx, r)
			data, e := protocol.Marshal(reply)
			writes.Lock()
			defer writes.Unlock()
			if e == nil {
				if _, err := fmt.Fprintln(out, string(data)); err != nil {
					sessionCancel()
				}
			}
		}(r)
	}
	return scan.Err()
}
