// Package backend defines the private native boundary. Keys identify retained
// provider instances, never names or paths. Drivers must never reuse a key.
package backend

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
)

type Key string
type Node struct {
	Key, App, Window, Parent Key
	Object                   dw.Object
}
type Query struct {
	Roots           []Key
	Desktop         bool
	Depth, MaxNodes int
	Summary, Detail bool
	// Native traversal allowance; output pagination is independently budgeted.
	ReadTimeoutMS int64
}
type Page struct {
	Nodes       []Node
	Complete    bool
	Visited     int
	Unavailable []string
	Seat        Seat
}
type Seat struct {
	Foreground, Focused Key
	Application         Key
	// Fresh native facts for references newly introduced by this seat sample.
	// The engine registers them, but normal query/scope filtering still governs output.
	Nodes                []Node
	Pointer              dw.Fact[dw.Point]
	Health, Intervention string
}
type Operation struct {
	Step      dw.Step
	Key       Key
	Point, To *dw.Point
}
type Outcome struct {
	Delivery            dw.Delivery
	Accepted, Requested *int
	Fault               *dw.Fault
	Unsafe              bool
}
type Image struct {
	Bytes         []byte
	ContentType   string
	Bounds        dw.Bounds
	Width, Height int
}
type Text struct {
	Value  dw.Fact[string]
	Source string
}
type Driver interface {
	Open(context.Context) error
	Environment(context.Context) (dw.Environment, error)
	Permissions(context.Context, dw.PermissionRequest) ([]dw.Permission, error)
	Query(context.Context, Query) (Page, error)
	Read(context.Context, Key) (Node, Seat, error)
	ReadText(context.Context, Key) (Text, error)
	Perform(context.Context, Operation) Outcome
	HitTest(context.Context, dw.Point, Key) (bool, error)
	Capture(context.Context, dw.CaptureRequest) ([]Image, error)
	Close(context.Context) error
}
