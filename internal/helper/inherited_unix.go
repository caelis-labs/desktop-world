//go:build !windows

package helper

import "os"

func InheritedControlFiles() (*os.File, *os.File, error) {
	return os.NewFile(3, "host-control-in"), os.NewFile(4, "host-control-out"), nil
}
