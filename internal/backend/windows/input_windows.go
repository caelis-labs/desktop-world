//go:build windows && amd64

package windows

import (
	"context"
	"encoding/binary"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"math"
	"strings"
	"syscall"
	"time"
	"unicode/utf16"
	"unsafe"
)

var sendInput = proc(user32, "SendInput")
var asyncKey = proc(user32, "GetAsyncKeyState")

type input struct {
	Type    uint32
	Padding uint32
	Data    [32]byte
}

func keyInput(key, scan uint16, flags uint32) input {
	i := input{Type: 1}
	binary.LittleEndian.PutUint16(i.Data[0:2], key)
	binary.LittleEndian.PutUint16(i.Data[2:4], scan)
	binary.LittleEndian.PutUint32(i.Data[4:8], flags)
	return i
}
func mouseInput(x, y int32, data, flags uint32) input {
	i := input{}
	binary.LittleEndian.PutUint32(i.Data[0:4], uint32(x))
	binary.LittleEndian.PutUint32(i.Data[4:8], uint32(y))
	binary.LittleEndian.PutUint32(i.Data[8:12], data)
	binary.LittleEndian.PutUint32(i.Data[12:16], flags)
	return i
}
func send(events []input) int {
	if len(events) == 0 {
		return 0
	}
	n, _, _ := sendInput.Call(uintptr(len(events)), ptr(&events[0]), unsafe.Sizeof(input{}))
	return int(n)
}
func move(p dw.Point) input {
	metric := func(n uintptr) int32 { v, _, _ := metrics.Call(n); return int32(v) }
	x, y, w, h := metric(76), metric(77), metric(78), metric(79)
	nx, ny := int32(0), int32(0)
	if w > 1 {
		nx = int32(math.Round((p.X - float64(x)) * 65535 / float64(w-1)))
	}
	if h > 1 {
		ny = int32(math.Round((p.Y - float64(y)) * 65535 / float64(h-1)))
	}
	return mouseInput(nx, ny, 0, 0x8000|0x4000|1)
}
func keyCode(k string) uint16 {
	if len(k) == 1 {
		return uint16(k[0])
	}
	return map[string]uint16{"Enter": 13, "Tab": 9, "Escape": 27, "Backspace": 8, "Delete": 46, "Space": 32, "Left": 37, "Right": 39, "Up": 38, "Down": 40, "Home": 36, "End": 35, "PageUp": 33, "PageDown": 34}[k]
}
func modifier(m string) uint16 {
	return map[string]uint16{"control": 17, "primary": 17, "shift": 16, "alt": 18, "meta": 91}[m]
}
func result(del dw.Delivery, code string) backend.Outcome {
	o := backend.Outcome{Delivery: del}
	if code != "" {
		o.Fault = dw.NewFault(code, code, "never_automatically")
	}
	return o
}
func (d *Driver) Perform(ctx context.Context, o backend.Operation) backend.Outcome {
	restore := dpiScope()
	defer restore()
	if err := ctx.Err(); err != nil {
		return result(dw.DeliveryNone, "cancelled")
	}
	s := o.Step
	if dw.ActionChannel(s.Op) == "semantic" || s.Op == "focus" {
		e, err := d.lookup(o.Key)
		if err != nil {
			return result(dw.DeliveryNone, "ref_gone")
		}
		if e.application {
			return result(dw.DeliveryNone, "capability_unavailable")
		}
		if s.Op == "focus" {
			if e.hwnd != 0 {
				ok, _, _ := setForeground.Call(e.hwnd)
				if ok == 0 {
					return result(dw.DeliveryNone, "needs_user_focus")
				}
				return result(dw.DeliveryComplete, "")
			}
			err = e.el.call(3)
		} else {
			patternID := uintptr(10000)
			if s.Op == "set_value" {
				patternID = 10002
			}
			if s.Op == "set_expanded" {
				patternID = 10005
			}
			switch s.Op {
			case "set_checked":
				patternID = 10015
			case "set_selected":
				patternID = 10010
			case "scroll_into_view":
				patternID = 10017
			}
			var p *com
			if er := e.el.call(16, patternID, ptr(&p)); er != nil || p == nil {
				return result(dw.DeliveryNone, "capability_unavailable")
			}
			defer p.release()
			if s.Op == "invoke" {
				err = p.call(3)
			} else if s.Op == "set_expanded" {
				method := 4
				if *s.SetExpanded.Expanded {
					method = 3
				}
				err = p.call(method)
			} else if s.Op == "set_checked" {
				state, er := intProp(p, 4)
				// A tri-state cycle is not a boolean setter. Never blindly toggle
				// an indeterminate state or repeat a toggle after uncertain delivery.
				if er != nil || (state != 0 && state != 1) {
					return result(dw.DeliveryNone, "state_unknown")
				}
				if (state == 1) == *s.SetChecked.Checked {
					return result(dw.DeliveryNA, "")
				}
				err = p.call(3)
			} else if s.Op == "set_selected" {
				selected := boolProp(p, 6)
				if selected.Status != dw.FactKnown || selected.Value == nil {
					return result(dw.DeliveryNone, "state_unknown")
				}
				if *selected.Value == *s.SetSelected.Selected {
					return result(dw.DeliveryNA, "")
				}
				method := 5 // RemoveFromSelection
				if *s.SetSelected.Selected {
					method = 4 // AddToSelection preserves other selections; never Select.
				}
				err = p.call(method)
			} else if s.Op == "scroll_into_view" {
				err = p.call(3)
			} else {
				v, er := syscall.UTF16FromString(s.SetValue.Text)
				if er != nil {
					return result(dw.DeliveryNone, "invalid_argument")
				}
				err = p.call(3, ptr(&v[0]))
			}
		}
		if err != nil {
			return result(dw.DeliveryUnknown, "native_action_failed")
		}
		return result(dw.DeliveryComplete, "")
	}
	// Never clear physical key state. Interference requires the human to release it.
	for _, k := range []uintptr{1, 2, 4, 16, 17, 18, 91, 92} {
		v, _, _ := asyncKey.Call(k)
		if v&0x8000 != 0 {
			return result(dw.DeliveryNone, "user_interrupted")
		}
	}
	events := []input{}
	cleanup := []input{}
	duration := time.Duration(0)
	switch s.Op {
	case "keyboard.type_text":
		for _, u := range utf16.Encode([]rune(s.TypeText.Text)) {
			events = append(events, keyInput(0, u, 4), keyInput(0, u, 4|2))
		}
	case "keyboard.press":
		for _, m := range s.Press.Modifiers {
			k := modifier(m)
			events = append(events, keyInput(k, 0, 0))
		}
		k := keyCode(s.Press.Key)
		flags := uint32(0)
		if strings.Contains(" Left Right Up Down Home End PageUp PageDown Delete ", " "+s.Press.Key+" ") {
			flags = 1
		}
		events = append(events, keyInput(k, 0, flags), keyInput(k, 0, flags|2))
		for i := len(s.Press.Modifiers) - 1; i >= 0; i-- {
			events = append(events, keyInput(modifier(s.Press.Modifiers[i]), 0, 2))
		}
	case "pointer.move":
		events = append(events, move(*o.Point))
	case "pointer.click":
		events = append(events, move(*o.Point))
		down, up := uint32(2), uint32(4)
		if s.Click.Button == "right" {
			down, up = 8, 16
		}
		if s.Click.Button == "middle" {
			down, up = 32, 64
		}
		for i := 0; i < s.Click.Count; i++ {
			events = append(events, mouseInput(0, 0, 0, down), mouseInput(0, 0, 0, up))
		}
	case "pointer.scroll":
		events = append(events, move(*o.Point))
		events = append(events, mouseInput(0, 0, uint32(int32(-s.Scroll.DY*120)), 0x800), mouseInput(0, 0, uint32(int32(s.Scroll.DX*120)), 0x1000))
	case "pointer.drag":
		events = append(events, move(*o.Point), mouseInput(0, 0, 0, 2))
		duration = s.Drag.Duration
		steps := int(duration / (16 * time.Millisecond))
		if steps < 1 {
			steps = 1
		}
		for i := 1; i <= steps; i++ {
			p := *o.Point
			p.X += (o.To.X - p.X) * float64(i) / float64(steps)
			p.Y += (o.To.Y - p.Y) * float64(i) / float64(steps)
			events = append(events, move(p))
		}
		events = append(events, mouseInput(0, 0, 0, 4))
	default:
		return result(dw.DeliveryNone, "capability_unavailable")
	}
	accepted := 0
	if duration == 0 {
		accepted = send(events)
	} else {
		for i, ev := range events {
			if ctx.Err() != nil {
				break
			}
			if send([]input{ev}) != 1 {
				break
			}
			accepted++
			if i >= 2 && i < len(events)-1 {
				t := time.NewTimer(duration / time.Duration(len(events)-3))
				select {
				case <-ctx.Done():
					t.Stop()
				case <-t.C:
				}
			}
		}
	}
	// Derive cleanup exclusively from accepted downs owned by this operation.
	heldKeys := map[[2]uint16]input{}
	heldButtons := map[uint32]bool{}
	for _, ev := range events[:accepted] {
		if ev.Type == 1 {
			k := [2]uint16{binary.LittleEndian.Uint16(ev.Data[:2]), binary.LittleEndian.Uint16(ev.Data[2:4])}
			fl := binary.LittleEndian.Uint32(ev.Data[4:8])
			if fl&2 != 0 {
				delete(heldKeys, k)
			} else {
				heldKeys[k] = keyInput(k[0], k[1], fl|2)
			}
		} else {
			fl := binary.LittleEndian.Uint32(ev.Data[12:16])
			for down, up := range map[uint32]uint32{2: 4, 8: 16, 32: 64} {
				if fl&down != 0 {
					heldButtons[up] = true
				}
				if fl&up != 0 {
					delete(heldButtons, up)
				}
			}
		}
	}
	for _, up := range heldKeys {
		cleanup = append(cleanup, up)
	}
	for up := range heldButtons {
		cleanup = append(cleanup, mouseInput(0, 0, 0, up))
	}
	cleaned := send(cleanup)
	out := result(dw.DeliveryComplete, "")
	if accepted == 0 && len(events) > 0 {
		out = result(dw.DeliveryNone, "input_rejected")
	} else if accepted < len(events) {
		out = result(dw.DeliveryPartial, "partial_delivery")
	}
	requested := len(events)
	out.Accepted = &accepted
	out.Requested = &requested
	out.Unsafe = cleaned != len(cleanup)
	return out
}
