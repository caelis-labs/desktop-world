package contract_test

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"testing"
)

func TestWindowCaptureScopeIdentityAndLocalCoordinates(t *testing.T) {
	h := setup(t)
	ref := h.refs["Desktop World Fixture"]
	a, err := h.w.NewActor(ctx, dw.ActorConfig{ID: "window-pixels", ReadScopes: []dw.Scope{{Refs: []dw.Ref{ref}}}, Operations: []string{"capture", "read_asset"}, InputPolicy: dw.InputNoShared})
	if err != nil {
		t.Fatal(err)
	}
	r, err := a.Capture(ctx, dw.CaptureRequest{Kind: "window_content", Target: ref, MaxPixelWidth: 2, MaxPixelHeight: 2})
	if err != nil || len(r.Tiles) != 1 {
		t.Fatal(r, err)
	}
	tile := r.Tiles[0]
	if tile.Target != ref || tile.DesktopFrame != "" || tile.ImageToDesktop != (dw.Transform2D{}) || tile.ImageToTarget.A != 1 || tile.ImageToTarget.D != 1 {
		t.Fatal(tile)
	}
	if _, err = a.ReadAsset(ctx, tile.Asset); err != nil {
		t.Fatal(err)
	}
	for _, req := range []dw.CaptureRequest{{Kind: "visible_region", Target: ref}, {Kind: "window_content", Target: h.refs["Fixture"]}} {
		if _, err = a.Capture(ctx, req); code(err) != "permission_denied" {
			t.Fatal("scope broadened", req, err)
		}
	}
	for _, req := range []dw.CaptureRequest{{Kind: "window_content", Target: ref, Region: &dw.Bounds{}}, {Kind: "window_content", Target: ref, IncludeCursor: true}, {Kind: "window_content", Target: h.refs["内容"]}, {Kind: "window_content"}} {
		if _, err = h.a.Capture(ctx, req); code(err) != "invalid_argument" {
			t.Fatal(req, err)
		}
	}
	h.f.Remove("window")
	if _, err = a.Capture(ctx, dw.CaptureRequest{Kind: "window_content", Target: ref}); code(err) != "ref_gone" {
		t.Fatal(err)
	}
}

func TestWindowCaptureFailureNeverReturnsAnAsset(t *testing.T) {
	h := setup(t)
	for _, failure := range []string{"seat_unavailable", "capture_timeout", "capture_frame_unavailable", "window_unavailable", "window_not_visible", "capture_geometry_changed"} {
		h.f.SetCaptureFault(dw.NewFault(failure, "injected native failure", "reobserve"))
		r, err := h.a.Capture(ctx, dw.CaptureRequest{Kind: "window_content", Target: h.refs["Desktop World Fixture"]})
		if code(err) != failure || len(r.Tiles) != 0 {
			t.Fatal(failure, r, err)
		}
	}
}

func TestWindowCaptureAuthorizationAndRevocation(t *testing.T) {
	h := setup(t)
	ref := h.refs["Desktop World Fixture"]
	checks := 0
	a, err := h.w.NewActor(ctx, dw.ActorConfig{ID: "capture-policy", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: allOps, Authorizer: authFunc(func(_ context.Context, in dw.Intent) (dw.Decision, error) {
		if in.Operation == "capture" {
			checks++
			return dw.Decision{Allow: in.CaptureKind == "window_content" && checks < 3}, nil
		}
		return dw.Decision{Allow: true}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	r, err := a.Capture(ctx, dw.CaptureRequest{Kind: "window_content", Target: ref})
	if code(err) != "permission_denied" || len(r.Tiles) != 0 || checks != 3 {
		t.Fatal("authorization not rechecked after native delivery", r, err, checks)
	}
	if _, err = a.Capture(ctx, dw.CaptureRequest{Kind: "visible_region"}); code(err) != "permission_denied" {
		t.Fatal(err)
	}
	good, err := h.a.Capture(ctx, dw.CaptureRequest{Kind: "window_content", Target: ref})
	if err != nil {
		t.Fatal(err)
	}
	h.f.SetPermission("screen_capture", "denied")
	if _, err = h.a.ReadAsset(ctx, good.Tiles[0].Asset); code(err) != "asset_expired" {
		t.Fatal("revoked capture asset remained readable", err)
	}
}

func TestCaptureInventoryIsOptInAndFresh(t *testing.T) {
	h := setup(t)
	for _, mode := range []string{"cached", "max_age"} {
		_, err := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionCaptureWindows, Freshness: dw.Freshness{Mode: mode}})
		if code(err) != "invalid_argument" {
			t.Fatal(mode, err)
		}
	}
	ob, err := h.a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionCaptureWindows})
	if err != nil || len(ob.Objects) != 1 || ob.Objects[0].Kind != dw.KindWindow || ob.Objects[0].Bounds.Value != nil || ob.Objects[0].ValuePreview.Value != nil || len(ob.Objects[0].Capabilities) > 0 {
		t.Fatal(ob, err)
	}
}
