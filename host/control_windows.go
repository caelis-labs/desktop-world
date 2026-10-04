//go:build windows

package host

import (
	"fmt"
	"os"
	"os/exec"
	"syscall"
)

func prepareControl(cmd *exec.Cmd, in, out *os.File) (func(), error) {
	process, err := syscall.GetCurrentProcess()
	if err != nil {
		return nil, err
	}
	var handles []syscall.Handle
	closeHandles := func() {
		for _, h := range handles {
			syscall.CloseHandle(h)
		}
	}
	for _, f := range []*os.File{in, out} {
		var h syscall.Handle
		if err = syscall.DuplicateHandle(process, syscall.Handle(f.Fd()), process, &h, 0, true, syscall.DUPLICATE_SAME_ACCESS); err != nil {
			closeHandles()
			return nil, err
		}
		handles = append(handles, h)
	}
	// Go's PROC_THREAD_ATTRIBUTE_HANDLE_LIST restricts inheritance to these handles and stdio.
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, AdditionalInheritedHandles: handles}
	cmd.Env = append(cmd.Env, fmt.Sprintf("DTW_CONTROL_IN_HANDLE=%d", handles[0]), fmt.Sprintf("DTW_CONTROL_OUT_HANDLE=%d", handles[1]))
	return closeHandles, nil
}
