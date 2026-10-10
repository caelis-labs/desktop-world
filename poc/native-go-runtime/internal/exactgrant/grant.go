// Package exactgrant is an isolated POC for the identity checks made at the
// dispatch boundary by the build-tagged exacthelper. The normal helper is
// unchanged while the full runtime POC remains behind its phase gate.
package exactgrant

import "errors"

// Identity is produced by trusted native discovery, never by a script. A
// window number is meaningful only for this process instance.
type Identity struct {
	PID            int32
	ProcessStart   uint64
	NativeWindowID uint32
	ApplicationRef string
	AXWindowRef    string
}

func (v Identity) valid() bool {
	return v.PID > 0 && v.ProcessStart > 0 && v.NativeWindowID > 0 &&
		v.ApplicationRef != "" && v.AXWindowRef != ""
}

// Grant is immutable. Title is purposefully absent: a rename or another
// window with the same title cannot change which native window was approved.
type Grant struct{ identity Identity }

func Bind(approved, axWindow Identity) (Grant, error) {
	if !approved.valid() || approved != axWindow {
		return Grant{}, errors.New("native and AX window identity unresolved")
	}
	return Grant{identity: approved}, nil
}

// Target is the trusted per-action mapping of a Ref, resolved by the helper.
// A missing owning window is unknown and does not inherit App-wide authority.
type Target struct {
	ApplicationRef string
	AXWindowRef    string
	NativeWindowID uint32
}

// Check is called before every step, including each step in a multi-step
// plan. live is a fresh OS snapshot, not a cached title or geometry match.
// A mismatch denies the action; it never rebinds to a replacement window.
func (g Grant) Check(live Identity, targets []Target) error {
	if !g.identity.valid() || !live.valid() || live != g.identity {
		return errors.New("approved window expired or unresolved")
	}
	if len(targets) == 0 {
		return errors.New("action target unresolved")
	}
	for _, target := range targets {
		if target.ApplicationRef != g.identity.ApplicationRef ||
			target.AXWindowRef != g.identity.AXWindowRef ||
			target.NativeWindowID != g.identity.NativeWindowID {
			return errors.New("action target outside approved window")
		}
	}
	return nil
}
