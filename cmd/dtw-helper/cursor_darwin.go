//go:build darwin && cgo

package main

/*
#cgo CFLAGS: -fobjc-arc -mmacosx-version-min=14.0
#cgo LDFLAGS: -framework AppKit -framework CoreGraphics
int dw_cursor_init(void);
int dw_cursor_show(double x, double y);
void dw_cursor_hide(void);
void dw_cursor_poll(void);
void dw_cursor_close(void);
*/
import "C"
import "fmt"

func cursorOverlayInit() error {
	if C.dw_cursor_init() == 0 {
		return fmt.Errorf("cannot create nonactivating cursor window")
	}
	return nil
}
func cursorOverlayShow(x, y float64) error {
	if C.dw_cursor_show(C.double(x), C.double(y)) == 0 {
		return fmt.Errorf("desktop point is outside active displays")
	}
	return nil
}
func cursorOverlayHide()  { C.dw_cursor_hide() }
func cursorOverlayPoll()  { C.dw_cursor_poll() }
func cursorOverlayClose() { C.dw_cursor_close() }
