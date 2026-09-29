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
	"unsafe"
)

func (d *Driver) Capture(ctx context.Context, r dw.CaptureRequest) ([]backend.Image, error) {
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
	bitmap, _, _ := proc(gdi32, "CreateDIBSection").Call(dc, ptr(&header), 0, ptr(&bits), 0, 0)
	if bitmap == 0 {
		return nil, nativeFault(0x80004005)
	}
	defer proc(gdi32, "DeleteObject").Call(bitmap)
	old, _, _ := proc(gdi32, "SelectObject").Call(mem, bitmap)
	defer proc(gdi32, "SelectObject").Call(mem, old)
	proc(gdi32, "SetStretchBltMode").Call(mem, 4)
	ok, _, _ := proc(gdi32, "StretchBlt").Call(mem, 0, 0, uintptr(w), uintptr(h), dc, uintptr(int32(b.X)), uintptr(int32(b.Y)), uintptr(int32(b.Width)), uintptr(int32(b.Height)), 0x00CC0020|0x40000000)
	if ok == 0 {
		return nil, nativeFault(0x80004005)
	}
	raw := unsafe.Slice((*byte)(bits), w*h*4)
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for i := 0; i < len(raw); i += 4 {
		img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = raw[i+2], raw[i+1], raw[i], 255
	}
	return img, nil
}
