// Package dwtest provides an in-memory desktop with controlled failure injection.
// The fixture's event log is independent of engine receipts.
package dwtest

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"github.com/caelis-labs/desktop-world/internal/engine"
	"image"
	"image/color"
	"image/png"
	"sort"
	"sync"
	"time"
)

type Node struct {
	ID, Parent, App, Window string
	Object                  dw.Object
	Text                    string
	TextSource              string
}
type Event struct {
	Operation, Target, Text string
	At                      time.Time
}
type Behavior struct {
	Before             func(*Fixture)
	Block              <-chan struct{}
	IgnoreCancellation bool
	Delivery           dw.Delivery
	Fault              *dw.Fault
	Apply              bool
	Unsafe             bool
}
type Options struct {
	HistoryLimit, RequestLimit, ViewLimit int
	HistoryTTL, ReceiptTTL, PollInterval  time.Duration
	VerificationPollInterval              time.Duration
	SeatID                                string
}
type Fixture struct {
	mu         sync.Mutex
	nodes      map[backend.Key]Node
	ids        map[string]backend.Key
	next       int
	env        dw.Environment
	seat       backend.Seat
	events     []Event
	behaviors  map[string][]Behavior
	incomplete bool
	readFault  *dw.Fault
	slowDelay  time.Duration
	slowScans  map[string]*slowScan
	slowNext   int
}
type slowScan struct {
	keys    []backend.Key
	offset  int
	visited int
}

func New(ctx context.Context) (dw.World, *Fixture, error) { return NewWithOptions(ctx, Options{}) }
func NewWithOptions(ctx context.Context, o Options) (dw.World, *Fixture, error) {
	f := &Fixture{nodes: map[backend.Key]Node{}, ids: map[string]backend.Key{}, behaviors: map[string][]Behavior{}, env: dw.Environment{Platform: "fixture", Topology: 1, Displays: []dw.Display{{Ref: "display", Frame: "desktop", Bounds: dw.Rect{Width: 1920, Height: 1080}, Unit: "physical_pixel", ScaleX: 1, ScaleY: 1}}, Permissions: []dw.Permission{{Name: "accessibility", State: "granted"}, {Name: "input", State: "granted"}, {Name: "screen_capture", State: "granted"}}}, seat: backend.Seat{Health: "ready", Intervention: "supported"}}
	if o.SeatID == "" {
		o.SeatID = "fixture-" + time.Now().Format("150405.000000000")
	}
	w, e := engine.Open(ctx, f, engine.Options{HistoryLimit: o.HistoryLimit, RequestLimit: o.RequestLimit, ViewLimit: o.ViewLimit, HistoryTTL: o.HistoryTTL, ReceiptTTL: o.ReceiptTTL, PollInterval: o.PollInterval, VerificationPollInterval: o.VerificationPollInterval, SeatID: o.SeatID})
	return w, f, e
}
func (f *Fixture) Add(n Node) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if old := f.ids[n.ID]; old != "" {
		delete(f.nodes, old)
	}
	f.next++
	k := backend.Key(fmt.Sprintf("fixture-%d", f.next))
	f.ids[n.ID] = k
	n.Object.Lifecycle = dw.LifeLive
	if n.Object.States == nil {
		n.Object.States = map[string]dw.Fact[bool]{}
	}
	for key, v := range map[string]bool{"enabled": true, "read_only": false, "protected": false, "focused": false} {
		if _, ok := n.Object.States[key]; !ok {
			n.Object.States[key] = dw.Known(v)
		}
	}
	if n.Object.Bounds.Status == "" {
		n.Object.Bounds = dw.Known(dw.Bounds{Frame: "desktop", Topology: f.env.Topology, Rect: dw.Rect{X: 100, Y: 100, Width: 200, Height: 80}})
	}
	if len(n.Object.Capabilities) == 0 {
		for _, op := range []string{"focus", "invoke", "set_value"} {
			n.Object.Capabilities = append(n.Object.Capabilities, dw.Capability{Name: op, Support: "supported", Availability: "available"})
		}
	}
	f.nodes[k] = n
}
func (f *Fixture) Remove(id string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.nodes, f.ids[id])
	delete(f.ids, id)
}
func (f *Fixture) Update(id string, fn func(*Node)) {
	f.mu.Lock()
	defer f.mu.Unlock()
	k := f.ids[id]
	n, ok := f.nodes[k]
	if ok {
		fn(&n)
		f.nodes[k] = n
	}
}
func (f *Fixture) Enqueue(op string, b Behavior) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.behaviors[op] = append(f.behaviors[op], b)
}
func (f *Fixture) Events() []Event {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]Event{}, f.events...)
}
func (f *Fixture) SetReadFault(e *dw.Fault) { f.mu.Lock(); f.readFault = e; f.mu.Unlock() }
func (f *Fixture) SetIncomplete(v bool)     { f.mu.Lock(); f.incomplete = v; f.mu.Unlock() }

