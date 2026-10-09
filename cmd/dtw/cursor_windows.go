//go:build windows && amd64

package main

import (
	"fmt"
	"math"
	"syscall"
	"unsafe"
)

var cursorUser32 = syscall.NewLazyDLL("user32.dll")
var cursorGdi32 = syscall.NewLazyDLL("gdi32.dll")
var cursorWindow uintptr
var cursorDC uintptr
var cursorBitmap uintptr
var cursorOldBitmap uintptr
var cursorBits unsafe.Pointer

const cursorWidth, cursorHeight = 18, 22

type cursorVertex struct{ x, y float64 }

// The Windows bitmap uses top-left coordinates. The paper plane's nose is
// anchored to the delivered desktop point, matching the macOS view.
var cursorPlane = [...]cursorVertex{
	{2, 2}, {15.5, 8.2}, {15.9, 10.5}, {10.4, 11},
	{11.7, 17.1}, {9.1, 18.6}, {2.8, 10.7},
}

type cursorBitmapInfo struct {
	Size                   uint32
	Width, Height          int32
	Planes, BitCount       uint16
	Compression, SizeImage uint32
	XPels, YPels           int32
	ClrUsed, ClrImportant  uint32
}
type cursorPoint struct{ X, Y int32 }
type cursorSize struct{ W, H int32 }
type cursorBlend struct{ Op, Flags, Alpha, Format byte }

func cursorCall(dll *syscall.LazyDLL, name string, args ...uintptr) uintptr {
	v, _, _ := dll.NewProc(name).Call(args...)
	return v
}

func cursorOverlayInit() error {
	// DTW desktop points are physical display coordinates. Use per-monitor DPI
	// awareness before creating a native window, without changing input state.
	cursorCall(cursorUser32, "SetProcessDpiAwarenessContext", ^uintptr(3))
	class := syscall.StringToUTF16Ptr("STATIC")
	cursorWindow = cursorCall(cursorUser32, "CreateWindowExW",
		0x00080000|0x00000020|0x00000080|0x00000008|0x08000000,
		uintptr(unsafe.Pointer(class)), 0, 0x80000000,
		0, 0, cursorWidth, cursorHeight, 0, 0, 0, 0)
	if cursorWindow == 0 {
		return fmt.Errorf("cannot create layered click-through window")
	}
	cursorDC = cursorCall(cursorGdi32, "CreateCompatibleDC", 0)
	if cursorDC == 0 {
		cursorOverlayClose()
		return fmt.Errorf("cannot create cursor surface")
	}
	info := cursorBitmapInfo{Size: 40, Width: cursorWidth, Height: -cursorHeight, Planes: 1, BitCount: 32}
	cursorBitmap = cursorCall(cursorGdi32, "CreateDIBSection", cursorDC, uintptr(unsafe.Pointer(&info)), 0, uintptr(unsafe.Pointer(&cursorBits)), 0, 0)
	if cursorBitmap == 0 || cursorBits == nil {
		cursorOverlayClose()
		return fmt.Errorf("cannot create cursor bitmap")
	}
	cursorOldBitmap = cursorCall(cursorGdi32, "SelectObject", cursorDC, cursorBitmap)
	cursorPaint(unsafe.Slice((*byte)(cursorBits), cursorWidth*cursorHeight*4))
	return nil
}

func cursorPaint(pixels []byte) {
	// Supersampling gives the narrow outline and diagonal edges smooth alpha
	// without GDI drawing state or a background that could obscure the target.
	const samples = 4
	for y := 0; y < cursorHeight; y++ {
		for x := 0; x < cursorWidth; x++ {
			var red, green, blue, alpha float64
			for sy := 0; sy < samples; sy++ {
				for sx := 0; sx < samples; sx++ {
					px := float64(x) + (float64(sx)+0.5)/samples
					py := float64(y) + (float64(sy)+0.5)/samples
					if !cursorContains(px, py) {
						continue
					}
					// Cyan, violet and soft pink color the plane itself; there
					// is no background disk or halo around the pointer.
					t := math.Max(0, math.Min(1, (px+py*0.45-3)/20))
					var r, g, b float64
					if t < 0.52 {
						u := t / 0.52
						r, g, b = 0.28+0.30*u, 0.83-0.35*u, 1-0.02*u
					} else {
						u := (t - 0.52) / 0.48
						r, g, b = 0.58+0.40*u, 0.48+0.23*u, 0.98-0.12*u
					}
					foldDistance := math.Min(
						cursorSegmentDistance(px, py, cursorVertex{4.7, 5.2}, cursorVertex{9.4, 10.7}),
						cursorSegmentDistance(px, py, cursorVertex{9.4, 10.7}, cursorVertex{10.1, 16.2}),
					)
					if cursorEdgeDistance(px, py) <= 0.38 {
						r, g, b = r*0.25+0.75, g*0.25+0.75, b*0.25+0.75
					} else if foldDistance <= 0.50 {
						r, g, b = r*0.13+0.87, g*0.13+0.87, b*0.13+0.87
					}
					red += r * 0.97
					green += g * 0.97
					blue += b * 0.97
					alpha += 0.97
				}
			}
			i := (y*cursorWidth + x) * 4
			pixels[i] = byte(math.Round(blue * 255 / (samples * samples)))
			pixels[i+1] = byte(math.Round(green * 255 / (samples * samples)))
			pixels[i+2] = byte(math.Round(red * 255 / (samples * samples)))
			pixels[i+3] = byte(math.Round(alpha * 255 / (samples * samples)))
		}
	}
}

