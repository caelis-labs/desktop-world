// Package backend defines the private native boundary. Keys identify retained
// provider instances, never names or paths. Drivers must never reuse a key.
package backend

import (
	"context"
	dw "github.com/caelis-labs/desktop-world/internal/world"
)

type Key string
type Node struct {
	// Fields nil means a full read. Nonempty masks certify only those properties.
	Fields                   []string
	Key, App, Window, Parent Key
	Object                   dw.Object
}
type Query struct {
	Fields  []string
	Roots   []Key
	Desktop bool
	// AppName narrows a desktop application locator before AX traversal.
	// Empty leaves ordinary desktop discovery unchanged.
	AppName         string
	Depth, MaxNodes int
	Summary, Detail bool
	CaptureWindows  bool
	// Resume is a private, helper-local native traversal cursor.
	Resume string
	// NoContinuation makes an internal one-shot query release unfinished native
	// traversal state on its owning worker rather than retain an unreachable cursor.
	NoContinuation bool
	// Native traversal allowance; output pagination is independently budgeted.
	ReadTimeoutMS int64
}
type Page struct {
	Nodes       []Node
	Complete    bool
	Dirty       bool
	Visited     int
	ScanCursor  string
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
	ToKey     Key // Observed drag destination; never exposed to the agent.
	Point, To *dw.Point
}
type Outcome struct {
	Delivery            dw.Delivery
	Accepted, Requested *int
	Fault               *dw.Fault
	Unsafe              bool
	// Channel is the route actually used by a driver that selects delivery per
	// action. Empty keeps the plan's pre-dispatch channel.
	Channel string
}

// CaptureRequest carries the resolved native key only inside the trusted helper.
type CaptureRequest struct {
	dw.CaptureRequest
	Key           Key
	ReadTimeoutMS int64
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
	Capture(context.Context, CaptureRequest) ([]Image, error)
	Close(context.Context) error
}

// TargetedInput identifies explicitly host-configured window-directed input.
// It changes delivery guards and receipt channels together; shared-mode drivers
// retain the physical foreground/hit-test contract. Host policy/authorization,
// stable native keys, request reconciliation and fencing still apply.
type TargetedInput interface {
	TargetsInput(operation string) bool
}

// InputTransaction releases any borrowed foreground on the native worker after
// the entire plan, including failure/cancellation. It must be idempotent.
type InputTransaction interface {
	InputChannel() string
	BeginInput(context.Context) error
	EndInput(context.Context) (dw.InputReport, error)
}
