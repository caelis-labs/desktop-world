//go:build windows

package helper

import (
	"fmt"
	"os"
	"strconv"
	"syscall"
)

// Handles are trusted process-start metadata, never agent arguments.
func InheritedControlFiles() (*os.File, *os.File, error) {
	handles := make([]uintptr, 2)
	for i, key := range []string{"DTW_CONTROL_IN_HANDLE", "DTW_CONTROL_OUT_HANDLE"} {
		value := os.Getenv(key)
		os.Unsetenv(key)
		n, err := strconv.ParseUint(value, 10, strconv.IntSize)
		if err != nil || n == 0 {
			return nil, nil, fmt.Errorf("managed mode requires private inherited pipe handles")
		}
		handles[i] = uintptr(n)
		kind, err := syscall.GetFileType(syscall.Handle(n))
		if err != nil || kind != syscall.FILE_TYPE_PIPE {
			return nil, nil, fmt.Errorf("invalid private control pipe")
		}
	}
	for _, h := range handles {
		if err := syscall.SetHandleInformation(syscall.Handle(h), syscall.HANDLE_FLAG_INHERIT, 0); err != nil {
			return nil, nil, err
		}
	}
	if handles[0] == handles[1] {
		return nil, nil, fmt.Errorf("control request and reply handles must differ")
	}
	return os.NewFile(handles[0], "host-control-in"), os.NewFile(handles[1], "host-control-out"), nil
}