func cursorContains(x, y float64) bool {
	inside := false
	for i, current := range cursorPlane {
		previous := cursorPlane[(i+len(cursorPlane)-1)%len(cursorPlane)]
		if (current.y > y) != (previous.y > y) &&
			x < (previous.x-current.x)*(y-current.y)/(previous.y-current.y)+current.x {
			inside = !inside
		}
	}
	return inside
}

func cursorEdgeDistance(x, y float64) float64 {
	closest := math.MaxFloat64
	for i, start := range cursorPlane {
		end := cursorPlane[(i+1)%len(cursorPlane)]
		closest = math.Min(closest, cursorSegmentDistance(x, y, start, end))
	}
	return closest
}

func cursorSegmentDistance(x, y float64, start, end cursorVertex) float64 {
	dx, dy := end.x-start.x, end.y-start.y
	t := math.Max(0, math.Min(1, ((x-start.x)*dx+(y-start.y)*dy)/(dx*dx+dy*dy)))
	return math.Hypot(x-start.x-t*dx, y-start.y-t*dy)
}

func cursorOverlayShow(x, y float64) error {
	if cursorWindow == 0 || cursorDC == 0 {
		return fmt.Errorf("cursor overlay closed")
	}
	position := cursorPoint{int32(x) - 2, int32(y) - 2}
	size := cursorSize{cursorWidth, cursorHeight}
	source := cursorPoint{}
	blend := cursorBlend{Alpha: 255, Format: 1}
	if cursorCall(cursorUser32, "UpdateLayeredWindow", cursorWindow, 0, uintptr(unsafe.Pointer(&position)), uintptr(unsafe.Pointer(&size)), cursorDC, uintptr(unsafe.Pointer(&source)), 0, uintptr(unsafe.Pointer(&blend)), 2) == 0 {
		return fmt.Errorf("cannot draw layered cursor")
	}
	cursorCall(cursorUser32, "ShowWindow", cursorWindow, 4) // SW_SHOWNOACTIVATE
	return nil
}
func cursorOverlayHide() {
	if cursorWindow != 0 {
		cursorCall(cursorUser32, "ShowWindow", cursorWindow, 0)
	}
}
func cursorOverlayPoll() {
	var message [64]byte
	for cursorCall(cursorUser32, "PeekMessageW", uintptr(unsafe.Pointer(&message[0])), 0, 0, 0, 1) != 0 {
		cursorCall(cursorUser32, "TranslateMessage", uintptr(unsafe.Pointer(&message[0])))
		cursorCall(cursorUser32, "DispatchMessageW", uintptr(unsafe.Pointer(&message[0])))
	}
}
func cursorOverlayClose() {
	cursorOverlayHide()
	if cursorDC != 0 && cursorOldBitmap != 0 {
		cursorCall(cursorGdi32, "SelectObject", cursorDC, cursorOldBitmap)
	}
	if cursorBitmap != 0 {
		cursorCall(cursorGdi32, "DeleteObject", cursorBitmap)
	}
	if cursorDC != 0 {
		cursorCall(cursorGdi32, "DeleteDC", cursorDC)
	}
	if cursorWindow != 0 {
		cursorCall(cursorUser32, "DestroyWindow", cursorWindow)
	}
	cursorWindow, cursorDC, cursorBitmap, cursorOldBitmap, cursorBits = 0, 0, 0, 0, nil
}
