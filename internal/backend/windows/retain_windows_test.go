//go:build windows && amd64

package windows

import (
	"os"
	"runtime"
	"syscall"
	"testing"
	"unsafe"

	"github.com/caelis-labs/desktop-world/internal/backend"
)

func TestRetainReplacesInterfaceOnlyForSameLiveNativeIdentity(t *testing.T) {
	for _, same := range []bool{false, true} {
		t.Run(map[bool]string{false: "different element", true: "same element"}[same], func(t *testing.T) {
			var oldReleased, newReleased int
			var oldVT, newVT, uiaVT [96]uintptr
			oldVT[2] = syscall.NewCallback(func(_ uintptr) uintptr { oldReleased++; return 0 })
			newVT[2] = syscall.NewCallback(func(_ uintptr) uintptr { newReleased++; return 0 })
			pid := uint32(os.Getpid())
			newVT[20] = syscall.NewCallback(func(_ uintptr, out unsafe.Pointer) uintptr { *(*int32)(out) = int32(pid); return 0 })
			uiaVT[3] = syscall.NewCallback(func(_, _, _ uintptr, out unsafe.Pointer) uintptr {
				*(*int32)(out) = 0
				if same {
					*(*int32)(out) = 1
				}
				return 0
			})
			d := New().(*Driver)
			d.uia = &com{vt: &uiaVT}
			original := &com{vt: &oldVT}
			fresh := &com{vt: &newVT}
			e := &entry{key: "original", pid: pid, start: processStart(pid), el: original}
			d.entries = append(d.entries, e)
			d.byKey[e.key] = e
			key, err := retainWithCallbackStack(d, fresh)
			if err != nil {
				t.Fatal(err)
			}
			if same {
				if key != e.key || e.el != fresh || oldReleased != 1 || newReleased != 0 {
					t.Fatal("same identity kept stale interface", key, oldReleased, newReleased)
				}
			} else if key == e.key || e.el != original || oldReleased != 0 || newReleased != 0 {
				t.Fatal("different identity silently rebound", key, oldReleased, newReleased)
			}
			for _, node := range d.entries {
				node.el.release()
			}
		})
	}
}

// Test COM callbacks reenter Go, unlike the real native UIA provider. Reserve
// stack space before passing native out-pointers so callback stack growth cannot
// relocate a caller's stack allocation while its address is in a native frame.
func retainWithCallbackStack(d *Driver, el *com) (backend.Key, error) {
	var reserve [32768]byte
	reserve[0] = 1
	key, err := d.retain(el, "app", "window", "parent", 0)
	runtime.KeepAlive(&reserve)
	return key, err
}
