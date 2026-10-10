package exactgrant

import "testing"

func TestExactWindowIdentityNeverWidensToAppOrTitle(t *testing.T) {
	first := Identity{PID: 41001, ProcessStart: 27, NativeWindowID: 111,
		ApplicationRef: "app-a", AXWindowRef: "ax-window-a"}
	grant, err := Bind(first, first)
	if err != nil {
		t.Fatal(err)
	}
	inside := Target{ApplicationRef: "app-a", AXWindowRef: "ax-window-a", NativeWindowID: 111}
	if err := grant.Check(first, []Target{inside}); err != nil {
		t.Fatal(err)
	}
	for name, targets := range map[string][]Target{
		"sibling same App and title": {{ApplicationRef: "app-a", AXWindowRef: "ax-window-b", NativeWindowID: 222}},
		"different App":              {{ApplicationRef: "app-b", AXWindowRef: "ax-window-a", NativeWindowID: 111}},
		"missing window":             {{ApplicationRef: "app-a"}},
		"mixed plan":                 {inside, {ApplicationRef: "app-a", AXWindowRef: "ax-window-b", NativeWindowID: 222}},
		"no targets":                 nil,
	} {
		if err := grant.Check(first, targets); err == nil {
			t.Errorf("%s was allowed", name)
		}
	}
	for name, live := range map[string]Identity{
		"reused PID":             {PID: 41001, ProcessStart: 28, NativeWindowID: 111, ApplicationRef: "app-a", AXWindowRef: "ax-window-a"},
		"replacement same title": {PID: 41001, ProcessStart: 27, NativeWindowID: 222, ApplicationRef: "app-a", AXWindowRef: "ax-window-b"},
		"closed":                 {},
		"AX mapping lost":        {PID: 41001, ProcessStart: 27, NativeWindowID: 111, ApplicationRef: "app-a"},
	} {
		if err := grant.Check(live, []Target{inside}); err == nil {
			t.Errorf("%s was allowed", name)
		}
	}
	if _, err := Bind(first, Identity{PID: 41001, ProcessStart: 27,
		NativeWindowID: 222, ApplicationRef: "app-a", AXWindowRef: "ax-window-b"}); err == nil {
		t.Fatal("title-selected native window was joined to a different AX window")
	}
}
