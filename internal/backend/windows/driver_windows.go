//go:build windows && amd64

package windows

import (
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"reflect"
	"sort"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"
)

type entry struct {
	el                       *com
	key, app, window, parent backend.Key
	pid                      uint32
	start                    uint64
	hwnd                     uintptr
	gone                     bool
	application              bool
}
type Driver struct {
	uia, walker, cache *com
	entries            []*entry
	byKey              map[backend.Key]*entry
	apps               map[string]backend.Key
	env                dw.Environment
	initialized        bool
}

func New() backend.Driver {
	return &Driver{byKey: map[backend.Key]*entry{}, apps: map[string]backend.Key{}}
}
func (d *Driver) Open(ctx context.Context) error {
	hr, _, _ := coInit.Call(0, 0)
	if int32(hr) < 0 {
		return nativeFault(hr)
	}
	d.initialized = true
	hr, _, _ = coCreate.Call(ptr(&clsid), 0, 1, ptr(&iid), ptr(&d.uia))
	if int32(hr) < 0 {
		return nativeFault(hr)
	}
	_ = d.uia.call(59, 0)
	_ = d.uia.call(61, 500)
	_ = d.uia.call(63, 500)
	if e := d.uia.call(14, ptr(&d.walker)); e != nil {
		return e
	}
	if e := d.uia.call(20, ptr(&d.cache)); e != nil {
		return e
	}
	for _, id := range []uintptr{30001, 30002, 30003, 30005, 30008, 30009, 30010, 30019, 30022, 30031, 30043, 30045} {
		if e := d.cache.call(3, id); e != nil {
			return e
		}
	}
	_ = d.cache.call(7, 1)
	_ = d.cache.call(11, 0)
	_, e := d.Environment(ctx)
	return e
}
func (d *Driver) Close(context.Context) error {
	for _, e := range d.entries {
		e.el.release()
	}
	d.cache.release()
	d.walker.release()
	d.uia.release()
	if d.initialized {
		coUninit.Call()
	}
	return nil
}
func (d *Driver) Environment(ctx context.Context) (dw.Environment, error) {
	restore := dpiScope()
	defer restore()
	var collected monitorCollection
	id := registerCallback(&collected)
	proc(user32, "EnumDisplayMonitors").Call(0, 0, monitorCallback, id)
	callbackStates.Delete(id)
	ds := collected.displays
	sort.Slice(ds, func(i, j int) bool { return ds[i].Ref < ds[j].Ref })
	if !reflect.DeepEqual(ds, d.env.Displays) {
		d.env.Topology++
	}
	d.env.Platform = "windows"
	d.env.Displays = ds
	d.env.Permissions = []dw.Permission{{Name: "accessibility", State: "granted", Reason: "limited_by_provider_and_integrity"}, {Name: "input", State: "unknown", Reason: "subject_to_UIPI_and_interactive_desktop"}, {Name: "screen_capture", State: "unknown", Reason: "interactive_desktop_required"}, {Name: "user_input_observation", State: "not_requested"}}
	d.env.Capabilities = []dw.Capability{{Name: "visible_region", Support: "supported", Availability: "unknown"}, {Name: "window_content", Support: "unsupported", Availability: "blocked"}}
	return d.env, ctx.Err()
}
func (d *Driver) Permissions(ctx context.Context, _ dw.PermissionRequest) ([]dw.Permission, error) {
	e, err := d.Environment(ctx)
	return e.Permissions, err
}
func (d *Driver) retain(el *com, app, window, parent backend.Key, hwnd uintptr) (backend.Key, error) {
	if el == nil {
		return "", nil
	}
	pid, e := intProp(el, 20)
	if e != nil {
		el.release()
		return "", e
	}
	start := processStart(uint32(pid))
	if start == 0 {
		el.release()
		return "", dw.NewFault("provider_unavailable", "process lifetime unavailable", "reobserve")
	}
	for _, old := range d.entries {
		if old.gone || old.application || old.pid != uint32(pid) || old.start != start {
			continue
		}
		var same int32
		if d.uia.call(3, ptr(old.el), ptr(el), ptr(&same)) == nil && same != 0 {
			if app != "" {
				old.app = app
			}
			if window != "" && old.hwnd == 0 {
				old.window = window
			}
			if parent != "" {
				old.parent = parent
			}
			el.release()
			return old.key, nil
		}
	}
	if len(d.entries) >= 10000 {
		el.release()
		return "", dw.NewFault("resource_exhausted", "native registry full", "reobserve")
	}
	k := backend.Key(fmt.Sprintf("native-%d", len(d.entries)+1))
	entry := &entry{el: el, key: k, app: app, window: window, parent: parent, pid: uint32(pid), start: start, hwnd: hwnd}
	if hwnd != 0 {
		entry.window = k
	}
	d.entries = append(d.entries, entry)
	d.byKey[k] = entry
	return k, nil
}
func (d *Driver) application(pid uint32, start uint64) backend.Key {
	id := fmt.Sprintf("%d:%d", pid, start)
	if k := d.apps[id]; k != "" {
		return k
	}
	k := backend.Key(fmt.Sprintf("native-%d", len(d.entries)+1))
	e := &entry{key: k, pid: pid, start: start, application: true, app: k}
	d.entries = append(d.entries, e)
	d.byKey[k] = e
	d.apps[id] = k
	return k
}
func (d *Driver) lookup(k backend.Key) (*entry, error) {
	e := d.byKey[k]
	if e == nil {
		return nil, dw.NewFault("ref_expired", "unknown native reference", "reobserve")
	}
	if !e.gone {
		if processStart(e.pid) != e.start {
			e.gone = true
		}
		if e.hwnd != 0 {
			v, _, _ := isWindow.Call(e.hwnd)
			if v == 0 {
				e.gone = true
			} else {
				var pid uint32
				windowPID.Call(e.hwnd, ptr(&pid))
				if pid != e.pid {
					e.gone = true
				}
			}
		}
	}
	if e.gone {
		return nil, dw.NewFault("ref_gone", "native lifetime ended", "reobserve")
	}
	return e, nil
}
func (d *Driver) node(ctx context.Context, k backend.Key) (backend.Node, error) {
	e, err := d.lookup(k)
	if err != nil {
		return backend.Node{}, err
	}
	if e.application {
		return backend.Node{Key: k, App: k, Object: dw.Object{Kind: dw.KindApplication, Role: "application", Name: dw.Known(fmt.Sprintf("Process %d", e.pid)), Lifecycle: dw.LifeLive}}, nil
	}
	if e.hwnd == 0 {
		if err = d.refreshOwnership(e); err != nil {
			return backend.Node{}, err
		}
	}
	var cached *com
	if err = e.el.call(9, ptr(d.cache), ptr(&cached)); err != nil {
		return backend.Node{}, err
	}
	defer cached.release()
	control, _ := intProp(cached, 53)
	name, nameErr := stringProp(cached, 55)
	o := dw.Object{Kind: dw.KindUI, Role: role(control), Lifecycle: dw.LifeLive, Name: dw.Unknown[string](), States: map[string]dw.Fact[bool]{"focused": boolProp(cached, 58), "enabled": boolProp(cached, 60), "protected": boolProp(cached, 67), "offscreen": boolProp(cached, 70)}}
	if e.hwnd != 0 {
		o.Kind = dw.KindWindow
	}
	if nameErr == nil {
		o.Name = dw.Known(name)
	}
	var rect [4]int32
	if cached.call(75, ptr(&rect)) == nil {
		o.Bounds = dw.Known(dw.Bounds{Frame: "desktop", Topology: d.env.Topology, Rect: dw.Rect{X: float64(rect[0]), Y: float64(rect[1]), Width: float64(rect[2] - rect[0]), Height: float64(rect[3] - rect[1])}})
	} else {
		o.Bounds = dw.Unknown[dw.Bounds]()
	}
	invoke := isTrue(propBool(cached, 30031, true))
	value := isTrue(propBool(cached, 30043, true))
	writable := false
	o.States["read_only"] = dw.Unknown[bool]()
	if value && !isTrue(o.States["protected"]) {
		var pattern *com
		if e.el.call(16, 10002, ptr(&pattern)) == nil && pattern != nil {
			ro := boolProp(pattern, 5)
			o.States["read_only"] = ro
			writable = ro.Status == dw.FactKnown && !isTrue(ro)
			v, er := stringProp(pattern, 4)
			if er == nil {
				r := []rune(v)
				if len(r) > 192 {
					r = r[:192]
				}
				o.ValuePreview = dw.Known(string(r))
			}
			pattern.release()
		}
	}
	if isTrue(o.States["protected"]) {
		o.ValuePreview = dw.Fact[string]{Status: dw.FactRedacted}
	}
	for op, supported := range map[string]bool{"focus": e.hwnd != 0 || isTrue(boolProp(cached, 59)), "invoke": invoke, "set_value": writable} {
		sup, avail := "unsupported", "blocked"
		if supported {
			sup = "supported"
			if isTrue(o.States["enabled"]) {
				avail = "available"
			}
		}
		o.Capabilities = append(o.Capabilities, dw.Capability{Name: op, Support: sup, Availability: avail})
	}
	sort.Slice(o.Capabilities, func(i, j int) bool { return o.Capabilities[i].Name < o.Capabilities[j].Name })
	o.SampleStart = time.Now().UTC()
	o.SampleEnd = o.SampleStart
	return backend.Node{Key: k, App: e.app, Window: e.window, Parent: e.parent, Object: o}, ctx.Err()
}
func role(c int32) string {
	m := map[int32]string{50000: "button", 50002: "checkbox", 50004: "text_field", 50007: "list_item", 50008: "list", 50009: "menu", 50011: "menu_item", 50018: "tab", 50020: "text", 50030: "document", 50032: "window", 50033: "container"}
	if r := m[c]; r != "" {
		return r
	}
	return "unknown"
}
func (d *Driver) Query(ctx context.Context, q backend.Query) (backend.Page, error) {
	restore := dpiScope()
	defer restore()
	p := backend.Page{Complete: true}
	type item struct {
		k     backend.Key
		depth int
	}
	queue := []item{}
	if q.Desktop {
		state := windowCollection{driver: d, ctx: ctx, max: q.MaxNodes, complete: true}
		id := registerCallback(&state)
		enumWindows.Call(windowCallback, id)
		callbackStates.Delete(id)
		p.Complete = state.complete
		for _, entry := range state.roots {
			queue = append(queue, item{entry, 0})
		}

	} else {
		for _, k := range q.Roots {
			queue = append(queue, item{k, 0})
		}
	}
	seen := map[backend.Key]bool{}
	for len(queue) > 0 {
		if ctx.Err() != nil || p.Visited >= q.MaxNodes {
			p.Complete = false
			break
		}
		cur := queue[0]
		queue = queue[1:]
		if seen[cur.k] {
			continue
		}
		seen[cur.k] = true
		p.Visited++
		n, e := d.node(ctx, cur.k)
		if e != nil {
			p.Complete = false
			continue
		}
		p.Nodes = append(p.Nodes, n)
		if q.Summary || q.Detail || cur.depth >= q.Depth {
			continue
		}
		entry := d.byKey[cur.k]
		if entry.application {
			for _, e := range d.entries {
				if e.app == cur.k && e.hwnd != 0 && !e.gone {
					queue = append(queue, item{e.key, cur.depth + 1})
				}
			}
			continue
		}
		var child *com
		if d.walker.call(4, ptr(entry.el), ptr(&child)) != nil {
			p.Complete = false
			continue
		}
		for child != nil {
			if p.Visited+len(queue) >= q.MaxNodes {
				child.release()
				p.Complete = false
				break
			}
			var next *com
			nextErr := d.walker.call(6, ptr(child), ptr(&next))
			k, e := d.retain(child, entry.app, entry.window, entry.key, 0)
			if e != nil {
				p.Complete = false
			} else {
				queue = append(queue, item{k, cur.depth + 1})
			}
			if nextErr != nil {
				p.Complete = false
				break
			}
			child = next
		}
	}
	p.Seat = d.seat()
	if !p.Complete {
		p.Unavailable = []string{"uia_partial"}
	}
	return p, nil
}
func (d *Driver) seat() backend.Seat {
	s := backend.Seat{Health: "ready", Intervention: "best_effort"}
	hwnd, _, _ := foreground.Call()
	for _, e := range d.entries {
		if !e.gone && e.hwnd == hwnd {
			s.Foreground = e.key
			break
		}
	}
	var focused *com
	if d.uia.call(8, ptr(&focused)) == nil && focused != nil {
		if win := d.byKey[s.Foreground]; win != nil {
			s.Focused, _ = d.retain(focused, win.app, win.key, win.key, 0)
		} else {
			focused.release()
		}
	}
	var point [2]int32
	if ok, _, _ := cursorPos.Call(ptr(&point)); ok != 0 {
		s.Pointer = dw.Known(dw.Point{Frame: "desktop", Topology: d.env.Topology, X: float64(point[0]), Y: float64(point[1]), ObservedAt: time.Now().UTC()})
	}
	if hwnd == 0 {
		s.Health = "unavailable"
	}
	return s
}
func (d *Driver) Read(ctx context.Context, k backend.Key) (backend.Node, backend.Seat, error) {
	restore := dpiScope()
	defer restore()
	n, e := d.node(ctx, k)
	return n, d.seat(), e
}
func (d *Driver) ReadText(ctx context.Context, k backend.Key) (backend.Text, error) {
	e, err := d.lookup(k)
	if err != nil {
		return backend.Text{}, err
	}
	if e.application {
		return backend.Text{Value: dw.Fact[string]{Status: dw.FactUnsupported}}, nil
	}
	if isTrue(boolProp(e.el, 35)) {
		return backend.Text{Value: dw.Fact[string]{Status: dw.FactRedacted}, Source: "value"}, nil
	}
	var pattern *com
	if e.el.call(16, 10002, ptr(&pattern)) == nil && pattern != nil {
		defer pattern.release()
		v, err := stringProp(pattern, 4)
		if err != nil {
			return backend.Text{}, err
		}
		return backend.Text{Value: dw.Known(v), Source: "value"}, nil
	}
	if e.el.call(16, 10014, ptr(&pattern)) == nil && pattern != nil {
		defer pattern.release()
		var r *com
		if err = rDocument(pattern, &r); err != nil {
			return backend.Text{}, err
		}
		defer r.release()
		var bstr unsafe.Pointer
		if err = r.call(12, 1<<20, ptr(&bstr)); err != nil {
			return backend.Text{}, err
		}
		defer freeBSTR.Call(uintptr(bstr))
		text := strBSTR(bstr)
		if len([]rune(text)) >= 1<<20 {
			return backend.Text{}, dw.NewFault("resource_exhausted", "native text exceeds bounded read", "reobserve")
		}
		return backend.Text{Value: dw.Known(text), Source: "text"}, nil
	}
	v, err := stringProp(e.el, 23)
	return backend.Text{Value: dw.Known(v), Source: "label"}, err
}
func rDocument(p *com, r **com) error { return p.call(7, ptr(r)) }
func (d *Driver) HitTest(ctx context.Context, p dw.Point, k backend.Key) (bool, error) {
	restore := dpiScope()
	defer restore()
	e, err := d.lookup(k)
	if err != nil {
		return false, err
	}
	packed := uint64(uint32(int32(p.X))) | uint64(uint32(int32(p.Y)))<<32
	var hit *com
	if err = d.uia.call(7, uintptr(packed), ptr(&hit)); err != nil {
		return false, err
	}
	defer hit.release()
	var same int32
	err = d.uia.call(3, ptr(hit), ptr(e.el), ptr(&same))
	return same != 0, err
}

