//go:build windows && amd64

package windows

import (
	"context"
	"os"
	"syscall"
	"testing"
	"unsafe"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
)

// A stale client bypassing capability discovery must still be stopped before
// obtaining or dispatching the legacy ScrollItem pattern. Other control families
// continue to reach their provider. No UI or physical input is sent here.
func TestScrollGuardRunsBeforePatternDispatch(t *testing.T) {
	for _, tc := range []struct {
		name, class, parentClass string
		control                  int32
		patternCalls             int
	}{
		{"legacy tree item", "SysTreeView32", "", 50024, 0},
		{"legacy child with empty class", "", "SysTreeView32", 50024, 0},
		{"modern tree item", "TreeViewItem", "", 50024, 1},
		{"list item", "ListBox", "", 50007, 1},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var calls int
			var vt [96]uintptr
			vt[21] = syscall.NewCallback(func(_ uintptr, out unsafe.Pointer) uintptr {
				*(*int32)(out) = tc.control
				return 0
			})
			vt[30] = syscall.NewCallback(func(_ uintptr, out unsafe.Pointer) uintptr {
				class, _ := syscall.UTF16PtrFromString(tc.class)
				bstr, _, _ := proc(oleaut, "SysAllocString").Call(ptr(class))
				*(*uintptr)(out) = bstr
				return 0
			})
			vt[16] = syscall.NewCallback(func(_, _ uintptr, out unsafe.Pointer) uintptr {
				calls++
				*(*uintptr)(out) = 0
				return 0
			})
			pid := uint32(os.Getpid())
			d := New().(*Driver)
			var parentVT, walkerVT [96]uintptr
			parentVT[2] = syscall.NewCallback(func(_ uintptr) uintptr { return 0 })
			parentVT[30] = syscall.NewCallback(func(_ uintptr, out unsafe.Pointer) uintptr {
				class, _ := syscall.UTF16PtrFromString(tc.parentClass)
				bstr, _, _ := proc(oleaut, "SysAllocString").Call(ptr(class))
				*(*uintptr)(out) = bstr
				return 0
			})
			parent := &com{vt: &parentVT}
			walkerVT[3] = syscall.NewCallback(func(_, _ uintptr, out unsafe.Pointer) uintptr {
				*(**com)(out) = nil
				if tc.parentClass != "" {
					*(**com)(out) = parent
				}
				return 0
			})
			d.walker = &com{vt: &walkerVT}
			e := &entry{key: "scroll-target", pid: pid, start: processStart(pid), el: &com{vt: &vt}}
			d.byKey[e.key] = e
			out := d.Perform(context.Background(), backend.Operation{Key: e.key, Step: dw.Step{Op: "scroll_into_view"}})
			if out.Delivery != dw.DeliveryNone || out.Fault == nil || out.Fault.Code != "capability_unavailable" || calls != tc.patternCalls {
				t.Fatalf("dispatch guard: outcome=%+v pattern calls=%d", out, calls)
			}
		})
	}
}
