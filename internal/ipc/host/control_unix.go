//go:build !windows

package host

import (
	"os"
	"os/exec"
)

func prepareControl(cmd *exec.Cmd, in, out *os.File) (func(), error) {
	cmd.ExtraFiles = []*os.File{in, out}
	return func() {}, nil
}
