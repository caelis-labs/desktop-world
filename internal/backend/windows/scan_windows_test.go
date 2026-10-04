//go:build windows && amd64

package windows

import (
	"context"
	"fmt"
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
