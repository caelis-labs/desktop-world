//go:build darwin && cgo

package main

/*
#cgo LDFLAGS: -framework ApplicationServices
#include <ApplicationServices/ApplicationServices.h>
#include <math.h>
static CGPoint dtwPOCPointerLocation(void) {
  CGEventRef event = CGEventCreate(NULL);
  if (!event) return CGPointMake(NAN, NAN);
  CGPoint point = CGEventGetLocation(event);
  CFRelease(event);
  return point;
}
static CGWindowID dtwPOCOnScreenWindowForPID(pid_t pid) {
  CFArrayRef windows = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID);
  if (!windows) return 0;
  CGWindowID found = 0;
  for (CFIndex i = 0; i < CFArrayGetCount(windows); i++) {
    CFDictionaryRef row = CFArrayGetValueAtIndex(windows, i);
    CFNumberRef owner = CFDictionaryGetValue(row, kCGWindowOwnerPID);
    CFNumberRef number = CFDictionaryGetValue(row, kCGWindowNumber);
    int candidate = 0;
    if (owner && number && CFNumberGetValue(owner, kCFNumberIntType, &candidate) && candidate == pid) {
      CFNumberGetValue(number, kCFNumberIntType, &found);
      break;
    }
  }
  CFRelease(windows);
  return found;
}
*/
import "C"

import (
	"errors"
	"math"
)

func pointerLocation() (float64, float64, error) {
	point := C.dtwPOCPointerLocation()
	x, y := float64(point.x), float64(point.y)
	if math.IsNaN(x) || math.IsNaN(y) {
		return 0, 0, errors.New("pointer location unavailable")
	}
	return x, y, nil
}

func onScreenWindowForPID(pid int) uint32 {
	return uint32(C.dtwPOCOnScreenWindowForPID(C.pid_t(pid)))
}
