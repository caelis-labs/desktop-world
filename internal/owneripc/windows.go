//go:build windows

package owneripc

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"github.com/Microsoft/go-winio"
	"golang.org/x/sys/windows"
	"net"
)

func Listen() (net.Listener, string, func(), error) {
	var id [16]byte
	if _, e := rand.Read(id[:]); e != nil {
		return nil, "", nil, e
	}
	user, e := windows.GetCurrentProcessToken().GetTokenUser()
	if e != nil {
		return nil, "", nil, e
	}
	address := `\\.\pipe\desktop-world-owner-` + hex.EncodeToString(id[:])
	l, e := winio.ListenPipe(address, &winio.PipeConfig{SecurityDescriptor: "D:P(A;;GA;;;" + user.User.Sid.String() + ")", InputBufferSize: 65536, OutputBufferSize: 65536})
	if e != nil {
		return nil, "", nil, e
	}
	return l, address, func() { l.Close() }, nil
}
func Dial(ctx context.Context, address string) (net.Conn, error) {
	return winio.DialPipeContext(ctx, address)
}
