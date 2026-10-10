package contract_test

import (
	"testing"
	"time"

	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/testutil"
)

// A persistent task must not stop observing when rapid snapshots fill the
// baseline cache. Eviction is explicit to sync clients and must not invalidate
// separately retained continuation pages or change native object identities.
func TestRapidSnapshotsEvictBaselinesAndPreserveContinuation(t *testing.T) {
	h := setup(t, dwtest.Options{ViewLimit: 4, HistoryTTL: time.Hour})
	paged := dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline, Budget: dw.Budget{MaxResults: 1}}
	first, err := h.a.Observe(ctx, paged)
	if err != nil || first.Coverage.Continuation == "" {
		t.Fatal(first, err)
	}
	var latest dw.Observation
	for i := 0; i < 160; i++ {
		latest, err = h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline})
		if err != nil || !latest.Coverage.Complete {
			t.Fatalf("snapshot %d: %+v %v", i, latest, err)
		}
	}
	expired, err := h.a.Changes(ctx, dw.ChangeRequest{Cursor: first.Cursor})
	if err != nil || !expired.ResetRequired || expired.ResetReason != "cursor_expired" {
		t.Fatal(expired, err)
	}
	h.f.Update("field", func(n *dwtest.Node) { n.Object.Name = dw.Known("updated") })
	delta, err := h.a.Changes(ctx, dw.ChangeRequest{Cursor: latest.Cursor})
	if err != nil || delta.ResetRequired || len(delta.Upserts) != 1 || delta.Upserts[0].Ref != h.refs["内容"] {
		t.Fatal(delta, err)
	}
	paged.Continuation = first.Coverage.Continuation
	next, err := h.a.Observe(ctx, paged)
	if err != nil || len(next.Objects) != 1 || next.Objects[0].Ref == first.Objects[0].Ref {
		t.Fatal("baseline eviction discarded or replayed the pending page", next, err)
	}
}
