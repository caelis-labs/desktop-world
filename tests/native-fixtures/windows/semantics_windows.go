//go:build windows && amd64

package main

import (
	"encoding/json"
	"fmt"
	"syscall"
	"unsafe"
)

var checkbox, choices, tree, treeRoot, treeLast, fieldCover uintptr

type treeItem struct {
	Mask                                     uint32
	Item                                     uintptr
	State, StateMask                         uint32
	Text                                     *uint16
	TextSize, Image, SelectedImage, Children int32
	Param                                    uintptr
}

func createSemantics(instance uintptr) {
	call("SetWindowPos", window, 0, 180, 180, 700, 620, 0x14)
	checkbox = call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("Receive updates")), 0x50010003, 24, 215, 220, 32, window, 20, instance, 0)
	call("CreateWindowExW", 0, p(wide("STATIC")), p(wide("Destinations")), 0x50000000, 24, 260, 220, 24, window, 0, instance, 0)
	choices = call("CreateWindowExW", 0, p(wide("LISTBOX")), p(wide("")), 0x50810801, 24, 290, 220, 120, window, 21, instance, 0)
	for _, name := range []string{"Beijing", "Shanghai", "Shenzhen"} {
		call("SendMessageW", choices, 0x180, 0, p(wide(name)))
	}
	for i := 0; i < 80; i++ {
		call("SendMessageW", choices, 0x180, 0, p(wide(fmt.Sprintf("Destination %02d", i))))
	}
	call("SendMessageW", choices, 0x185, 1, 0)
	init := struct{ Size, Classes uint32 }{8, 2}
	cc := syscall.NewLazyDLL("comctl32.dll")
	cc.NewProc("InitCommonControlsEx").Call(p(&init))
	tree = call("CreateWindowExW", 0, p(wide("SysTreeView32")), p(wide("Orders")), 0x50810007, 280, 215, 350, 320, window, 22, instance, 0)
	insert := func(parent uintptr, name string) uintptr {
		v := struct {
			Parent, After uintptr
			Item          treeItem
		}{parent, ^uintptr(0xfffd), treeItem{Mask: 1, Text: wide(name)}}
		return call("SendMessageW", tree, 0x1132, 0, p(&v))
	}
	treeRoot = insert(^uintptr(0xffff), "Order archive")
	for i := 0; i < 80; i++ {
		treeLast = insert(treeRoot, fmt.Sprintf("Invoice %02d", i))
	}
	call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("Inspect state")), 0x50010000, 24, 455, 220, 32, window, 23, instance, 0)
	call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("Cover input")), 0x50010000, 24, 505, 110, 32, window, 24, instance, 0)
	call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("Uncover input")), 0x50010000, 150, 505, 130, 32, window, 25, instance, 0)
	for i, name := range []string{"Resize fixture", "Minimize briefly", "Hide briefly"} {
		call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide(name)), 0x50010000, uintptr(300+i*115), 505, 110, 32, window, uintptr(27+i), instance, 0)
	}
}

func coverField() {
	instance, _, _ := k.NewProc("GetModuleHandleW").Call(0)
	fieldCover = call("CreateWindowExW", 0, p(wide("BUTTON")), p(wide("Covered field")), 0x50000000, 24, 62, 430, 30, window, 26, instance, 0)
	call("SetWindowPos", fieldCover, 0, 0, 0, 0, 0, 0x13)
	record("covered", "")
}

// Business evidence uses native control messages, independent of dtw/UIA.
func semanticSnapshot() {
	selected := []string{}
	for i, name := range []string{"Beijing", "Shanghai", "Shenzhen"} {
		if call("SendMessageW", choices, 0x187, uintptr(i), 0) > 0 {
			selected = append(selected, name)
		}
	}
	item := treeItem{Mask: 8, Item: treeRoot, StateMask: 0x20}
	call("SendMessageW", tree, 0x113e, 0, p(&item))
	rect := [4]int32{}
	*(*uintptr)(unsafe.Pointer(&rect[0])) = treeLast
	visible := call("SendMessageW", tree, 0x1104, 1, p(&rect)) != 0
	client := [4]int32{}
	call("GetClientRect", tree, p(&client))
	visible = visible && rect[1] >= 0 && rect[3] <= client[3]
	listRect, listClient := [4]int32{}, [4]int32{}
	listVisible := int32(call("SendMessageW", choices, 0x198, 82, p(&listRect))) != -1
	call("GetClientRect", choices, p(&listClient))
	listVisible = listVisible && listRect[3] > 0 && listRect[1] < listClient[3]
	b, _ := json.Marshal(map[string]any{"checked": call("SendMessageW", checkbox, 0xf0, 0, 0) == 1,
		"selected": selected, "expanded": item.State&0x20 != 0, "last_visible": visible, "last_choice_visible": listVisible})
	record("state", string(b))
}
