package contract_test

import (
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/testutil"
	"testing"
)

func TestPartialRefreshPreservesUnrequestedFactsAndTheirTimes(t *testing.T) {
	h := setup(t)
	ref := h.refs["内容"]
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{ref}}, Projection: dw.ProjectionDetail, Fields: []string{"name", "value_preview", "bounds", "states", "capabilities"}}
	full, err := h.a.Observe(ctx, req)
	if err != nil || len(full.Objects) != 1 {
		t.Fatal(full, err)
	}
	old := full.Objects[0]
	h.f.Update("field", func(n *dwtest.Node) {
		n.Object.Name = dw.Known("renamed")
		n.Object.ValuePreview = dw.Known("new value unseen")
		n.Object.Bounds = dw.Unknown[dw.Bounds]()
		n.Object.States["enabled"] = dw.Known(false)
	})
	req.Fields = []string{"name"}
	partial, err := h.a.Observe(ctx, req)
	if err != nil {
		t.Fatal(err)
	}
	if partial.Objects[0].ValuePreview.Status != dw.FactUnrequested {
		t.Fatal("unrequested value disclosed")
	}
	req.Fields = []string{"name", "value_preview", "bounds", "states", "capabilities"}
	req.Freshness.Mode = "cached"
	cached, err := h.a.Observe(ctx, req)
	if err != nil {
		t.Fatal(err)
	}
	got := cached.Objects[0]
	if !got.ValuePreview.SampledAt.Equal(old.ValuePreview.SampledAt) || !got.Bounds.SampledAt.Equal(old.Bounds.SampledAt) || got.States["enabled"].Value == nil || !*got.States["enabled"].Value || got.Name.Value == nil || *got.Name.Value != "renamed" {
		t.Fatalf("partial refresh altered unrequested facts: %+v", got)
	}
	// A full fresh read before writing still rejects the now-disabled target.
	_, err = h.a.Execute(ctx, h.plan("fresh-disabled", dw.Step{ID: "write", Op: "set_value", Target: target(ref), SetValue: &dw.SetValue{Text: "never"}}))
	if code(err) != "capability_unavailable" || len(h.f.Events()) != 0 {
		t.Fatal(err, h.f.Events())
	}
}
func TestMatchReadsHiddenFieldsWithoutDisclosingThem(t *testing.T) {
	h := setup(t)
	name := "内容"
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{h.refs["Desktop World Fixture"]}}, Projection: dw.ProjectionOutline, Fields: []string{"role"}, Match: &dw.Locator{Within: h.refs["Desktop World Fixture"], NameEquals: &name, RequiredStates: map[string]bool{"enabled": true}, RequiredCapability: "set_value"}}
	ob, err := h.a.Observe(ctx, req)
	if err != nil || len(ob.Objects) != 1 || ob.Objects[0].Ref != h.refs["内容"] || ob.Objects[0].Name.Status != "" || len(ob.Objects[0].States) != 0 || len(ob.Objects[0].Capabilities) != 0 {
		t.Fatal(ob, err)
	}
}

func TestPartialProjectionDoesNotDiscloseCachedRelations(t *testing.T) {
	h := setup(t)
	ref := h.refs["内容"]
	h.f.Update("field", func(n *dwtest.Node) { n.Object.Relations = []dw.Relation{{Name: "related", Target: h.refs["提交"]}} })
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{ref}}, Projection: dw.ProjectionDetail, Fields: []string{"relations"}}
	ob, err := h.a.Observe(ctx, req)
	if err != nil || len(ob.Objects[0].Relations) != 1 {
		t.Fatal(ob, err)
	}
	req.Fields = []string{"role"}
	ob, err = h.a.Observe(ctx, req)
	if err != nil || len(ob.Objects[0].Relations) != 0 {
		t.Fatal("unrequested cached relations disclosed", ob, err)
	}
}

func TestPartialProtectedRefreshRedactsPreviouslyCachedValueAndURI(t *testing.T) {
	h := setup(t)
	ref := h.refs["内容"]
	h.f.Update("field", func(n *dwtest.Node) { n.Object.URI = dw.Known("fixture://private") })
	req := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{ref}}, Projection: dw.ProjectionDetail, Fields: []string{"value_preview", "uri", "states"}}
	if _, err := h.a.Observe(ctx, req); err != nil {
		t.Fatal(err)
	}
	h.f.Update("field", func(n *dwtest.Node) {
		n.Object.States["protected"] = dw.Known(true)
		n.Object.ValuePreview = dw.Known("must never be disclosed")
	})
	req.Fields = []string{"value_preview"}
	ob, err := h.a.Observe(ctx, req)
	if err != nil || len(ob.Objects) != 1 || ob.Objects[0].ValuePreview.Status != dw.FactRedacted {
		t.Fatal(ob, err)
	}
	req.Fields = []string{"value_preview", "uri"}
	req.Freshness.Mode = "cached"
	ob, err = h.a.Observe(ctx, req)
	if err != nil || ob.Objects[0].ValuePreview.Status != dw.FactRedacted || ob.Objects[0].URI.Status != dw.FactRedacted {
		t.Fatal("partial protected sample left cached secrets", ob, err)
	}
}
