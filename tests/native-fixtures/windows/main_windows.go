//go:build windows && amd64

// A Win32 fixture with an independent append-only event log. No automation APIs
// are used here: evidence comes from messages received by the controls.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"runtime"
	"syscall"
	"time"
	"unsafe"
)

var u = syscall.NewLazyDLL("user32.dll")
var k = syscall.NewLazyDLL("kernel32.dll")

func call(n string, a ...uintptr) uintptr { r, _, _ := u.NewProc(n).Call(a...); return r }
func p[T any](v *T) uintptr               { return uintptr(unsafe.Pointer(v)) }
func wide(s string) *uint16 {
	v, e := syscall.UTF16PtrFromString(s)
	if e != nil {
		panic(e)
	}
	return v
}

var window, field, label, status, oldEdit uintptr
var submits int
var logPath string

func record(event, value string) {
	b, _ := json.Marshal(map[string]any{"event": event, "value": value, "pid": os.Getpid(), "at": time.Now().UTC()})
	f, e := os.OpenFile(logPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if e != nil {
		return
	}
	defer f.Close()
	_, _ = f.Write(append(b, '\n'))
}
func text(hwnd uintptr) string {
	n := call("GetWindowTextLengthW", hwnd)
	b := make([]uint16, n+1)
	call("GetWindowTextW", hwnd, p(&b[0]), uintptr(len(b)))
	return syscall.UTF16ToString(b)
}
func submit() {
	submits++
	call("SetWindowTextW", status, p(wide("submitted:"+itoa(submits))))
	record("submit", text(field))
}
func itoa(n int) string {
	if n < 10 {
		return string(rune('0' + n))
	}
	return "many"
}
func editProc(hwnd, msg, w, l uintptr) uintptr {
	if msg == 0x201 {
		record("pointer_down", "")
	}
	// Classic single-line EDIT does not implement Ctrl+A itself. Provide the
	// same select-all command as the macOS fixture and ordinary text editors.
	if msg == 0x100 && w == 'A' && call("GetKeyState", 17)&0x8000 != 0 {
		call("SendMessageW", hwnd, 0xB1, 0, ^uintptr(0))
		record("key_down", "Ctrl+A")
		return 0
	}
	if msg == 0x100 && w == 13 {
		record("key_down", "Enter")
		submit()
		return 0
	}
	if msg == 0x102 && w == 13 {
		return 0
	}
	r := call("CallWindowProcW", oldEdit, hwnd, msg, w, l)
	if msg == 0xC {
		record("set_value", text(hwnd))
	}
	if msg == 0x102 {
		record("text_changed", text(hwnd))
	}
	return r
}
func mainProc(hwnd, msg, w, l uintptr) uintptr {
	switch msg {
	case 0x113:
		if w == 43 {
			call("KillTimer", hwnd, w)
			call("ShowWindow", hwnd, 4) // restore without activating
			// Publish restoration only after the app has repainted its children.
			call("RedrawWindow", hwnd, 0, 0, 0x585) // invalidate/erase/frame/all children/update now
			record("visibility_restored", "")
			return 0
		}
	case 0x111:
		if w&0xffff == 27 && w>>16 == 0 {
			call("SetWindowPos", hwnd, 0, 1100, 200, 760, 650, 0x14)
			record("resized", "1100,200,760,650")
			return 0
		}
		if (w&0xffff == 28 || w&0xffff == 29) && w>>16 == 0 {
			state := uintptr(6)
			if w&0xffff == 29 {
				state = 0
			}
			call("ShowWindow", hwnd, state)
			call("SetTimer", hwnd, 43, 1500, 0)
			record("visibility_changed", fmt.Sprint(state))
			return 0
		}
		if w&0xffff == 24 && w>>16 == 0 {
			coverField()
			return 0
		}
		if w&0xffff == 25 && w>>16 == 0 {
			call("DestroyWindow", fieldCover)
			fieldCover = 0
			record("uncovered", "")
			return 0
		}
		if w&0xffff == 23 && w>>16 == 0 {
			semanticSnapshot()
			return 0
		}
		if w&0xffff == 20 && w>>16 == 0 {
			record("checked", fmt.Sprint(call("SendMessageW", checkbox, 0xf0, 0, 0)))
		}
		if (w&0xffff == 2 || w&0xffff == 9) && w>>16 == 0 {
			submit()
			return 0
		}
		if w&0xffff == 3 && w>>16 == 0 {
			call("DestroyWindow", field)
			createField()
			record("replaced", "")
			return 0
		}
	case 2:
		call("PostQuitMessage", 0)
		return 0
	}
	return call("DefWindowProcW", hwnd, msg, w, l)
}
func createField() {
	instance, _, _ := k.NewProc("GetModuleHandleW").Call(0)
	field = call("CreateWindowExW", 0, p(wide("EDIT")), p(wide("")), 0x50810080, 24, 62, 430, 30, window, 1, instance, 0)
	// Win32 derives the accessible edit name from the preceding static label.
	// Keep that relationship when the replacement control is created later.
	call("SetWindowPos", field, label, 0, 0, 0, 0, 0x13)
	oldEdit = call("SetWindowLongPtrW", field, ^uintptr(3), editCallback)
}
func main() {
	runtime.LockOSThread()
	ole := syscall.NewLazyDLL("ole32.dll")
	ole.NewProc("OleInitialize").Call(0)
	defer ole.NewProc("OleUninitialize").Call()
	rows := flag.Int("rows", 0, "bounded large-tree fixture row count")
	semantics := flag.Bool("semantics", false, "native checkbox, multi-selection and tree scrolling")
	title := flag.String("title", "Desktop World Native Fixture", "unique fixture title")
	flag.StringVar(&logPath, "log", "native-fixture.jsonl", "event log")
	flag.Parse()
	if *semantics {
		defer commonControls()()
	}
	instance, _, _ := k.NewProc("GetModuleHandleW").Call(0)
	name := wide("DWNativeFixtureClass")
	var wc struct {
		Size, Style                        uint32
		Proc                               uintptr
		ClsExtra, WndExtra                 int32
		Instance, Icon, Cursor, Background uintptr
		Menu, Class                        *uint16
		SmallIcon                          uintptr
	}
	wc.Size = uint32(unsafe.Sizeof(wc))
	wc.Proc = syscall.NewCallback(mainProc)
	wc.Instance = instance
	wc.Class = name
	wc.Background = 6
	wc.Cursor = call("LoadCursorW", 0, 32512)
	if call("RegisterClassExW", p(&wc)) == 0 {
		panic("RegisterClassExW failed")
	}
	window = call("CreateWindowExW", 0, p(name), p(wide(*title)), 0x00CF0000|0x10000000, 180, 180, 520, 320, 0, 0, instance, 0)
	if window == 0 {
		panic("CreateWindowExW failed")
	}
	label = call("CreateWindowExW", 0, p(wide("STATIC")), p(wide("内容")), 0x50000000, 24, 28, 430, 24, window, 0, instance, 0)
	createField()
	call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("提交")), 0x50010000, 24, 110, 110, 32, window, 2, instance, 0)
	call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("替换输入框")), 0x50010000, 150, 110, 150, 32, window, 3, instance, 0)
	status = call("CreateWindowExW", 0, p(wide("STATIC")), p(wide("ready")), 0x50000000, 24, 170, 430, 24, window, 4, instance, 0)
	if *semantics {
		createSemantics(instance)
	}
	if *rows < 0 || *rows > 4096 {
		panic("rows must be 0..4096")
	}
	for i := 0; i < *rows; i++ {
		row := call("CreateWindowExW", 0, p(wide("STATIC")), p(wide(fmt.Sprintf("Order row %04d", i))), 0x50000000, 24, uintptr(230+i*24), 430, 24, window, uintptr(100+i), instance, 0)
		call("SetWindowPos", row, 1, 0, 0, 0, 0, 0x13) // bottom, retain geometry, do not activate
	}
	if *rows > 0 {
		deep := call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("Deep submit")), 0x50010000, 350, 110, 140, 32, window, 9, instance, 0)
		call("SetWindowPos", deep, 1, 0, 0, 0, 0, 0x13)
	}
	call("ShowWindow", window, 5)
	call("UpdateWindow", window)
	record("ready", *title)
	var msg struct {
		HWND           uintptr
		Message        uint32
		Padding        uint32
		WParam, LParam uintptr
		Time           uint32
		X, Y           int32
		Private        uint32
	}
	for {
		r := call("GetMessageW", p(&msg), 0, 0, 0)
		if r == 0 || int32(r) == -1 {
			break
		}
		call("TranslateMessage", p(&msg))
		call("DispatchMessageW", p(&msg))
	}
}

var editCallback = syscall.NewCallback(editProc)
