//go:build windows && amd64

package main

import (
	"fmt"
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

const cursorWidth, cursorHeight = 24, 30

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
	pixels := unsafe.Slice((*byte)(cursorBits), cursorWidth*cursorHeight*4)
	for y := 0; y < cursorHeight; y++ {
		for x := 0; x < cursorWidth; x++ {
			// Compact arrow with a white edge and blue interior. Coordinates are
			// confined to this translucent window and never represent input.
			inside := (y >= 2 && y <= 24 && x >= 3 && x <= 3+(y-2)*2/3) || (y >= 17 && y <= 27 && x >= 8 && x <= 12)
			if !inside {
				continue
			}
			edge := x == 3 || x == 3+(y-2)*2/3 || y == 2 || y == 24 || x == 8 || x == 12 || y == 27
			i := (y*cursorWidth + x) * 4
			if edge {
				pixels[i], pixels[i+1], pixels[i+2], pixels[i+3] = 255, 255, 255, 255
			} else {
				pixels[i], pixels[i+1], pixels[i+2], pixels[i+3] = 211, 112, 24, 245
			}
		}
	}
	return nil
}

func cursorOverlayShow(x, y float64) error {
	if cursorWindow == 0 || cursorDC == 0 {
		return fmt.Errorf("cursor overlay closed")
	}
	position := cursorPoint{int32(x) - 3, int32(y) - 2}
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
