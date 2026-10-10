package helper

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"sync"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/protocol"
)

const ControlVersion = "desktop-world/host-control-v0.1"

// ControlRequest belongs to a private inherited pipe owned by the application.
// It is deliberately absent from agent schemas and the data dispatcher.
type ControlRequest struct {
	ID, Op, Turn               string
	Application                dw.Ref
	Name, WindowTitle, GrantID string
}

type turnKey struct{}
type turnGrants struct {
	mu           sync.Mutex
	turn         string
	used         map[string]bool
	apps         map[dw.Ref]bool
	ctx          context.Context
	cancel       context.CancelFunc
	declarations map[string]*ApplicationGrant
	appContexts  map[dw.Ref]context.Context
	appCancels   map[dw.Ref]context.CancelFunc
	sequence     uint64
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

func (g *turnGrants) begin(turn string) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if !validTurn(turn) {
		return dw.Invalid("turn requires 1..64 ASCII letters, digits, hyphen or underscore")
	}
	if g.turn == turn {
		return nil
	} // Lost control response can be reconciled.
	if g.turn != "" {
		return dw.NewFault("turn_active", "end the current turn before starting another", "never_automatically")
	}
	if g.used[turn] {
		return dw.NewFault("turn_expired", "ended turn IDs cannot be reused", "never_automatically")
	}
	if len(g.used) >= 4096 {
		return dw.NewFault("resource_exhausted", "managed session turn limit reached", "never_automatically")
	}
	g.turn, g.apps, g.used[turn] = turn, map[dw.Ref]bool{}, true
	g.ctx, g.cancel = context.WithCancel(context.Background())
	g.declarations = map[string]*ApplicationGrant{}
	g.appContexts = map[dw.Ref]context.Context{}
	g.appCancels = map[dw.Ref]context.CancelFunc{}
	return nil
}

func (g *turnGrants) end(turn string) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.turn == "" && g.used[turn] {
		return nil
	}
	if turn == "" || g.turn != turn {
		return dw.NewFault("turn_expired", "no matching active turn", "never_automatically")
	}
	g.cancel()
	g.turn, g.apps = "", nil
	return nil
}

func (g *turnGrants) stop() {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.cancel != nil {
		g.cancel()
	}
	g.turn, g.apps = "", nil
}

func (g *turnGrants) bind(ctx context.Context, turn string) (context.Context, func(), error) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if turn == "" || turn != g.turn {
		return nil, nil, dw.NewFault("turn_expired", "trusted host must supply the active turn", "never_automatically")
	}
	c, cancel := context.WithCancel(context.WithValue(ctx, turnKey{}, turn))
	watch := &grantWatch{cancel: cancel, stops: map[dw.Ref]func() bool{}}
	c = context.WithValue(c, grantWatchKey{}, watch)
	stop := context.AfterFunc(g.ctx, cancel)
	return c, func() { stop(); watch.close(); cancel() }, nil
}

func (g *turnGrants) Check(ctx context.Context, in dw.Intent) (dw.Decision, error) {
	g.mu.Lock()
	defer g.mu.Unlock()
	turn, _ := ctx.Value(turnKey{}).(string)
	if ctx.Err() != nil || turn == "" || turn != g.turn {
		return dw.Decision{}, nil
	}
	switch in.Operation {
	case "observe", "read", "sync", "bind", "bind_focus", "wait", "resolve_anchor", "capture", "read_asset":
		return dw.Decision{Allow: true}, nil
	}
	if len(in.Targets) == 0 || len(in.Applications) == 0 {
		return dw.Decision{}, nil
	}
	for _, app := range in.Applications {
		if !g.apps[app] {
			return dw.Decision{}, nil
		}
		if watch, ok := ctx.Value(grantWatchKey{}).(*grantWatch); ok {
			watch.add(app, g.appContexts[app])
		}
	}
	return dw.Decision{Allow: true}, nil
}

