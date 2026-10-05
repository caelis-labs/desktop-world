//go:build windows && amd64

package main

import (
	"os"
	"unsafe"
)

// Use the modern Common Controls provider in the fixture, without installing
// resources or changing machine-wide activation/foreground settings.
func commonControls() func() {
	f, err := os.CreateTemp("", "dtw-controls-*.manifest")
	if err != nil {
		panic(err)
	}
	name := f.Name()
	_, err = f.WriteString(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<assemblyIdentity version="1.0.0.0" processorArchitecture="amd64" name="DTW.Fixture" type="win32"/>
<dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="amd64" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>
</assembly>`)
	f.Close()
	if err != nil {
		panic(err)
	}
	defer os.Remove(name)
	ctx := struct {
		Size, Flags                      uint32
		Source                           *uint16
		Architecture, Language           uint16
		Directory, Resource, Application *uint16
		Module                           uintptr
	}{Source: wide(name)}
	ctx.Size = uint32(unsafe.Sizeof(ctx))
	handle, _, _ := k.NewProc("CreateActCtxW").Call(p(&ctx))
	if handle == ^uintptr(0) {
		panic("Common Controls activation context unavailable")
	}
	var cookie uintptr
	if ok, _, _ := k.NewProc("ActivateActCtx").Call(handle, p(&cookie)); ok == 0 {
		panic("Common Controls activation failed")
	}
	return func() {
		k.NewProc("DeactivateActCtx").Call(0, cookie)
		k.NewProc("ReleaseActCtx").Call(handle)
	}
}
