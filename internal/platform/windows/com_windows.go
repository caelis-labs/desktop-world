//go:build windows && amd64

package windows

import (
	"fmt"
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"path/filepath"
	"strings"
	"syscall"
	"unsafe"
)

// Vtable slots are pinned to Microsoft's UIAutomationClient.h. Every interface
// pointer is owned and used exclusively on the engine's locked MTA worker.
type com struct{ vt *[96]uintptr }

// Convert pointer arguments at this call site, never in a uintptr-returning
// helper. The directive keeps native out-buffers alive and off movable stacks,
// including when a Windows callback reenters Go during SyscallN.
//go:uintptrescapes
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

// Standard UIA/MSAA SetFocus can make a child HWND the active foreground
// handle. Public Window refs identify the owning top-level window.
func foregroundRoot() uintptr {
	hwnd, _, _ := foreground.Call()
	if hwnd != 0 {
		if root, _, _ := proc(user32, "GetAncestor").Call(hwnd, 2); root != 0 {
			return root
		}
	}
	return hwnd
}

var windowPID = proc(user32, "GetWindowThreadProcessId")
var metrics = proc(user32, "GetSystemMetrics")
var cursorPos = proc(user32, "GetCursorPos")
var setDPI = proc(user32, "SetThreadDpiAwarenessContext")
var openProcess = proc(kernel32, "OpenProcess")
var processTimes = proc(kernel32, "GetProcessTimes")
var waitForSingleObject = proc(kernel32, "WaitForSingleObject")
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
	e := c.call(slot, uintptr(unsafe.Pointer(&s)))
	if s != nil {
		defer freeBSTR.Call(uintptr(s))
	}
	if e != nil {
		return "", e
	}
	return strBSTR(s), nil
}
func intProp(c *com, slot int) (int32, error) {
	var v int32
	e := c.call(slot, uintptr(unsafe.Pointer(&v)))
	return v, e
}
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
	if e := c.call(slot, uintptr(id), uintptr(unsafe.Pointer(&v))); e != nil {
		return dw.Unknown[bool]()
	}
	defer variantClear.Call(uintptr(unsafe.Pointer(&v)))
	if v.VT != 11 {
		return dw.Unknown[bool]()
	}
	return dw.Known(v.Value&0xffff != 0)
}
func isTrue(v dw.Fact[bool]) bool { return v.Status == dw.FactKnown && v.Value != nil && *v.Value }
func processStart(pid uint32) uint64 {
	// Creation time survives process exit while another caller retains a handle.
	// Check the process signal as well, so exited instances cannot keep grants.
	h, _, _ := openProcess.Call(0x1000|0x100000, 0, uintptr(pid))
	if h == 0 {
		return 0
	}
	defer closeHandle.Call(h)
	if state, _, _ := waitForSingleObject.Call(h, 0); state != 0x102 {
		return 0
	}
	var creation, exit, kernel, user uint64
	ok, _, _ := processTimes.Call(h, uintptr(unsafe.Pointer(&creation)), uintptr(unsafe.Pointer(&exit)), uintptr(unsafe.Pointer(&kernel)), uintptr(unsafe.Pointer(&user)))
	if ok == 0 {
		return 0
	}
	if state, _, _ := waitForSingleObject.Call(h, 0); state != 0x102 {
		return 0
	}
	return creation
}
func processName(pid uint32) string {
	h, _, _ := openProcess.Call(0x1000, 0, uintptr(pid))
	if h == 0 {
		return fmt.Sprintf("Process %d", pid)
	}
	defer closeHandle.Call(h)
	var path [32768]uint16
	size := uint32(len(path))
	if ok, _, _ := proc(kernel32, "QueryFullProcessImageNameW").Call(h, 0, uintptr(unsafe.Pointer(&path[0])), uintptr(unsafe.Pointer(&size))); ok == 0 {
		return fmt.Sprintf("Process %d", pid)
	}
	name := filepath.Base(syscall.UTF16ToString(path[:size]))
	return strings.TrimSuffix(name, filepath.Ext(name))
}
func dpiScope() func() {
	if setDPI.Find() != nil {
		return func() {}
	}
	old, _, _ := setDPI.Call(^uintptr(3))
	return func() { setDPI.Call(old) }
}