type monitorCollection struct{ displays []dw.Display }

var monitorCallback = syscall.NewCallback(func(h, dc, rect, data uintptr) uintptr {
	value, ok := callbackStates.Load(data)
	if !ok {
		return 0
	}
	state := value.(*monitorCollection)
	var info struct {
		Size          uint32
		Monitor, Work [4]int32
		Flags         uint32
	}
	info.Size = uint32(unsafe.Sizeof(info))
	success, _, _ := proc(user32, "GetMonitorInfoW").Call(h, ptr(&info))
	if success != 0 {
		r := info.Monitor
		state.displays = append(state.displays, dw.Display{Ref: fmt.Sprint(h), Frame: "desktop", Bounds: dw.Rect{X: float64(r[0]), Y: float64(r[1]), Width: float64(r[2] - r[0]), Height: float64(r[3] - r[1])}, Unit: "physical_pixel", ScaleX: 1, ScaleY: 1})
	}
	return 1
})

type windowCollection struct {
	driver   *Driver
	ctx      context.Context
	max      int
	complete bool
	roots    []backend.Key
}

var windowCallback = syscall.NewCallback(func(hwnd, param uintptr) uintptr {
	value, ok := callbackStates.Load(param)
	if !ok {
		return 0
	}
	state := value.(*windowCollection)
	d := state.driver
	if state.ctx.Err() != nil || len(state.roots) >= state.max {
		state.complete = false
		return 0
	}
	v, _, _ := isVisible.Call(hwnd)
	if v == 0 {
		return 1
	}
	var el *com
	if d.uia.call(6, hwnd, ptr(&el)) != nil || el == nil {
		state.complete = false
		return 1
	}
	pid, _ := intProp(el, 20)
	start := processStart(uint32(pid))
	if start == 0 {
		el.release()
		state.complete = false
		return 1
	}
	app := d.application(uint32(pid), start)
	key, e := d.retain(el, app, "", app, hwnd)
	if e != nil {
		state.complete = false
		return 1
	}
	state.roots = append(state.roots, app, key)
	return 1
})

