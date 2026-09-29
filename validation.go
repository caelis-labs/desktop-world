package desktopworld

import (
	"fmt"
	"math"
	"strings"
	"time"
	"unicode/utf8"
)

// Known retains explicit zero values, including false and empty strings.
func Known[T any](v T) Fact[T] {
	return Fact[T]{Status: FactKnown, Value: &v, Source: "ui_content", SampledAt: time.Now().UTC()}
}
func Unknown[T any]() Fact[T] { return Fact[T]{Status: FactUnknown} }
func (f Fact[T]) Validate() error {
	switch f.Status {
	case FactUnrequested, FactUnknown, FactUnsupported, FactRedacted:
		if f.Value != nil {
			return Invalid("non-known fact has a value")
		}
	case FactKnown:
		if f.Value == nil {
			return Invalid("known fact needs a value")
		}
	default:
		return Invalid("unknown fact status")
	}
	return nil
}
func NewFault(code, message, retry string) *Fault {
	return &Fault{Code: code, Message: message, RetryClass: retry}
}
func Invalid(message string) *Fault {
	return NewFault("invalid_argument", message, "never_automatically")
}
func Finite(v float64) bool { return !math.IsNaN(v) && !math.IsInf(v, 0) }
func (s Scope) Validate() error {
	if s.Desktop == (len(s.Refs) > 0) {
		return Invalid("scope requires desktop or nonempty refs")
	}
	seen := map[Ref]bool{}
	for _, r := range s.Refs {
		if r == "" || seen[r] {
			return Invalid("empty or duplicate scope ref")
		}
		seen[r] = true
	}
	return nil
}
func (t Target) Validate() error {
	n := 0
	if t.Ref != "" {
		n++
	}
	if t.Bound != "" {
		n++
	}
	if t.Point != nil {
		n++
		p := t.Point
		if p.Frame == "" || p.Topology == 0 || !Finite(p.X) || !Finite(p.Y) || math.Abs(p.X) > 1e7 || math.Abs(p.Y) > 1e7 {
			return Invalid("invalid point")
		}
	}
	if t.Anchor != nil {
		n++
		a := t.Anchor
		if a.Target == "" || !Finite(a.U) || !Finite(a.V) || a.U < 0 || a.U > 1 || a.V < 0 || a.V > 1 {
			return Invalid("invalid anchor")
		}
	}
	if n != 1 {
		return Invalid("target requires exactly one arm")
	}
	return nil
}
func (l Locator) Validate() error {
	if l.Within == "" {
		return Invalid("locator requires within")
	}
	if l.Kind != "" && l.Kind != KindApplication && l.Kind != KindWindow && l.Kind != KindUI {
		return Invalid("unknown kind")
	}
	if l.MaxDepth < 0 || l.MaxDepth > 32 {
		return Invalid("invalid locator depth")
	}
	if len(l.Role) > 128 || len(l.RequiredStates) > 16 {
		return Invalid("locator too large")
	}
	for _, v := range []*string{l.NameEquals, l.NameContains} {
		if v != nil && (!utf8.ValidString(*v) || len(*v) > 4096) {
			return Invalid("invalid locator text")
		}
	}
	for k := range l.RequiredStates {
		if !ValidState(k) {
			return Invalid("unknown state")
		}
	}
	return nil
}
func ValidState(s string) bool {
	switch s {
	case "enabled", "focused", "selected", "checked", "expanded", "offscreen", "read_only", "protected":
		return true
	}
	return false
}
func (p Predicate) Validate() error {
	if e := p.Target.Validate(); e != nil {
		return e
	}
	if p.Target.Point != nil || p.Target.Anchor != nil {
		return Invalid("predicate needs object target")
	}
	n := 0
	if p.EqualsBool != nil {
		n++
	}
	if p.EqualsString != nil {
		n++
		if len(*p.EqualsString) > 65536 || !utf8.ValidString(*p.EqualsString) {
			return Invalid("predicate text too large")
		}
	}
	if p.EqualsVersion != nil {
		n++
	}
	if n != 1 {
		return Invalid("predicate requires one equality")
	}
	switch p.Property {
	case "name", "value", "role", "lifecycle":
		if p.EqualsString == nil {
			return Invalid("string equality required")
		}
	case "version", "geometry_version":
		if p.EqualsVersion == nil {
			return Invalid("version equality required")
		}
	case "exists", "foreground":
		if p.EqualsBool == nil {
			return Invalid("bool equality required")
		}
	default:
		if !ValidState(p.Property) || p.EqualsBool == nil {
			return Invalid("unknown predicate property or type")
		}
	}
	return nil
}
func IsWrite(op string) bool {
	switch op {
	case "focus", "invoke", "set_value", "pointer.move", "pointer.click", "pointer.drag", "pointer.scroll", "keyboard.type_text", "keyboard.press":
		return true
	}
	return false
}
func ValidOperation(op string) bool {
	return IsWrite(op) || op == "observe" || op == "read" || op == "sync" || op == "capture" || op == "read_asset" || op == "resolve_anchor" || op == "bind" || op == "wait" || op == "raw_input"
}
func (p Plan) Validate() error {
	if p.Epoch == "" || !strings.HasPrefix(string(p.RequestID), string(p.Epoch)+":") || len(p.RequestID) > 256 {
		return Invalid("request_id must have current epoch prefix")
	}
	if len(p.Steps) == 0 || len(p.Steps) > 16 {
		return Invalid("plan needs 1..16 steps")
	}
	if p.Timeout < 0 || p.Timeout > 10*time.Second {
		return Invalid("plan timeout exceeds 10 seconds")
	}
	ids := map[string]bool{}
	aliases := map[string]bool{}
	check := func(t Target) error {
		if e := t.Validate(); e != nil {
			return e
		}
		if t.Bound != "" && !aliases[t.Bound] {
			return Invalid("alias used before binding")
		}
		return nil
	}
	for _, s := range p.Steps {
		if s.ID == "" || len(s.ID) > 128 || ids[s.ID] {
			return Invalid("empty or duplicate step id")
		}
		ids[s.ID] = true
		if s.Timeout < 0 || s.Timeout > 10*time.Second {
			return Invalid("invalid step timeout")
		}
		if s.Completion != "" && s.Completion != "verify" && s.Completion != "dispatch" {
			return Invalid("unknown completion")
		}
		arms := 0
		for _, b := range []bool{s.Bind != nil, s.SetValue != nil, s.TypeText != nil, s.Press != nil, s.Click != nil, s.Drag != nil, s.Scroll != nil} {
			if b {
				arms++
			}
		}
		need := 0
		switch s.Op {
		case "bind":
			need = 1
			if s.Bind == nil || s.Bind.Name == "" || len(s.Bind.Name) > 128 || !s.Bind.RequireUnique || aliases[s.Bind.Name] {
				return Invalid("invalid or duplicate binding")
			}
			if e := s.Bind.Locator.Validate(); e != nil {
				return e
			}
			if s.Target != (Target{}) {
				return Invalid("bind cannot have target")
			}
		case "wait":
			if len(s.After) == 0 {
				return Invalid("wait needs predicates")
			}
			if s.Target != (Target{}) {
				return Invalid("wait uses predicate targets")
			}
		case "focus", "invoke", "pointer.move":
		case "set_value":
			need = 1
			if s.SetValue == nil {
				return Invalid("set_value arguments required")
			}
		case "keyboard.type_text":
			need = 1
			if s.TypeText == nil {
				return Invalid("type_text arguments required")
			}
		case "keyboard.press":
			need = 1
			if s.Press == nil {
				return Invalid("press arguments required")
			}
			if e := validateChord(*s.Press); e != nil {
				return e
			}
		case "pointer.click":
			need = 1
			if s.Click == nil || (s.Click.Count != 1 && s.Click.Count != 2) || (s.Click.Button != "left" && s.Click.Button != "right" && s.Click.Button != "middle") {
				return Invalid("invalid click")
			}
		case "pointer.drag":
			need = 1
			if s.Drag == nil || s.Drag.Duration < 0 || s.Drag.Duration > 2*time.Second {
				return Invalid("invalid drag")
			}
			if e := check(s.Drag.To); e != nil {
				return e
			}
		case "pointer.scroll":
			need = 1
			if s.Scroll == nil || s.Scroll.Unit != "wheel_step" || !Finite(s.Scroll.DX) || !Finite(s.Scroll.DY) || math.Abs(s.Scroll.DX) > 100 || math.Abs(s.Scroll.DY) > 100 {
				return Invalid("invalid scroll")
			}
		default:
			return Invalid("unknown step operation")
		}
		if arms != need {
			return Invalid("operation argument union mismatch")
		}
		if IsWrite(s.Op) {
			if e := check(s.Target); e != nil {
				return e
			}
			if !strings.HasPrefix(s.Op, "pointer.") && (s.Target.Point != nil || s.Target.Anchor != nil) {
				return Invalid("operation requires object target")
			}
			if s.Completion == "verify" && s.Op != "focus" && s.Op != "set_value" && len(s.After) == 0 {
				return Invalid("verify requires after predicates")
			}
		}
		if len(s.Before)+len(s.After) > 32 {
			return Invalid("too many predicates")
		}
		for _, pr := range append(append([]Predicate{}, s.Before...), s.After...) {
			if e := pr.Validate(); e != nil {
				return e
			}
			if e := check(pr.Target); e != nil {
				return e
			}
		}
		for _, txt := range []string{func() string {
			if s.SetValue != nil {
				return s.SetValue.Text
			}
			return ""
		}(), func() string {
			if s.TypeText != nil {
				return s.TypeText.Text
			}
			return ""
		}()} {
			if !utf8.ValidString(txt) || len(txt) > 65536 || strings.ContainsRune(txt, 0) {
				return Invalid("text must be valid bounded Unicode without NUL")
			}
		}
		if s.Bind != nil {
			aliases[s.Bind.Name] = true
		}
	}
	return nil
}
func validateChord(k KeyChord) error {
	allowed := map[string]bool{"Enter": true, "Tab": true, "Escape": true, "Backspace": true, "Delete": true, "Space": true, "Left": true, "Right": true, "Up": true, "Down": true, "Home": true, "End": true, "PageUp": true, "PageDown": true}
	if !(len(k.Key) == 1 && ((k.Key[0] >= 'A' && k.Key[0] <= 'Z') || (k.Key[0] >= '0' && k.Key[0] <= '9'))) && !allowed[k.Key] {
		return Invalid("unsupported key")
	}
	m := map[string]bool{}
	for _, v := range k.Modifiers {
		if m[v] {
			return Invalid("duplicate modifier")
		}
		switch v {
		case "control", "alt", "shift", "meta", "primary":
		default:
			return Invalid("unsupported modifier")
		}
		m[v] = true
	}
	if m["primary"] && (m["meta"] || m["control"]) {
		return Invalid("primary cannot be combined with control or meta")
	}
	return nil
}
func (r ObserveRequest) Validate() error {
	if e := r.Scope.Validate(); e != nil {
		return e
	}
	switch r.Projection {
	case "", ProjectionSummary, ProjectionOutline, ProjectionDetail:
	default:
		return Invalid("unknown projection")
	}
	switch r.Freshness.Mode {
	case "", "cached", "max_age", "refresh":
	default:
		return Invalid("unknown freshness")
	}
	if r.Freshness.MaxAge < 0 || r.Freshness.MaxAge > time.Hour {
		return Invalid("invalid max age")
	}
	b := r.Budget
	if b.MaxResults < 0 || b.MaxResults > 1024 || b.MaxDepth < 0 || b.MaxDepth > 32 || b.MaxVisitedNodes < 0 || b.MaxVisitedNodes > 10000 || b.MaxOutputBytes < 0 || b.MaxOutputBytes > 1<<20 || b.MaxTextRunes < 0 || b.MaxTextRunes > 4096 || b.ReadDeadline < 0 || b.ReadDeadline > 10*time.Second {
		return Invalid("budget out of range")
	}
	allowed := map[string]bool{"role": true, "kind": true, "app": true, "window": true, "parent": true, "relations": true, "name": true, "value_preview": true, "states": true, "bounds": true, "capabilities": true, "lifecycle": true}
	seen := map[string]bool{}
	for _, f := range r.Fields {
		if !allowed[f] || seen[f] {
			return Invalid(fmt.Sprintf("invalid field %q", f))
		}
		seen[f] = true
	}
	if r.Match != nil {
		return r.Match.Validate()
	}
	return nil
}
