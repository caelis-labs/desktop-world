//go:build dtw_poc_exactgrant

package backend

// POCWindowIdentity is compiled only into the isolated exact-grant helper.
// Keys are native registry identities, not script-visible selectors.
type POCWindowIdentity struct {
	PID            int32
	StartSec       int64
	StartUSec      int64
	NativeWindowID uint32
	AppKey         Key
	WindowKey      Key
}
