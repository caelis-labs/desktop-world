//go:build darwin && cgo && dtw_poc_exactgrant

package main

import (
	"context"
	"errors"
	"testing"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/engine"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"github.com/caelis-labs/desktop-world/poc/native-go-runtime/internal/exactgrant"
)

type fakeSource struct {
	identity engine.POCWindowIdentity
	err      error
}

func (s *fakeSource) POCWindowIdentity(context.Context, dw.Ref) (engine.POCWindowIdentity, error) {
	return s.identity, s.err
}

func TestVisibleExactGrantStatusTracksUnresolvedIdentity(t *testing.T) {
	id := engine.POCWindowIdentity{PID: 1234, StartSec: 10, StartUSec: 20,
		NativeWindowID: 111, Application: "app-a", Window: "win-a"}
	bound, err := exactgrant.Bind(toGrantIdentity(id), toGrantIdentity(id))
	if err != nil {
		t.Fatal(err)
	}
	source := &fakeSource{identity: id}
	gate := newGate(source)
	gate.windows["grant-1"] = boundWindow{app: "app-a", ref: "win-a", grant: bound}
	status := helper.GrantStatus{Turn: "session", Grants: []helper.ApplicationGrant{{
		ID: "grant-1", Application: "app-a", WindowTitle: "Target", State: "active",
	}}}
	if actual := gate.visibleStatus(context.Background(), status).Grants[0]; actual.State != "active" {
		t.Fatalf("live grant incorrectly hidden: %+v", actual)
	}
	source.identity.NativeWindowID = 222
	if actual := gate.visibleStatus(context.Background(), status).Grants[0]; actual.State != "unresolved" || actual.Reason != "window_identity_unavailable_or_changed" {
		t.Fatalf("replacement still looked active: %+v", actual)
	}
	source.err = errors.New("AX mapping unavailable")
	if actual := gate.visibleStatus(context.Background(), status).Grants[0]; actual.State != "unresolved" {
		t.Fatalf("unknown mapping still looked active: %+v", actual)
	}
}
