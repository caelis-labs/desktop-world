package helper

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"sync"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/protocol"
)

const ControlVersion = "desktop-world/host-control-v0.1"

// The private control pipe owns only session lifecycle. The embedding
// application makes user authorization decisions outside this helper.
type ControlRequest struct{ ID, Op, Turn string }

type turnLifecycle struct {
	mu     sync.Mutex
	turn   string
	used   map[string]bool
	ctx    context.Context
	cancel context.CancelFunc
}

func validTurn(s string) bool {
	if len(s) < 1 || len(s) > 64 {
		return false
	}
	for _, c := range s {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}

func (t *turnLifecycle) begin(turn string) error {
	t.mu.Lock()
	defer t.mu.Unlock()
	if !validTurn(turn) {
		return dw.Invalid("turn requires 1..64 ASCII letters, digits, hyphen or underscore")
	}
	if t.turn == turn {
		return nil // Reconcile a lost control reply.
	}
	if t.turn != "" {
		return dw.NewFault("turn_active", "end the current turn first", "never_automatically")
	}
	if t.used[turn] {
		return dw.NewFault("turn_expired", "ended turn IDs cannot be reused", "never_automatically")
	}
	if len(t.used) >= 4096 {
		return dw.NewFault("resource_exhausted", "session turn limit reached", "never_automatically")
	}
	t.turn, t.used[turn] = turn, true
	t.ctx, t.cancel = context.WithCancel(context.Background())
	return nil
}

func (t *turnLifecycle) end(turn string) error {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.turn == "" && t.used[turn] {
		return nil
	}
	if turn == "" || t.turn != turn {
		return dw.NewFault("turn_expired", "no matching active turn", "never_automatically")
	}
	t.cancel()
	t.turn = ""
	return nil
}

func (t *turnLifecycle) stop() {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.cancel != nil {
		t.cancel()
	}
	t.turn = ""
}

func (t *turnLifecycle) bind(ctx context.Context, turn string) (context.Context, func(), error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if turn == "" || turn != t.turn {
		return nil, nil, dw.NewFault("turn_expired", "host must supply the active turn", "never_automatically")
	}
	bound, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(t.ctx, cancel)
	return bound, func() { stop(); cancel() }, nil
}

func (s *Server) Control(_ context.Context, r ControlRequest) Response {
	out := Response{ID: r.ID, Protocol: ControlVersion, World: s.epoch}
	if !s.config.Managed || len(r.ID) == 0 || len(r.ID) > 128 || !validTurn(r.Turn) {
		out.Error = dw.Invalid("control requires managed mode, id and valid turn")
		return out
	}
	var err error
	switch r.Op {
	case "begin_turn":
		err = s.turns.begin(r.Turn)
	case "end_turn":
		err = s.turns.end(r.Turn)
	default:
		err = dw.Invalid("host control supports begin_turn and end_turn")
	}
	if err != nil {
		out.Error = asFault(err)
	} else {
		out.Result = map[string]any{"turn": r.Turn, "operation": r.Op, "acknowledged": true}
	}
	return out
}

// Losing the private owner channel cancels all calls associated with its turn.
func (s *Server) ServeControl(ctx context.Context, in io.Reader, out io.Writer) error {
	if !s.config.Managed {
		return dw.Invalid("managed mode required")
	}
	defer s.turns.stop()
	scanner := bufio.NewScanner(in)
	scanner.Buffer(make([]byte, 4096), 16384)
	for scanner.Scan() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		var req ControlRequest
		var reply Response
		if err := protocol.Decode(scanner.Bytes(), &req); err != nil {
			reply = Response{Protocol: ControlVersion, Error: asFault(err)}
		} else {
			reply = s.Control(ctx, req)
		}
		body, err := protocol.Marshal(reply)
		if err != nil {
			return err
		}
		if _, err = fmt.Fprintln(out, string(body)); err != nil {
			return err
		}
	}
	return scanner.Err()
}
