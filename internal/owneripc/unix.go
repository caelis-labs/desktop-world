//go:build !windows

package owneripc

import (
	"context"
	"net"
	"os"
	"path/filepath"
)

func Listen() (net.Listener, string, func(), error) {
	dir, e := os.MkdirTemp("", "dtw-owner-")
	if e != nil {
		return nil, "", nil, e
	}
	address := filepath.Join(dir, "owner.sock")
	l, e := net.Listen("unix", address)
	if e != nil {
		os.RemoveAll(dir)
		return nil, "", nil, e
	}
	if e = os.Chmod(address, 0600); e != nil {
		l.Close()
		os.RemoveAll(dir)
		return nil, "", nil, e
	}
	return l, address, func() { l.Close(); os.RemoveAll(dir) }, nil
}
func Dial(ctx context.Context, address string) (net.Conn, error) {
	return (&net.Dialer{}).DialContext(ctx, "unix", address)
}