var callbackStates sync.Map
var nextCallback atomic.Uint64

func registerCallback(v any) uintptr {
	id := uintptr(nextCallback.Add(1))
	callbackStates.Store(id, v)
	return id
}

// Resolve the current native window before exporting or acting on a UI element.
// A stale containment cache must not authorize a control moved to another window.
func (d *Driver) refreshOwnership(e *entry) error {
	cur := e.el
	_ = cur.call(1)
	defer func() { cur.release() }()
	for i := 0; i < 32; i++ {
		var hwnd uintptr
		if cur.call(36, ptr(&hwnd)) == nil && hwnd != 0 {
			root, _, _ := proc(user32, "GetAncestor").Call(hwnd, 2)
			for _, window := range d.entries {
				if !window.gone && window.hwnd == root && window.pid == e.pid && window.start == e.start {
					e.window = window.key
					return nil
				}
			}
			return dw.NewFault("ref_stale", "window ownership needs rediscovery", "reobserve")
		}
		var parent *com
		if d.walker.call(3, ptr(cur), ptr(&parent)) != nil || parent == nil {
			return dw.NewFault("ref_stale", "window ownership unavailable", "reobserve")
		}
		cur.release()
		cur = parent
	}
	return dw.NewFault("ref_stale", "ownership traversal limit reached", "reobserve")
}
