//go:build windows && amd64

package windows

import (
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"syscall"
	"unsafe"
)

// Vtable slots are pinned to Microsoft's UIAutomationClient.h. Every interface
// pointer is owned and used exclusively on the engine's locked MTA worker.
type com struct{ vt *[96]uintptr }

func (c *com) call(slot int, args ...uintptr) error {
	if c == nil {
		return nativeFault(0x80004003)
	}
	all := append([]uintptr{uintptr(unsafe.Pointer(c))}, args...)
	hr, _, _ := syscall.SyscallN(c.vt[slot], all...)
	if int32(hr) < 0 {
		return nativeFault(hr)
	}
	return nil
}
func (c *com) release() {
	if c != nil {
		_ = c.call(2)
	}
}
func ptr[T any](v *T) uintptr { return uintptr(unsafe.Pointer(v)) }
func nativeFault(hr uintptr) *dw.Fault {
	return &dw.Fault{Code: "provider_unavailable", Message: "UI Automation call failed", NativeCode: fmt.Sprintf("0x%08x", uint32(hr)), RetryClass: "reobserve"}
}

type guid struct {
	A    uint32
	B, C uint16
	D    [8]byte
}

var clsid = guid{0xE22AD333, 0xB25F, 0x460C, [8]byte{0x83, 0xD0, 0x05, 0x81, 0x10, 0x73, 0x95, 0xC9}}
var iid = guid{0x34723AFF, 0x0C9D, 0x49D0, [8]byte{0x98, 0x96, 0x7A, 0xB5, 0x2D, 0xF8, 0xCD, 0x8A}}
var ole32 = syscall.NewLazyDLL("ole32.dll")
var oleaut = syscall.NewLazyDLL("oleaut32.dll")
var user32 = syscall.NewLazyDLL("user32.dll")
var kernel32 = syscall.NewLazyDLL("kernel32.dll")
var gdi32 = syscall.NewLazyDLL("gdi32.dll")

func proc(d *syscall.LazyDLL, n string) *syscall.LazyProc { return d.NewProc(n) }

var coInit = proc(ole32, "CoInitializeEx")
var coCreate = proc(ole32, "CoCreateInstance")
var coUninit = proc(ole32, "CoUninitialize")
var freeBSTR = proc(oleaut, "SysFreeString")
var bstrLen = proc(oleaut, "SysStringLen")
var variantClear = proc(oleaut, "VariantClear")
var enumWindows = proc(user32, "EnumWindows")
var isVisible = proc(user32, "IsWindowVisible")
var isWindow = proc(user32, "IsWindow")
var foreground = proc(user32, "GetForegroundWindow")
var setForeground = proc(user32, "SetForegroundWindow")
var windowPID = proc(user32, "GetWindowThreadProcessId")
var metrics = proc(user32, "GetSystemMetrics")
var cursorPos = proc(user32, "GetCursorPos")
var setDPI = proc(user32, "SetThreadDpiAwarenessContext")
var openProcess = proc(kernel32, "OpenProcess")
var processTimes = proc(kernel32, "GetProcessTimes")
var closeHandle = proc(kernel32, "CloseHandle")

func strBSTR(v unsafe.Pointer) string {
	if v == nil {
		return ""
	}
	n, _, _ := bstrLen.Call(uintptr(v))
	if n > 1<<20 {
		return ""
	}
	s := syscall.UTF16ToString(unsafe.Slice((*uint16)(v), int(n)))
	return s
}
func stringProp(c *com, slot int) (string, error) {
	var s unsafe.Pointer
	e := c.call(slot, ptr(&s))
	if s != nil {
		defer freeBSTR.Call(uintptr(s))
	}
	if e != nil {
		return "", e
	}
	return strBSTR(s), nil
}
func intProp(c *com, slot int) (int32, error) { var v int32; e := c.call(slot, ptr(&v)); return v, e }
func boolProp(c *com, slot int) dw.Fact[bool] {
	v, e := intProp(c, slot)
	if e != nil {
		return dw.Unknown[bool]()
	}
	return dw.Known(v != 0)
}
func propBool(c *com, id int32, cached bool) dw.Fact[bool] {
	var v struct {
		VT           uint16
		R            [3]uint16
		Value, Extra uint64
	}
	slot := 10
	if cached {
		slot = 12
	}
	if e := c.call(slot, uintptr(id), ptr(&v)); e != nil {
		return dw.Unknown[bool]()
	}
	defer variantClear.Call(ptr(&v))
	if v.VT != 11 {
		return dw.Unknown[bool]()
	}
	return dw.Known(v.Value&0xffff != 0)
}
func isTrue(v dw.Fact[bool]) bool { return v.Status == dw.FactKnown && v.Value != nil && *v.Value }
func processStart(pid uint32) uint64 {
	h, _, _ := openProcess.Call(0x1000, 0, uintptr(pid))
	if h == 0 {
		return 0
	}
	defer closeHandle.Call(h)
	var creation, exit, kernel, user uint64
	ok, _, _ := processTimes.Call(h, ptr(&creation), ptr(&exit), ptr(&kernel), ptr(&user))
	if ok == 0 {
		return 0
	}
	return creation
}
func dpiScope() func() {
	if setDPI.Find() != nil {
		return func() {}
	}
	old, _, _ := setDPI.Call(^uintptr(3))
	return func() { setDPI.Call(old) }
}
