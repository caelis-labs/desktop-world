//go:build windows && amd64

package windows

import (
	"bytes"
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"image"
	"image/png"
	"math"
	"syscall"
	"unsafe"
)

func (d *Driver) Capture(ctx context.Context, r backend.CaptureRequest) ([]backend.Image, error) {
	if r.Kind == "window_content" {
		return d.captureWindow(ctx, r)
	}
	if r.Kind != "visible_region" || r.IncludeCursor {
		return nil, dw.NewFault("capability_unavailable", "visible region without cursor is supported", "reobserve")
	}
	restore := dpiScope()
	defer restore()
	dc, _, _ := proc(user32, "GetDC").Call(0)
	if dc == 0 {
		return nil, nativeFault(0x80004005)
	}
	defer proc(user32, "ReleaseDC").Call(0, dc)
	var images []backend.Image
	for _, display := range d.env.Displays {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		b := display.Bounds
		if r.Region != nil {
			q := r.Region.Rect
			x, y := math.Max(b.X, q.X), math.Max(b.Y, q.Y)
			right, bottom := math.Min(b.X+b.Width, q.X+q.Width), math.Min(b.Y+b.Height, q.Y+q.Height)
			b = dw.Rect{X: x, Y: y, Width: right - x, Height: bottom - y}
		}
		if b.Width <= 0 || b.Height <= 0 {
			continue
		}
		scale := math.Min(1, math.Min(float64(r.MaxPixelWidth)/b.Width, float64(r.MaxPixelHeight)/b.Height))
		w, h := int(math.Max(1, b.Width*scale)), int(math.Max(1, b.Height*scale))
		img, err := captureTile(dc, b, w, h)
		if err != nil {
			return nil, err
		}
		var buf bytes.Buffer
		if err = png.Encode(&buf, img); err != nil {
			return nil, err
		}
		images = append(images, backend.Image{Bytes: buf.Bytes(), ContentType: "image/png", Bounds: dw.Bounds{Frame: display.Frame, Topology: d.env.Topology, Rect: b}, Width: w, Height: h})
	}
	return images, nil
}
func captureTile(dc uintptr, b dw.Rect, w, h int) (*image.RGBA, error) {
	return renderBitmap(dc, w, h, func(mem uintptr) error {
		proc(gdi32, "SetStretchBltMode").Call(mem, 4)
		ok, _, _ := proc(gdi32, "StretchBlt").Call(mem, 0, 0, uintptr(w), uintptr(h), dc, uintptr(int32(b.X)), uintptr(int32(b.Y)), uintptr(int32(b.Width)), uintptr(int32(b.Height)), 0x00CC0020|0x40000000)
		if ok == 0 {
			return nativeFault(0x80004005)
		}
		return nil
	})
}
func renderBitmap(dc uintptr, w, h int, render func(uintptr) error) (*image.RGBA, error) {
	mem, _, _ := proc(gdi32, "CreateCompatibleDC").Call(dc)
	if mem == 0 {
		return nil, nativeFault(0x80004005)
	}
	defer proc(gdi32, "DeleteDC").Call(mem)
	var header struct {
		Size                   uint32
		Width, Height          int32
		Planes, BitCount       uint16
		Compression, SizeImage uint32
		XPels, YPels           int32
		ClrUsed, ClrImportant  uint32
		Colors                 [1]uint32
	}
	header.Size = 40
	header.Width = int32(w)
	header.Height = -int32(h)
	header.Planes = 1
	header.BitCount = 32
	var bits unsafe.Pointer
	bitmap, _, _ := proc(gdi32, "CreateDIBSection").Call(dc, uintptr(unsafe.Pointer(&header)), 0, uintptr(unsafe.Pointer(&bits)), 0, 0)
	if bitmap == 0 {
		return nil, nativeFault(0x80004005)
	}
	defer proc(gdi32, "DeleteObject").Call(bitmap)
	old, _, _ := proc(gdi32, "SelectObject").Call(mem, bitmap)
	defer proc(gdi32, "SelectObject").Call(mem, old)
	if err := render(mem); err != nil {
		return nil, err
	}
	raw := unsafe.Slice((*byte)(bits), w*h*4)
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for i := 0; i < len(raw); i += 4 {
		img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = raw[i+2], raw[i+1], raw[i], 255
	}
	return img, nil
}

