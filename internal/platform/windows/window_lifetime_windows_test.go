//go:build windows && amd64

package windows

import (
	"io"
	"os"
	"os/exec"
	"runtime"
	"syscall"
	"testing"
	"unsafe"

	dw "github.com/caelis-labs/desktop-world/internal/world"
)

// The child represents a live provider hosted outside the HWND owner. No UIA
// actions, foreground changes or input are performed by these lifetime tests.
func TestWindowProviderProcessHelper(t *testing.T) {
	if os.Getenv("DTW_WINDOW_PROVIDER_HELPER") != "1" {
		return
	}
	_, _ = io.Copy(io.Discard, os.Stdin)
}

func TestWindowLookupKeepsProviderAndOwnerLifetimesSeparate(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	class, _ := syscall.UTF16PtrFromString("STATIC")
	hwnd, _, nativeErr := proc(user32, "CreateWindowExW").Call(0, uintptr(unsafe.Pointer(class)), 0, 0, 0, 0, 1, 1, 0, 0, 0, 0)
	if hwnd == 0 {
		t.Fatal("create owned hidden window:", nativeErr)
	}
	defer proc(user32, "DestroyWindow").Call(hwnd)
	child := exec.Command(os.Args[0], "-test.run=^TestWindowProviderProcessHelper$")
	child.Env = append(os.Environ(), "DTW_WINDOW_PROVIDER_HELPER=1")
	child.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: 0x08000000}
	stdin, err := child.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err = child.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = stdin.Close(); _ = child.Wait() }()
	providerPID, ownerPID := uint32(child.Process.Pid), uint32(os.Getpid())
	// Keep the kernel process object alive after Wait closes exec's handle.
	// Python's Popen similarly retains its handle after a fixture exits.
	retained, _, errNative := openProcess.Call(0x1000|0x100000, 0, uintptr(providerPID))
	if retained == 0 {
		t.Fatal("retain provider process handle:", errNative)
	}
	defer closeHandle.Call(retained)
	providerStart, ownerStart := processStart(providerPID), processStart(ownerPID)
	if providerStart == 0 || ownerStart == 0 || providerPID == ownerPID {
		t.Fatal("independent process lifetimes unavailable")
	}
	d := New().(*Driver)
	newEntry := func() *entry {
		e := &entry{key: "cross-process-window", hwnd: hwnd, pid: providerPID, start: providerStart, windowPID: ownerPID, windowStart: ownerStart}
		d.byKey[e.key] = e
		return e
	}
	e := newEntry()
	if _, err = d.lookup(e.key); err != nil {
		t.Fatal("live provider/owner pair was rejected:", err)
	}
	for _, tc := range []struct {
		name   string
		mutate func(*entry)
	}{
		{"provider restart", func(e *entry) { e.start++ }},
		{"owner restart", func(e *entry) { e.windowStart++ }},
		{"HWND reassigned to provider", func(e *entry) { e.windowPID = providerPID }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			e := newEntry()
			tc.mutate(e)
			_, err := d.lookup(e.key)
			fault, ok := err.(*dw.Fault)
			if !ok || fault.Code != "ref_gone" || !e.gone {
				t.Fatal("changed identity not rejected:", err)
			}
		})
	}
	e = newEntry()
	_ = stdin.Close()
	if err = child.Wait(); err != nil {
		t.Fatal(err)
	}
	if processStart(providerPID) != 0 {
		t.Fatal("exited process with a retained handle reported a live identity")
	}
	if _, err = d.lookup(e.key); err == nil {
		t.Fatal("exited provider retained a live window Ref")
	}
}
