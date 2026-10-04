//go:build windows && amd64

package windows

import (
	"context"
	"fmt"
	"runtime"
	"syscall"
	"testing"
	"time"

	"github.com/caelis-labs/desktop-world/internal/backend"
)

// Cursor lifecycle only. Native provider traversal has a separate interactive test.
func TestScanCapacityResumeAndExpiryPreserveOtherFrontiers(t *testing.T) {
	d := New().(*Driver)
	for i := range 16 {
		d.scans[fmt.Sprint(i)] = &uiaScan{stack: []*uiaFrame{{key: backend.Key(fmt.Sprint(i))}}, seen: map[backend.Key]bool{}, expires: time.Now().Add(time.Minute)}
	}
	if _, err := d.beginScan(context.Background(), backend.Query{}); err == nil || len(d.scans) != 16 {
		t.Fatal("capacity evicted a live frontier", err)
	}
	old := d.scans["0"]
	s, err := d.beginScan(context.Background(), backend.Query{Resume: "0"})
	if err != nil || s != old || !s.dirty || len(s.stack) != 1 || len(d.scans) != 15 {
		t.Fatal("resume lost frontier", s, err)
	}
	if _, err = d.beginScan(context.Background(), backend.Query{Resume: "0"}); err == nil {
		t.Fatal("consumed native cursor reused")
	}
	expired := d.scans["1"]
	expired.expires = time.Now().Add(-time.Second)
	if _, err = d.beginScan(context.Background(), backend.Query{Resume: "1"}); err == nil || len(expired.stack) != 0 || len(d.scans) != 14 {
		t.Fatal("expiry failed to release pending references", err)
	}
	for _, cursor := range d.scans {
		if len(cursor.stack) != 1 {
			t.Fatal("another cursor was disturbed")
		}
	}
}

func TestOneShotAtCapacityReleasesPendingCOMOnOwningThread(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	threadID := proc(kernel32, "GetCurrentThreadId")
	owner, _, _ := threadID.Call()
	releases := 0
	var releasedOn uintptr
	vt := &[96]uintptr{}
	vt[2] = syscall.NewCallback(func(uintptr) uintptr {
		releases++
		releasedOn, _, _ = threadID.Call()
		return 0
	})
	pending := &com{vt: vt}
	d := New().(*Driver)
	for i := range 16 {
		d.scans[fmt.Sprint(i)] = &uiaScan{stack: []*uiaFrame{{key: "retained"}}, expires: time.Now().Add(time.Minute)}
	}
	q := backend.Query{Roots: []backend.Key{"one-shot"}, NoContinuation: true, MaxNodes: 1}
	s, err := d.beginScan(context.Background(), q)
	if err != nil {
		t.Fatal("one-shot query unnecessarily needs a cursor slot", err)
	}
	s.stack[0].next = pending
	p, err := d.scanPage(context.Background(), q, s, time.Now().Add(-time.Second), func(*backend.Page) { t.Fatal("expired budget started traversal") }, func() backend.Seat { t.Fatal("expired budget sampled provider seat"); return backend.Seat{} })
	if err != nil || p.Complete || p.ScanCursor != "" || len(s.stack) != 0 || len(d.scans) != 16 || releases != 1 || releasedOn != owner {
		t.Fatalf("one-shot ownership leaked: page=%+v err=%v slots=%d releases=%d thread=%d want=%d", p, err, len(d.scans), releases, releasedOn, owner)
	}
	// Cancellation releases this query only, and never retains an inaccessible cursor.
	s = &uiaScan{stack: []*uiaFrame{{next: pending}}}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err = d.scanPage(ctx, backend.Query{MaxNodes: 1}, s, time.Now().Add(time.Second), func(*backend.Page) { t.Fatal("cancelled query advanced") }, func() backend.Seat { return backend.Seat{} }); err != context.Canceled || len(s.stack) != 0 || len(d.scans) != 16 || releases != 2 {
		t.Fatal("cancelled query retained frontier", err)
	}
	runtime.KeepAlive(pending)
	runtime.KeepAlive(vt)
}
