package grantcandidate

import (
	"testing"

	dw "github.com/caelis-labs/desktop-world"
)

func TestResolveRequiresCompleteScopedStableNativeIdentity(t *testing.T) {
	appName, title := "Owned Test App", "Owned Test Window"
	known := func(s string) dw.Fact[string] { return dw.Fact[string]{Status: dw.FactKnown, Value: &s} }
	apps := dw.Observation{Coverage: dw.Coverage{Scope: dw.Scope{Desktop: true}, Complete: true},
		Objects: []dw.Object{{Ref: "app-1", Kind: dw.KindApplication, Lifecycle: dw.LifeLive, Name: known(appName)}}}
	window := dw.Object{Ref: "native-window-1", Kind: dw.KindWindow, Role: "capture_window", App: "app-1", Lifecycle: dw.LifeLive, Name: known(title)}
	first := dw.Observation{Coverage: dw.Coverage{Scope: dw.Scope{Refs: []dw.Ref{"app-1"}}, Complete: true}, Objects: []dw.Object{window}}
	second := first
	if got, state := Resolve(appName, title, apps, first, second); state != "candidate_only" || got.Application != "app-1" || got.CaptureWindow != "native-window-1" {
		t.Fatalf("complete stable scoped identity rejected: state=%s candidate=%+v", state, got)
	}
	check := func(label string, a, b, c dw.Observation, want string) {
		t.Helper()
		got, state := Resolve(appName, title, a, b, c)
		if state != want || got != (Candidate{}) {
			t.Fatalf("%s: state=%s candidate=%+v, want %s", label, state, got, want)
		}
	}
	partialApps := apps
	partialApps.Coverage.Complete = false
	partialApps.Objects = nil
	check("incomplete zero app match", partialApps, first, second, "unresolved")
	dirtyApps := apps
	dirtyApps.Coverage.Dirty = true
	check("dirty positive app match", dirtyApps, first, second, "unresolved")
	partialWindow := first
	partialWindow.Coverage.Complete = false
	partialWindow.Objects = nil
	check("incomplete zero window match", apps, partialWindow, second, "unresolved")
	wrongOwner := first
	wrongOwner.Objects = []dw.Object{window}
	wrongOwner.Objects[0].App = "app-2"
	check("different process owner", apps, wrongOwner, second, "owner_unresolved")
	duplicate := first
	duplicate.Objects = []dw.Object{window, window}
	duplicate.Objects[1].Ref = "native-window-2"
	check("same title duplicate", apps, duplicate, second, "ambiguous_window")
	replaced := first
	replaced.Objects = []dw.Object{window}
	replaced.Objects[0].Ref = "native-window-new"
	check("window replaced between reads", apps, first, replaced, "identity_changed")
}
