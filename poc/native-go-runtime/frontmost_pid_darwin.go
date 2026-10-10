//go:build darwin && cgo

package main

/*
#cgo LDFLAGS: -framework AppKit
int dtwPOCFrontmostPID(void);
*/
import "C"

func frontmostPID() int { return int(C.dtwPOCFrontmostPID()) }
