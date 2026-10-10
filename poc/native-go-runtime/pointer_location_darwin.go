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
