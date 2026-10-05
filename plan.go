package desktopworld

import "fmt"

// PlanBuilder records steps locally. Only Actor.Execute or a client Act submits them.
// No foreground is held while building; execution does not roll back delivered input.
type PlanBuilder struct{ Steps []Step }

func NewPlan() *PlanBuilder { return &PlanBuilder{} }
func (p *PlanBuilder) Add(s Step) *PlanBuilder {
	if s.ID == "" {
		s.ID = fmt.Sprintf("s%d", len(p.Steps)+1)
	}
	p.Steps = append(p.Steps, s)
	return p
}
func (p *PlanBuilder) Focus(ref Ref) *PlanBuilder {
	return p.Add(Step{Op: "focus", Target: Target{Ref: ref}})
}
func (p *PlanBuilder) BindFocus(name string, within Ref) *PlanBuilder {
	return p.Add(Step{Op: "bind_focus", BindFocus: &BindFocus{Name: name, Within: within}})
}
func (p *PlanBuilder) Press(target Target, key string, modifiers ...string) *PlanBuilder {
	return p.Add(Step{Op: "keyboard.press", Target: target, Press: &KeyChord{Key: key, Modifiers: modifiers}})
}
func (p *PlanBuilder) Type(target Target, text string) *PlanBuilder {
	return p.Add(Step{Op: "keyboard.type_text", Target: target, TypeText: &TypeText{Text: text}})
}
func (p *PlanBuilder) Set(ref Ref, text string) *PlanBuilder {
	return p.Add(Step{Op: "set_value", Target: Target{Ref: ref}, SetValue: &SetValue{Text: text}})
}
func (p *PlanBuilder) Invoke(ref Ref) *PlanBuilder {
	return p.Add(Step{Op: "invoke", Target: Target{Ref: ref}})
}
func (p *PlanBuilder) Build(epoch Epoch, id RequestID) Plan {
	return Plan{Epoch: epoch, RequestID: id, Steps: append([]Step(nil), p.Steps...)}
}
