//go:build windows && amd64

package windows

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"time"
)

// All pending COM references remain on the driver's fixed MTA worker.
type uiaFrame struct {
	key               backend.Key
	depth             int
	visited, expanded bool
	next              *com
	windows           []backend.Key
}
type uiaScan struct {
	stack   []*uiaFrame
	seen    map[backend.Key]bool
	visited int
	dirty   bool
	expires time.Time
}

func (s *uiaScan) pop() {
	n := len(s.stack)
	if n == 0 {
		return
	}
	s.stack[n-1].next.release()
	s.stack = s.stack[:n-1]
}
func (s *uiaScan) release() {
	for len(s.stack) > 0 {
		s.pop()
	}
}
func (d *Driver) beginScan(ctx context.Context, q backend.Query) (*uiaScan, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	now := time.Now()
	for id, s := range d.scans {
		if now.After(s.expires) {
			s.release()
			delete(d.scans, id)
		}
	}
	if q.Resume != "" {
		s := d.scans[q.Resume]
		if s == nil {
			return nil, dw.NewFault("continuation_expired", "native scan expired", "reobserve")
		}
		delete(d.scans, q.Resume)
		s.dirty = true
		return s, nil
	}
	if !q.NoContinuation && len(d.scans) >= 16 {
		return nil, dw.NewFault("uia_scan_capacity", "16 native scans are active; resume one or wait for expiry", "never_automatically")
	}
	s := &uiaScan{seen: map[backend.Key]bool{}, expires: now.Add(90 * time.Second)}
	roots := q.Roots
	if q.Desktop {
		state := windowCollection{driver: d, ctx: ctx, max: 10000, complete: true}
		id := registerCallback(&state)
		enumWindows.Call(windowCallback, id)
		callbackStates.Delete(id)
		roots = state.roots
		s.dirty = !state.complete
	}
	for i := len(roots) - 1; i >= 0; i-- {
		s.stack = append(s.stack, &uiaFrame{key: roots[i]})
	}
	return s, nil
}

// Yield before the caller expires: one bounded provider call may still be in
// flight, and the engine must commit facts and recheck authorization afterwards.
func scanDeadline(ctx context.Context, q backend.Query) time.Time {
	now := time.Now()
	allowance := 2 * time.Second
	if q.ReadTimeoutMS > 0 {
		allowance = time.Duration(q.ReadTimeoutMS) * time.Millisecond
	}
	if end, ok := ctx.Deadline(); ok {
		remaining := time.Until(end)
		allowance = min(allowance, max(0, remaining-max(100*time.Millisecond, remaining/4)))
	}
	return now.Add(allowance)
}

// The same budget/ownership loop is exercised by controlled slow-provider
// tests. Native transitions and all COM releases execute on the owning worker.
func (d *Driver) scanPage(ctx context.Context, q backend.Query, scan *uiaScan, deadline time.Time, advance func(*backend.Page), seat func() backend.Seat) (backend.Page, error) {
	p := backend.Page{}
	start := scan.visited
	timedOut := false
	for len(scan.stack) > 0 && scan.visited-start < q.MaxNodes && scan.visited < 10000 {
		if err := ctx.Err(); err != nil {
			scan.release()
			return backend.Page{}, err
		}
		if !time.Now().Before(deadline) {
			timedOut = true
			break
		}
		advance(&p)
	}
	if err := ctx.Err(); err != nil {
		scan.release()
		return backend.Page{}, err
	}
	p.Visited = scan.visited
	if len(scan.stack) > 0 && scan.visited >= 10000 {
		p.Unavailable = append(p.Unavailable, "uia_scan_limit")
		scan.dirty = true
		scan.release()
	}
	p.Complete = len(scan.stack) == 0 && !scan.dirty
	p.Dirty = scan.dirty
	if len(scan.stack) > 0 {
		reason := "uia_node_budget"
		if timedOut {
			reason = "uia_timeout"
		}
		p.Unavailable = append(p.Unavailable, reason)
		if q.NoContinuation {
			scan.release()
		}
	}
	if !p.Complete && len(p.Unavailable) == 0 {
		p.Unavailable = []string{"uia_partial"}
	}
	// A budget yield must not add more provider calls for optional seat facts.
	// Full seat sampling is left for a later read with its own allowance.
	if timedOut || !time.Now().Before(deadline) {
		p.Seat = backend.Seat{Pointer: dw.Unknown[dw.Point](), Health: "ready", Intervention: "best_effort"}
	} else {
		p.Seat = seat()
	}
	if err := ctx.Err(); err != nil {
		scan.release()
		return backend.Page{}, err
	}
	if len(scan.stack) > 0 {
		var id [16]byte
		if _, err := rand.Read(id[:]); err != nil {
			scan.release()
			return backend.Page{}, err
		}
		p.ScanCursor = hex.EncodeToString(id[:])
		d.scans[p.ScanCursor] = scan
	}
	return p, nil
}