// SetSlowQuery enables a deterministic delayed traversal for cursor regression
// tests. It does not change ordinary fixture query behavior.
func (f *Fixture) SetSlowQuery(delay time.Duration) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.slowDelay = delay
	f.slowScans = map[string]*slowScan{}
}
func (f *Fixture) SetPermission(name, state string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for i, p := range f.env.Permissions {
		if p.Name == name {
			f.env.Permissions[i].State = state
		}
	}
}
func (f *Fixture) SetTopology(v dw.Version)    { f.mu.Lock(); f.env.Topology = v; f.mu.Unlock() }
func (f *Fixture) SetFocus(id string)          { f.mu.Lock(); defer f.mu.Unlock(); f.focus(f.ids[id]) }
func (f *Fixture) Open(context.Context) error  { return nil }
func (f *Fixture) Close(context.Context) error { return nil }
func (f *Fixture) Environment(context.Context) (dw.Environment, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	e := f.env
	e.Permissions = append([]dw.Permission{}, f.env.Permissions...)
	e.Displays = append([]dw.Display{}, f.env.Displays...)
	return e, nil
}
func (f *Fixture) Permissions(ctx context.Context, _ dw.PermissionRequest) ([]dw.Permission, error) {
	e, _ := f.Environment(ctx)
	return e.Permissions, nil
}
func (f *Fixture) allowed(name string) bool {
	for _, p := range f.env.Permissions {
		if p.Name == name {
			return p.State == "granted"
		}
	}
	return false
}
func (f *Fixture) native(k backend.Key) backend.Node {
	n := f.nodes[k]
	raw, _ := json.Marshal(n.Object)
	var o dw.Object
	_ = json.Unmarshal(raw, &o)
	o.SampleStart = time.Now().UTC()
	o.SampleEnd = o.SampleStart
	return backend.Node{Key: k, Parent: f.ids[n.Parent], App: f.ids[n.App], Window: f.ids[n.Window], Object: o}
}
func (f *Fixture) Query(ctx context.Context, q backend.Query) (backend.Page, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.allowed("accessibility") {
		return backend.Page{}, dw.NewFault("permission_denied", "accessibility denied", "reobserve")
	}
	if f.readFault != nil {
		return backend.Page{}, f.readFault
	}
	if f.slowDelay > 0 {
		return f.slowQuery(ctx, q)
	}
	p := backend.Page{Complete: !f.incomplete, Seat: f.seatSnapshot()}
	keys := make([]backend.Key, 0, len(f.nodes))
	for k := range f.nodes {
		keys = append(keys, k)
	}
	sort.Slice(keys, func(i, j int) bool { return f.nodes[keys[i]].ID < f.nodes[keys[j]].ID })
	for _, k := range keys {
		n := f.nodes[k]
		include := q.Desktop
		if q.Summary && n.Object.Kind == dw.KindUI {
			continue
		}
		if !q.Desktop {
			for _, root := range q.Roots {
				if root == k {
					include = true
					break
				}
				if q.Detail {
					continue
				}
				cur := f.ids[n.Parent]
				for depth := 1; cur != "" && depth <= q.Depth; depth++ {
					if cur == root {
						include = true
						break
					}
					cur = f.ids[f.nodes[cur].Parent]
				}
			}
		}
		if !include {
			continue
		}
		if p.Visited >= q.MaxNodes {
			p.Complete = false
			break
		}
		p.Visited++
		p.Nodes = append(p.Nodes, f.native(k))
	}
	return p, ctx.Err()
}
func (f *Fixture) slowQuery(ctx context.Context, q backend.Query) (backend.Page, error) {
	var s *slowScan
	if q.Resume != "" {
		s = f.slowScans[q.Resume]
		if s == nil {
			return backend.Page{}, dw.NewFault("continuation_expired", "fixture scan expired", "reobserve")
		}
		delete(f.slowScans, q.Resume)
	} else {
		s = &slowScan{}
		for k, n := range f.nodes {
			if q.Summary && n.Object.Kind == dw.KindUI {
				continue
			}
			include := q.Desktop
			for _, root := range q.Roots {
				if k == root || (!q.Detail && q.Depth > 0 && f.ids[n.Parent] == root) {
					include = true
				}
			}
			if include {
				s.keys = append(s.keys, k)
			}
		}
		sort.Slice(s.keys, func(i, j int) bool { return f.nodes[s.keys[i]].ID < f.nodes[s.keys[j]].ID })
	}
	p := backend.Page{Seat: f.seatSnapshot()}
	for s.offset < len(s.keys) && p.Visited < q.MaxNodes && s.visited < 10000 {
		if ctx.Err() != nil {
			break
		}
		time.Sleep(f.slowDelay)
		if ctx.Err() != nil {
			break
		}
		p.Nodes = append(p.Nodes, f.native(s.keys[s.offset]))
		s.offset++
		s.visited++
		p.Visited++
	}
	p.Visited = s.visited
	p.Complete = s.offset == len(s.keys)
	if s.offset < len(s.keys) {
		p.Complete = false
		if s.visited >= 10000 {
			p.Unavailable = []string{"ax_scan_limit"}
		} else {
			f.slowNext++
			p.ScanCursor = fmt.Sprintf("fixture-scan-%d", f.slowNext)
			f.slowScans[p.ScanCursor] = s
			if ctx.Err() != nil {
				p.Unavailable = []string{"ax_timeout"}
			} else {
				p.Unavailable = []string{"ax_node_budget"}
			}
		}
	}
	return p, nil
}
func (f *Fixture) Read(ctx context.Context, k backend.Key) (backend.Node, backend.Seat, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.allowed("accessibility") {
		return backend.Node{}, f.seat, dw.NewFault("permission_denied", "accessibility denied", "reobserve")
	}
	if f.readFault != nil {
		return backend.Node{}, f.seat, f.readFault
	}
	if _, ok := f.nodes[k]; !ok {
		return backend.Node{}, f.seat, dw.NewFault("ref_gone", "fixture instance destroyed", "reobserve")
	}
	return f.native(k), f.seatSnapshot(), ctx.Err()
}
func (f *Fixture) seatSnapshot() backend.Seat {
	s := f.seat
	s.Nodes = nil
	for _, key := range []backend.Key{s.Foreground, s.Focused} {
		if _, ok := f.nodes[key]; ok {
			s.Nodes = append(s.Nodes, f.native(key))
		}
	}
	return s
}

