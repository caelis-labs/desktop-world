//go:build darwin && cgo

package darwin

/*
#cgo CFLAGS: -mmacosx-version-min=14.0
#cgo LDFLAGS: -framework AppKit -framework ApplicationServices -framework ScreenCaptureKit -framework ImageIO -framework UniformTypeIdentifiers
#include <stdlib.h>
void *dw_open(void);
char *dw_call(void *,const char *,const char *,void *);
void *dw_cancel_new(void);
void dw_cancel_signal(void *);
void dw_close(void *);
*/
import "C"

import (
	"context"
	"encoding/json"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"unsafe"
)

type Driver struct{ ptr unsafe.Pointer }

func New() backend.Driver { return &Driver{} }
func (d *Driver) Open(context.Context) error {
	d.ptr = C.dw_open()
	if d.ptr == nil {
		return fmt.Errorf("native backend allocation failed")
	}
	return nil
}
func (d *Driver) Close(context.Context) error {
	if d.ptr != nil {
		C.dw_close(d.ptr)
		d.ptr = nil
	}
	return nil
}
func (d *Driver) call(ctx context.Context, op string, in, out any) error {
	if e := ctx.Err(); e != nil {
		return e
	}
	data, e := json.Marshal(in)
	if e != nil {
		return e
	}
	a, b := C.CString(op), C.CString(string(data))
	defer C.free(unsafe.Pointer(a))
	defer C.free(unsafe.Pointer(b))
	flag := C.dw_cancel_new()
	if flag == nil {
		return dw.NewFault("resource_exhausted", "native cancellation allocation failed", "reobserve")
	}
	stopped := make(chan struct{})
	stop := context.AfterFunc(ctx, func() { C.dw_cancel_signal(flag); close(stopped) })
	p := C.dw_call(d.ptr, a, b, flag)
	if !stop() {
		<-stopped
	}
	C.free(flag)
	if p == nil {
		return dw.NewFault("provider_unavailable", "empty native response", "reobserve")
	}
	defer C.free(unsafe.Pointer(p))
	raw := []byte(C.GoString(p))
	var envelope struct {
		Result json.RawMessage
		Fault  *dw.Fault
	}
	if e = json.Unmarshal(raw, &envelope); e != nil {
		return fmt.Errorf("invalid native response: %w", e)
	}
	if envelope.Fault != nil {
		return envelope.Fault
	}
	return json.Unmarshal(envelope.Result, out)
}
func (d *Driver) Environment(c context.Context) (v dw.Environment, e error) {
	e = d.call(c, "environment", nil, &v)
	return
}
func (d *Driver) Permissions(c context.Context, r dw.PermissionRequest) (v []dw.Permission, e error) {
	e = d.call(c, "permissions", r, &v)
	return
}
func (d *Driver) Query(c context.Context, q backend.Query) (v backend.Page, e error) {
	e = d.call(c, "query", q, &v)
	return
}
func (d *Driver) Read(c context.Context, k backend.Key) (n backend.Node, s backend.Seat, e error) {
	var v struct {
		Node backend.Node
		Seat backend.Seat
	}
	e = d.call(c, "read", map[string]any{"Key": k}, &v)
	return v.Node, v.Seat, e
}
func (d *Driver) ReadText(c context.Context, k backend.Key) (v backend.Text, e error) {
	e = d.call(c, "text", map[string]any{"Key": k}, &v)
	return
}
func (d *Driver) Perform(c context.Context, o backend.Operation) (v backend.Outcome) {
	if c.Err() != nil {
		return backend.Outcome{Delivery: dw.DeliveryNone, Fault: dw.NewFault("cancelled", "cancelled before native dispatch", "reobserve")}
	}
	if e := d.call(c, "perform", o, &v); e != nil {
		v.Delivery = dw.DeliveryUnknown
		v.Unsafe = true
		f, ok := e.(*dw.Fault)
		if !ok {
			f = dw.NewFault("provider_unavailable", "native request failed", "reobserve")
		}
		v.Fault = f
	}
	return
}
func (d *Driver) HitTest(c context.Context, p dw.Point, k backend.Key) (v bool, e error) {
	e = d.call(c, "hit", map[string]any{"Point": p, "Key": k}, &v)
	return
}
func (d *Driver) Capture(c context.Context, r dw.CaptureRequest) (v []backend.Image, e error) {
	e = d.call(c, "capture", r, &v)
	return
}
