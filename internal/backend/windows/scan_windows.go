//go:build windows && amd64

package windows

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
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
	if len(d.scans) >= 16 {
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
