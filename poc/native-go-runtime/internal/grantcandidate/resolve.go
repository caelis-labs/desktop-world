// Package grantcandidate holds a read-only proof of a directed native window
// candidate. A candidate is not an authorization grant or an AX click target.
package grantcandidate

import dw "github.com/caelis-labs/desktop-world"

type Candidate struct {
	Application   dw.Ref
	CaptureWindow dw.Ref
}

// Resolve requires a complete application inventory followed by two complete
// observations scoped to one application. Capture-window Refs bind native
// process/window identity, but cannot be used as AX action Refs.
func Resolve(appName, title string, apps, first, second dw.Observation) (Candidate, string) {
	if appName == "" || title == "" || !apps.Coverage.Scope.Desktop || !clean(apps.Coverage) {
		return Candidate{}, "unresolved"
	}
	var application dw.Ref
	for _, o := range apps.Objects {
		if o.Kind != dw.KindApplication || o.Lifecycle != dw.LifeLive || !named(o, appName) {
			continue
		}
		if application != "" && application != o.Ref {
			return Candidate{}, "ambiguous_app"
		}
		application = o.Ref
	}
	if application == "" {
		return Candidate{}, "pending_app"
	}
	win1, reason := exactWindow(application, title, first)
	if reason != "" {
		return Candidate{}, reason
	}
	win2, reason := exactWindow(application, title, second)
	if reason != "" {
		return Candidate{}, reason
	}
	if win1 != win2 {
		return Candidate{}, "identity_changed"
	}
	return Candidate{Application: application, CaptureWindow: win1}, "candidate_only"
}

func clean(c dw.Coverage) bool {
	return c.Complete && !c.Dirty && !c.Truncated && c.Continuation == "" && len(c.UnavailableSources) == 0
}
func named(o dw.Object, name string) bool {
	return o.Name.Status == dw.FactKnown && o.Name.Value != nil && *o.Name.Value == name
}
func exactWindow(app dw.Ref, title string, ob dw.Observation) (dw.Ref, string) {
	if ob.Coverage.Scope.Desktop || len(ob.Coverage.Scope.Refs) != 1 || ob.Coverage.Scope.Refs[0] != app || !clean(ob.Coverage) {
		return "", "unresolved"
	}
	var found dw.Ref
	for _, o := range ob.Objects {
		if o.Kind != dw.KindWindow || o.Role != "capture_window" || o.Lifecycle != dw.LifeLive || !named(o, title) {
			continue
		}
		if o.App != app || o.Ref == "" {
			return "", "owner_unresolved"
		}
		if found != "" && found != o.Ref {
			return "", "ambiguous_window"
		}
		found = o.Ref
	}
	if found == "" {
		return "", "pending_window"
	}
	return found, ""
}