func (s *Server) Control(ctx context.Context, r ControlRequest) Response {
	out := Response{ID: r.ID, Protocol: ControlVersion, World: s.epoch}
	var err error
	// The tagged core POC uses this private pipe only for Session lifecycle.
	// Keep DTW's own grant operations unavailable even to its trusted host.
	if pocCoreWithoutGrants(s.config) && r.Op != "begin_turn" && r.Op != "end_turn" {
		out.Error = dw.Invalid("DTW grants are unavailable in the core POC")
		return out
	}
	if !s.config.Managed && r.Turn == "" {
		r.Turn = "session"
	}
	if s.grants == nil || len(r.ID) < 1 || len(r.ID) > 128 || !validTurn(r.Turn) {
		err = dw.Invalid("control requires managed mode, id and a valid turn")
	} else {
		switch r.Op {
		case "begin_turn":
			if r.Application != "" || r.Name != "" || r.WindowTitle != "" || r.GrantID != "" {
				err = dw.Invalid("begin_turn does not accept application")
			} else {
				err = s.grants.begin(r.Turn)
			}
		case "end_turn":
			if r.Application != "" || r.Name != "" || r.WindowTitle != "" || r.GrantID != "" {
				err = dw.Invalid("end_turn does not accept application")
			} else {
				err = s.grants.end(r.Turn)
			}
		case "grant":
			if r.Name != "" || r.WindowTitle != "" || r.GrantID != "" {
				err = dw.Invalid("grant accepts application only")
				break
			}
			err = s.grant(ctx, r.Turn, r.Application)
		case "declare":
			err = s.declare(r)
		case "revoke":
			err = s.revoke(r)
		case "grants":
			if r.Application != "" || r.Name != "" || r.WindowTitle != "" || r.GrantID != "" {
				err = dw.Invalid("grants accepts turn only")
				break
			}
			s.refreshGrants(ctx, r.Turn)
			out.Result, err = s.grantStatus(r.Turn)
		default:
			err = dw.Invalid("host control supports begin_turn, grant, declare, revoke, grants and end_turn")
		}
	}
	if err != nil {
		out.Error = asFault(err)
	} else if out.Result == nil {
		out.Result = map[string]any{"turn": r.Turn, "operation": r.Op, "application": r.Application, "acknowledged": true}
	}
	if s.config.Audit != nil {
		var states []map[string]any
		if snapshot, e := s.grantStatus(r.Turn); e == nil {
			for _, grant := range snapshot.Grants {
				states = append(states, map[string]any{"id": grant.ID, "application": grant.Application, "state": grant.State, "reason": grant.Reason})
			}
		}
		s.mu.Lock()
		row := map[string]any{"event": "authorization", "epoch": s.epoch, "id": r.ID, "op": r.Op, "turn": r.Turn, "application": r.Application, "grants": states}
		if out.Error != nil {
			row["error_code"] = out.Error.Code
		}
		if e := json.NewEncoder(s.config.Audit).Encode(row); e != nil {
			s.grants.stop()
			out.Error = dw.NewFault("audit_failed", "authorization revoked because metadata audit failed", "never_automatically")
		}
		s.mu.Unlock()
	}
	return out
}

func (s *Server) grant(ctx context.Context, turn string, app dw.Ref) error {
	if app == "" {
		return dw.Invalid("grant requires an observed application Ref")
	}
	c, done, err := s.grants.bind(ctx, turn)
	if err != nil {
		return err
	}
	defer done()
	ob, err := s.actor.Observe(c, dw.ObserveRequest{Freshness: dw.Freshness{Mode: "refresh"}, Scope: dw.Scope{Refs: []dw.Ref{app}}, Projection: dw.ProjectionDetail, Fields: []string{"kind", "lifecycle"}, Budget: dw.Budget{MaxResults: 1}})
	if err != nil {
		return err
	}
	if len(ob.Objects) != 1 || ob.Objects[0].Ref != app || ob.Objects[0].Kind != dw.KindApplication || ob.Objects[0].Lifecycle != dw.LifeLive {
		return dw.Invalid("grant requires a live application Ref; names, windows and native IDs are not grants")
	}
	s.grants.mu.Lock()
	defer s.grants.mu.Unlock()
	if c.Err() != nil || s.grants.turn != turn {
		return dw.NewFault("turn_expired", "turn ended during authorization", "never_automatically")
	}
	key := "ref:" + string(app)
	if len(s.grants.declarations) >= 32 && s.grants.declarations[key] == nil {
		return dw.Invalid("at most 32 application grants per turn")
	}
	s.grants.apps[app] = true
	if s.grants.appContexts[app] == nil {
		s.grants.appContexts[app], s.grants.appCancels[app] = context.WithCancel(s.grants.ctx)
	}
	s.grants.declarations[key] = &ApplicationGrant{ID: key, Application: app, State: "active"}
	return nil
}

// EOF on the host control channel revokes all grants, even if stdin stays open.
// The process owner must also cancel the data server when this method returns.
func (s *Server) ServeControl(ctx context.Context, in io.Reader, out io.Writer) error {
	if s.grants == nil {
		return dw.Invalid("managed mode required")
	}
	defer s.grants.stop()
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
		b, err := protocol.Marshal(reply)
		if err != nil {
			return err
		}
		if _, err = fmt.Fprintln(out, string(b)); err != nil {
			return err
		}
	}
	return scanner.Err()
}