// Experimental provider rendering. PrintWindow is synchronous and provider-owned;
// the managed helper bounds caller waits and can terminate a stuck worker. There
// is deliberately no shared-desktop blit fallback or Windows availability claim.
func (d *Driver) captureWindow(ctx context.Context, r backend.CaptureRequest) ([]backend.Image, error) {
	restore := dpiScope()
	defer restore()
	e, err := d.lookup(r.Key)
	if err != nil {
		return nil, err
	}
	if e.hwnd == 0 || e.application || r.IncludeCursor || r.Region != nil {
		return nil, dw.NewFault("capability_unavailable", "a native top-level window is required", "reobserve")
	}
	// A locked/disconnected interactive desktop must never serve old pixels.
	desktop, _, _ := proc(user32, "OpenInputDesktop").Call(0, 0, 1)
	if desktop == 0 {
		return nil, dw.NewFault("seat_unavailable", "input desktop unavailable", "reobserve")
	}
	defer proc(user32, "CloseDesktop").Call(desktop)
	var name [256]uint16
	var needed uint32
	ok, _, _ := proc(user32, "GetUserObjectInformationW").Call(desktop, 2, uintptr(unsafe.Pointer(&name)), uintptr(len(name)*2), uintptr(unsafe.Pointer(&needed)))
	if ok == 0 || syscall.UTF16ToString(name[:]) != "Default" {
		return nil, dw.NewFault("seat_unavailable", "interactive desktop is not active", "reobserve")
	}
	before, err := windowCaptureRect(e.hwnd)
	if err != nil {
		return nil, err
	}
	w, h := int(before[2]-before[0]), int(before[3]-before[1])
	if w <= 0 || h <= 0 || w > 8192 || h > 8192 || w*h > 16<<20 {
		return nil, dw.NewFault("resource_exhausted", "window surface too large", "reobserve")
	}
	img, err := renderWindow(ctx, e.hwnd)
	if err != nil {
		return nil, err
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	if _, err = d.lookup(r.Key); err != nil {
		return nil, err
	}
	// UIA instance identity must still be readable even if a HWND was recycled.
	if _, err = intProp(e.el, 20); err != nil {
		return nil, err
	}
	after, err := windowCaptureRect(e.hwnd)
	if err != nil {
		return nil, err
	}
	if before != after {
		return nil, dw.NewFault("capture_geometry_changed", "window moved while capturing", "reobserve")
	}
	scale := math.Min(1, math.Min(float64(r.MaxPixelWidth)/float64(w), float64(r.MaxPixelHeight)/float64(h)))
	outW, outH := max(1, int(float64(w)*scale)), max(1, int(float64(h)*scale))
	sampleW, sampleH := img.Bounds().Dx(), img.Bounds().Dy()
	if outW != sampleW || outH != sampleH {
		small := image.NewRGBA(image.Rect(0, 0, outW, outH))
		for y := 0; y < outH; y++ {
			for x := 0; x < outW; x++ {
				small.SetRGBA(x, y, img.RGBAAt(x*sampleW/outW, y*sampleH/outH))
			}
		}
		img = small
	}
	var data bytes.Buffer
	if err = png.Encode(&data, img); err != nil {
		return nil, err
	}
	return []backend.Image{{Bytes: data.Bytes(), ContentType: "image/png", Width: outW, Height: outH, Bounds: dw.Bounds{Frame: "window", Topology: d.env.Topology, Rect: dw.Rect{Width: float64(w), Height: float64(h)}}}}, nil
}

// Legacy DPI-unaware providers paint their virtualized dimensions, even when
// the capture worker observes physical pixels. Render in the window's context,
// then map that complete surface to the physical window frame above. Restore
// worker awareness before any UIA/lifetime/geometry checks.
func renderWindow(ctx context.Context, hwnd uintptr) (*image.RGBA, error) {
	awareness, _, _ := proc(user32, "GetWindowDpiAwarenessContext").Call(hwnd)
	if awareness == 0 {
		return nil, dw.NewFault("ref_gone", "window DPI context unavailable", "reobserve")
	}
	previous, _, _ := setDPI.Call(awareness)
	if previous == 0 {
		return nil, dw.NewFault("capture_frame_unavailable", "window DPI context cannot be entered", "reobserve")
	}
	defer setDPI.Call(previous)
	var rect [4]int32
	if ok, _, _ := proc(user32, "GetWindowRect").Call(hwnd, uintptr(unsafe.Pointer(&rect))); ok == 0 {
		return nil, dw.NewFault("ref_gone", "window render geometry unavailable", "reobserve")
	}
	w, h := int(rect[2]-rect[0]), int(rect[3]-rect[1])
	if w <= 0 || h <= 0 || w > 8192 || h > 8192 || w*h > 16<<20 {
		return nil, dw.NewFault("resource_exhausted", "window render surface too large", "reobserve")
	}
	dc, _, _ := proc(user32, "GetDC").Call(hwnd)
	if dc == 0 {
		return nil, nativeFault(0x80004005)
	}
	defer proc(user32, "ReleaseDC").Call(hwnd, dc)
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return renderBitmap(dc, w, h, func(mem uintptr) error {
		// PW_RENDERFULLCONTENT supports compositor-backed providers. There is
		// no desktop blit, forced repaint or second rendering attempt.
		if ok, _, _ := proc(user32, "PrintWindow").Call(hwnd, mem, 2); ok == 0 {
			return dw.NewFault("capture_frame_unavailable", "provider did not render the window", "reobserve")
		}
		return nil
	})
}
func windowCaptureRect(hwnd uintptr) ([4]int32, error) {
	var rect [4]int32
	visible, _, _ := isVisible.Call(hwnd)
	minimized, _, _ := proc(user32, "IsIconic").Call(hwnd)
	var cloaked uint32
	hr, _, _ := proc(syscall.NewLazyDLL("dwmapi.dll"), "DwmGetWindowAttribute").Call(hwnd, 14, uintptr(unsafe.Pointer(&cloaked)), 4)
	if visible == 0 || minimized != 0 || cloaked != 0 {
		return rect, dw.NewFault("window_not_visible", "window is hidden, minimized or cloaked", "reobserve")
	}
	if int32(hr) < 0 {
		return rect, nativeFault(hr)
	}
	ok, _, _ := proc(user32, "GetWindowRect").Call(hwnd, uintptr(unsafe.Pointer(&rect)))
	if ok == 0 {
		return rect, dw.NewFault("ref_gone", "window is gone", "reobserve")
	}
	return rect, nil
}