// Focus simulates an external focus change before the next observation.
func (f *Fixture) Focus(id string) { f.mu.Lock(); defer f.mu.Unlock(); f.focus(f.ids[id]) }
func (f *Fixture) ReadText(ctx context.Context, k backend.Key) (backend.Text, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	n, ok := f.nodes[k]
	if !ok {
		return backend.Text{}, dw.NewFault("ref_gone", "fixture instance destroyed", "reobserve")
	}
	v := n.Text
	if n.Object.ValuePreview.Value != nil && v == "" {
		v = *n.Object.ValuePreview.Value
	}
	source := n.TextSource
	if source == "" {
		source = "value"
	}
	return backend.Text{Value: dw.Known(v), Source: source}, ctx.Err()
}
func (f *Fixture) focus(k backend.Key) {
	n := f.nodes[k]
	f.seat.Application = f.ids[n.App]
	if n.Object.Kind == dw.KindWindow {
		f.seat.Foreground = k
	} else {
		f.seat.Foreground = f.ids[n.Window]
	}
	f.seat.Focused = k
	for key, node := range f.nodes {
		node.Object.States["focused"] = dw.Known(key == k)
		f.nodes[key] = node
	}
}
func (f *Fixture) Perform(ctx context.Context, op backend.Operation) backend.Outcome {
	f.mu.Lock()
	var b Behavior
	custom := false
	if len(f.behaviors[op.Step.Op]) > 0 {
		custom = true
		b = f.behaviors[op.Step.Op][0]
		f.behaviors[op.Step.Op] = f.behaviors[op.Step.Op][1:]
	}
	f.mu.Unlock()
	if b.Before != nil {
		b.Before(f)
	}
	if b.Block != nil {
		if b.IgnoreCancellation {
			<-b.Block
		} else {
			select {
			case <-ctx.Done():
				return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("cancelled", "fixture cancelled", "reobserve")}
			case <-b.Block:
			}
		}
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.allowed("input") {
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("permission_denied", "input denied", "reobserve")}
	}
	delivery := b.Delivery
	if delivery == "" {
		delivery = dw.DeliveryComplete
	}
	if !custom || b.Apply {
		n, ok := f.nodes[op.Key]
		if op.Key != "" && !ok {
			return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("ref_gone", "fixture instance destroyed", "reobserve")}
		}
		ev := Event{Operation: op.Step.Op, Target: n.ID, At: time.Now().UTC()}
		switch op.Step.Op {
		case "focus":
			f.focus(op.Key)
		case "set_value":
			n.Text = op.Step.SetValue.Text
			n.Object.ValuePreview = dw.Known(n.Text)
			f.nodes[op.Key] = n
			ev.Text = n.Text
		case "keyboard.type_text":
			n.Text += op.Step.TypeText.Text
			n.Object.ValuePreview = dw.Known(n.Text)
			f.nodes[op.Key] = n
			ev.Text = op.Step.TypeText.Text
		case "keyboard.press":
			ev.Text = op.Step.Press.Key
		}
		f.events = append(f.events, ev)
	}
	return backend.Outcome{Delivery: delivery, Fault: b.Fault, Unsafe: b.Unsafe}
}
func (f *Fixture) HitTest(ctx context.Context, p dw.Point, k backend.Key) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	n, ok := f.nodes[k]
	if !ok || n.Object.Bounds.Value == nil {
		return false, nil
	}
	r := n.Object.Bounds.Value.Rect
	return p.X >= r.X && p.Y >= r.Y && p.X <= r.X+r.Width && p.Y <= r.Y+r.Height, ctx.Err()
}
func (f *Fixture) Capture(ctx context.Context, r dw.CaptureRequest) ([]backend.Image, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.allowed("screen_capture") {
		return nil, dw.NewFault("permission_denied", "capture denied", "reobserve")
	}
	if r.Kind != "visible_region" {
		return nil, dw.NewFault("capability_unavailable", "window content unsupported", "reobserve")
	}
	img := image.NewRGBA(image.Rect(0, 0, 2, 2))
	img.Set(0, 0, color.RGBA{R: 255, A: 255})
	var b bytes.Buffer
	_ = png.Encode(&b, img)
	bounds := dw.Bounds{Frame: "desktop", Topology: f.env.Topology, Rect: dw.Rect{Width: 2, Height: 2}}
	if r.Region != nil {
		bounds = *r.Region
	}
	return []backend.Image{{Bytes: b.Bytes(), ContentType: "image/png", Bounds: bounds, Width: 2, Height: 2}}, ctx.Err()
}

// Form creates a small, named world used by examples and contract tests.
func (f *Fixture) Form() {
	f.Add(Node{ID: "app", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Fixture")}})
	f.Add(Node{ID: "window", App: "app", Parent: "app", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("Desktop World Fixture")}})
	f.Add(Node{ID: "field", App: "app", Parent: "window", Window: "window", Object: dw.Object{Kind: dw.KindUI, Role: "text_field", Name: dw.Known("内容"), ValuePreview: dw.Known("")}})
	f.Add(Node{ID: "submit", App: "app", Parent: "window", Window: "window", Object: dw.Object{Kind: dw.KindUI, Role: "button", Name: dw.Known("提交")}})
}
