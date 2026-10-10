package helper

import (
	"context"
	"encoding/json"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
	"testing"
	"time"
)

func TestDeferredDeclarationAndInstanceExpiry(t *testing.T) {
	s, f := setup(t, Config{WriteApps: []string{"Later"}})
	status, _ := s.grantStatus("session")
	if status.Grants[0].State != "pending" {
		t.Fatal(status)
	}
	f.Add(dwtest.Node{ID: "later", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Later")}})
	f.Add(dwtest.Node{ID: "later-button", App: "later", Parent: "later", Object: dw.Object{Kind: dw.KindUI, Role: "button", Name: dw.Known("Later button")}})
	r := refs(t, s)
	if out := call(s, "first", "act", invoke(r["Later button"])); out.Error != nil {
		t.Fatal(out.Error)
	}
	status, _ = s.grantStatus("session")
	old := status.Grants[0].Application
	f.Remove("later-button")
	f.Remove("later")
	s.refreshGrants(context.Background(), "session")
	f.Add(dwtest.Node{ID: "later", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Later")}})
	f.Add(dwtest.Node{ID: "later-button", App: "later", Parent: "later", Object: dw.Object{Kind: dw.KindUI, Role: "button", Name: dw.Known("Later button")}})
	r = refs(t, s)
	if out := call(s, "new-instance", "act", invoke(r["Later button"])); out.Error == nil {
		t.Fatal("restart inherited grant")
	}
	status, _ = s.grantStatus("session")
	if status.Grants[0].Application != old || status.Grants[0].State != "expired" {
		t.Fatal(status)
	}
}
func TestIncompleteInventoryCannotAuthorizeUniqueObservedApp(t *testing.T) {
	s, f := setup(t, Config{})
	f.SetIncomplete(true)
	if err := s.declare(ControlRequest{Turn: "session", Name: "Fixture"}); err != nil {
		t.Fatal(err)
	}
	r := refs(t, s)
	if out := call(s, "incomplete", "act", invoke(r["提交"])); out.Error == nil {
		t.Fatal("incomplete scope authorized")
	}
	status, _ := s.grantStatus("session")
	if status.Grants[0].State != "unresolved" {
		t.Fatal(status)
	}
	f.SetIncomplete(false)
	if out := call(s, "complete", "act", invoke(r["提交"])); out.Error != nil {
		t.Fatal(out.Error)
	}
}
func TestDynamicGrantRevokeKeepsOtherGrantAndReceipts(t *testing.T) {
	if pocCoreWithoutGrants(Config{Managed: true}) {
		t.Skip("grant contract does not apply to the core POC helper")
	}
	s, f := setup(t, Config{Managed: true})
	control(t, s, "begin_turn", "t1", "")
	f.Add(dwtest.Node{ID: "other", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Other")}})
	f.Add(dwtest.Node{ID: "other-button", App: "other", Parent: "other", Object: dw.Object{Kind: dw.KindUI, Role: "button", Name: dw.Known("Other button")}})
	r := managedRefs(t, s, "t1")
	control(t, s, "grant", "t1", r["Fixture"])
	control(t, s, "grant", "t1", r["Other"])
	first := managedCall(s, "t1", "first", "act", invoke(r["提交"]))
	if first.Error != nil {
		t.Fatal(first.Error)
	}
	control(t, s, "revoke", "t1", r["Fixture"])
	if out := managedCall(s, "t1", "revoked", "act", invoke(r["提交"])); out.Error == nil {
		t.Fatal("revoked grant used")
	}
	if out := managedCall(s, "t1", "other", "act", invoke(r["Other button"])); out.Error != nil {
		t.Fatal(out.Error)
	}
	same := managedCall(s, "t1", "first", "act", invoke(r["提交"]))
	if same.Error != nil {
		t.Fatal("lost original receipt", same.Error)
	}
	if len(f.Events()) != 2 {
		t.Fatal("replayed effects", f.Events())
	}
}
func TestBindFocusUsesExecutionTimeScopedFocus(t *testing.T) {
	s, f := setup(t, Config{WriteApps: []string{"Fixture"}})
	r := refs(t, s)
	f.SetFocus("field")
	plan := dw.NewPlan().BindFocus("input", r["Desktop World Fixture"]).Set(r["内容"], "hello")
	out := call(s, "bind-focus", "act", func() any { b, _ := protocol.Marshal(struct{ Steps []dw.Step }{plan.Steps}); return json.RawMessage(b) }())
	if out.Error != nil {
		t.Fatal(out.Error)
	}
	f.Add(dwtest.Node{ID: "foreign", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Foreign")}})
	f.Add(dwtest.Node{ID: "foreign-window", App: "foreign", Parent: "foreign", Object: dw.Object{Kind: dw.KindWindow}})
	f.Add(dwtest.Node{ID: "foreign-field", App: "foreign", Window: "foreign-window", Parent: "foreign-window", Object: dw.Object{Kind: dw.KindUI, Role: "text_field"}})
	f.SetFocus("foreign-field")
	if out := call(s, "wrong-focus", "act", func() any { b, _ := protocol.Marshal(struct{ Steps []dw.Step }{plan.Steps}); return json.RawMessage(b) }()); out.Error == nil {
		t.Fatal("out-of-scope focus accepted")
	}
}

func TestRevocationCancelsInflightAndAllBoundAliases(t *testing.T) {
	if pocCoreWithoutGrants(Config{Managed: true}) {
		t.Skip("grant contract does not apply to the core POC helper")
	}
	s, f := setup(t, Config{Managed: true})
	control(t, s, "begin_turn", "t1", "")
	r := managedRefs(t, s, "t1")
	control(t, s, "grant", "t1", r["Fixture"])
	if out := s.Control(context.Background(), ControlRequest{ID: "alias", Op: "declare", Turn: "t1", Name: "Fixture"}); out.Error != nil {
		t.Fatal(out.Error)
	}
	s.refreshGrants(context.Background(), "t1")
	started := make(chan struct{})
	f.Enqueue("invoke", dwtest.Behavior{Before: func(*dwtest.Fixture) { close(started) }, Block: make(chan struct{})})
	done := make(chan Response, 1)
	go func() { done <- managedCall(s, "t1", "inflight", "act", invoke(r["提交"])) }()
	select {
	case <-started:
	case <-time.After(3 * time.Second):
		t.Fatal("input never started")
	}
	status, _ := s.grantStatus("t1")
	out := s.Control(context.Background(), ControlRequest{ID: "revoke-alias", Op: "revoke", Turn: "t1", GrantID: status.Grants[0].ID})
	if out.Error != nil {
		t.Fatal(out.Error)
	}
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("revocation did not cancel input")
	}
	status, _ = s.grantStatus("t1")
	for _, g := range status.Grants {
		if g.State != "revoked" {
			t.Fatal("alias reports false active authority", g)
		}
	}
	if len(f.Events()) != 0 {
		t.Fatal("revoked call delivered input", f.Events())
	}
	if result := managedCall(s, "t1", "after-revoke", "act", invoke(r["提交"])); result.Error == nil {
		t.Fatal("new input allowed")
	}
}
