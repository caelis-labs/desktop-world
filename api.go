// Package desktopworld defines a sparse, scoped desktop object world.
// Use local.Open for a native world or dwtest.New for a deterministic fixture.
package desktopworld

import (
	"context"
	"time"
)

type Epoch string
type Ref string
type ActorID string
type FrameID string
type Cursor string
type RunID string
type RequestID string
type AssetID string
type Revision uint64
type Version uint64

type FactStatus string

const (
	FactUnrequested FactStatus = "" // Zero value: omitted from projected wire output.
	FactUnknown     FactStatus = "unknown"
	FactKnown       FactStatus = "known"
	FactUnsupported FactStatus = "unsupported"
	FactRedacted    FactStatus = "redacted"
)

// Value must be non-nil exactly when Status == FactKnown.
type Fact[T any] struct {
	Status    FactStatus
	Value     *T
	Source    string
	SampledAt time.Time
}

type Kind string

const (
	KindApplication Kind = "application"
	KindWindow      Kind = "window"
	KindUI          Kind = "ui"
)

type Lifecycle string

const (
	LifeLive        Lifecycle = "live"
	LifeStale       Lifecycle = "stale"
	LifeUnavailable Lifecycle = "unavailable"
	LifeGone        Lifecycle = "gone"
	LifeExpired     Lifecycle = "expired"
)

type Capability struct {
	Name         string
	Support      string // supported | unsupported | unknown
	Availability string // available | blocked | unknown
	Reason       string
	NativeName   string // Diagnostic only; never an arbitrary native-call entry point.
}

type Relation struct {
	Name   string
	Target Ref
}
type Rect struct{ X, Y, Width, Height float64 }
type Point struct {
	Frame      FrameID
	Topology   Version
	X, Y       float64
	ObservedAt time.Time
}
type Bounds struct {
	Frame    FrameID
	Topology Version
	Rect     Rect
}
type Object struct {
	Ref                    Ref
	Kind                   Kind
	Role                   string
	App                    Ref
	Window                 Ref // Empty for some application-owned menus/popups.
	Parent                 Ref
	Relations              []Relation
	Name                   Fact[string]
	ValuePreview           Fact[string]
	URI                    Fact[string] // Native document/link URL when exposed; not inferred from title.
	States                 map[string]Fact[bool]
	Bounds                 Fact[Bounds]
	Capabilities           []Capability
	Lifecycle              Lifecycle
	Version                Version
	GeometryVersion        Version
	SampleStart, SampleEnd time.Time
}

type Display struct {
	Ref             string
	Frame           FrameID
	Bounds          Rect
	Unit            string // quartz_point | physical_pixel; NOT a universal desktop DIP.
	ScaleX, ScaleY  float64
	RotationDegrees int
}
type SeatState struct {
	ForegroundApplication Fact[Ref]
	Pointer               Fact[Point]
	ForegroundWindow      Fact[Ref]
	FocusedObject         Fact[Ref]
	Health                string // ready | unavailable | fenced
	InterventionDetection string // supported | best_effort | unavailable
}
type Environment struct {
	Epoch        Epoch
	Platform     string
	Topology     Version
	Displays     []Display
	Permissions  []Permission
	Capabilities []Capability
}

type Scope struct {
	Desktop bool
	Refs    []Ref
}
type Projection string

const (
	ProjectionSummary Projection = "summary"
	ProjectionOutline Projection = "outline"
	ProjectionDetail  Projection = "detail"
)

type Budget struct {
	MaxResults, MaxDepth, MaxVisitedNodes int
	MaxOutputBytes, MaxTextRunes          int
	ReadDeadline                          time.Duration
}
type Freshness struct {
	Mode   string // cached | max_age | refresh
	MaxAge time.Duration
}

// Locator describes discovery, not the identity of a previously seen object.
// v0.1 equality is exact; NameContains is explicit and cannot prove identity.
type Locator struct {
	Within             Ref
	Kind               Kind
	Role               string
	NameEquals         *string
	NameContains       *string
	RequiredStates     map[string]bool
	RequiredCapability string
	MaxDepth           int
}
type ObserveRequest struct {
	Scope        Scope
	Projection   Projection
	Fields       []string
	Match        *Locator
	Freshness    Freshness
	Budget       Budget
	Continuation string
}
type Coverage struct {
	Scope                  Scope
	Fields                 []string
	MaxDepth               int
	Complete               bool // Complete only for the declared scope/depth/filter/sample.
	VisitedNodes           int
	Truncated              bool
	Continuation           string
	Dirty                  bool
	UnavailableSources     []string
	SampleStart, SampleEnd time.Time
}
type Observation struct {
	Epoch       Epoch
	Revision    Revision
	Cursor      Cursor
	Objects     []Object
	Seat        SeatState
	Coverage    Coverage
	Environment *Environment
}
type Removed struct {
	Ref    Ref
	Reason string // destroyed | out_of_view | expired | backend_reset
}
type ChangeRequest struct {
	Cursor         Cursor
	MaxOutputBytes int
	Wait           time.Duration
}
type ChangeSet struct {
	From, To          Revision
	Cursor            Cursor
	ResetRequired     bool
	ResetReason       string
	Upserts           []Object // Full selected projection of each changed object.
	Removed           []Removed
	InvalidatedScopes []Scope
	Seat              *SeatState
	Coverage          Coverage
}
type WatchRequest struct {
	Cursor Cursor
	Buffer int
}
type Stream interface {
	Next(context.Context) (ChangeSet, error)
	Close() error
}

type TextRequest struct {
	Target Ref
	// Offsets are Unicode scalar-value offsets in the returned normalized text,
	// not UTF-16/native offsets. Continuation is bound to a text version.
	Offset, LimitRunes int
	Continuation       string
	Freshness          Freshness
}
type TextResult struct {
	Target      Ref
	Text        Fact[string]
	TextVersion Version
	Next        string
	Truncated   bool
	Source      string // value | text | label; no automatic OCR.
	Coverage    Coverage
}

type Anchor struct {
	Target Ref
	U, V   float64 // Relative bounds position in [0,1]; not a clickable-point claim.
}
type ResolvedAnchor struct {
	Anchor          Anchor
	Point           Point
	GeometryVersion Version
	Valid           bool
	Reason          string
}
type CaptureRequest struct {
	Kind                          string // visible_region | window_content (capability-dependent)
	Region                        *Bounds
	Target                        Ref
	MaxPixelWidth, MaxPixelHeight int
	IncludeCursor                 bool
}
type Transform2D struct{ A, B, C, D, TX, TY float64 }
type CaptureTile struct {
	Asset                   AssetID
	ImageFrame              FrameID
	DesktopFrame            FrameID
	PixelWidth, PixelHeight int
	ImageToDesktop          Transform2D
	Topology                Version
	CapturedAt              time.Time
	Kind                    string
}
type CaptureResult struct {
	Tiles           []CaptureTile
	RelatedRevision Revision // Correlation, NOT simultaneous pixel/AX snapshot.
	ExpiresAt       time.Time
	Partial         bool
	Exclusions      []string
}
type Asset struct {
	ContentType string
	Bytes       []byte
	ExpiresAt   time.Time
}

// Target is a tagged union. Exactly one arm is set. Allowed arms depend on Op.
// Locator is only valid on bind in v0.1. Bound aliases cannot rebind themselves.
type Target struct {
	Ref    Ref
	Bound  string
	Point  *Point
	Anchor *Anchor
}
type Predicate struct {
	Target        Target
	Property      string // Small documented allowlist, not arbitrary expressions.
	EqualsString  *string
	EqualsBool    *bool
	EqualsVersion *Version
}
type Bind struct {
	Name          string
	Locator       Locator
	RequireUnique bool // MUST be true for a v0.1 execution binding.
}
type SetValue struct{ Text string }
type TypeText struct{ Text string }
type KeyChord struct {
	Modifiers []string
	Key       string
}
type Click struct {
	Button string
	Count  int
}
type Drag struct {
	To       Target
	Duration time.Duration
}
type Scroll struct {
	DX, DY float64
	Unit   string // wheel_step in v0.1; positive right/down viewport direction.
}
type Step struct {
	ID         string
	Op         string // bind | wait | focus | invoke | set_value | pointer.* | keyboard.*
	Target     Target
	Bind       *Bind
	SetValue   *SetValue
	TypeText   *TypeText
	Press      *KeyChord
	Click      *Click
	Drag       *Drag
	Scroll     *Scroll
	Before     []Predicate
	After      []Predicate
	Completion string // dispatch | verify
	Timeout    time.Duration
}
type Plan struct {
	Epoch     Epoch
	RequestID RequestID // World-epoch-prefixed; deduplicated with canonical body.
	Timeout   time.Duration
	Steps     []Step // Bounded, linear, no branches/loops/implicit ref healing.
}

type Delivery string

const (
	DeliveryNA       Delivery = "not_applicable"
	DeliveryNone     Delivery = "none"
	DeliveryComplete Delivery = "complete"
	DeliveryPartial  Delivery = "partial"
	DeliveryUnknown  Delivery = "unknown"
)

type Verification string

const (
	VerifyNotRequested Verification = "not_requested"
	VerifyVerified     Verification = "verified"
	VerifyNotMet       Verification = "not_met"
	VerifyUnknown      Verification = "unknown"
)

type Fault struct {
	Code       string
	Message    string
	NativeCode string
	RetryClass string            // read_only | reobserve | never_automatically
	Details    map[string]string // Sanitized; no raw secret/provider payload.
}

func (f *Fault) Error() string { return f.Code + ": " + f.Message }

type StepResult struct {
	ID                         string
	Target                     Ref
	State                      string // skipped | satisfied | dispatched | failed | unknown
	Delivery                   Delivery
	AcceptedInputEvents        *int
	RequestedInputEvents       *int
	Verification               Verification
	Evidence                   []Predicate
	Fault                      *Fault
	StartRevision, EndRevision Revision
	StartedAt, FinishedAt      time.Time
}
type Receipt struct {
	RunID                      RunID
	RequestID                  RequestID
	Epoch                      Epoch
	State                      string // queued | running | terminal
	Outcome                    string // pending | completed | stopped | partial | unknown
	Steps                      []StepResult
	Bindings                   map[string]Ref
	StartRevision, EndRevision Revision
	Fault                      *Fault
	SeatHealth                 string
	ExpiresAt                  time.Time // Detail retention; request tombstone outlives details.
}

type Permission struct {
	Name   string
	State  string // granted | denied | not_requested | restricted | unknown
	Reason string
}
type PermissionRequest struct{ Names []string }
type Intent struct {
	Actor               ActorID
	Operation           string
	Targets             []Ref
	Scope               Scope
	HasSensitivePayload bool
	PlanDigest          string
	Step                *Step // Resolved operation arguments; sensitive, never log by default.
	Fields              []string
	CaptureKind         string
}
type Decision struct {
	Allow  bool
	Reason string
}

// Authorizer is supplied by trusted host code. UI text and remote tool arguments
// cannot replace it. It must be bounded/noninteractive; approval UI stays outside.
type Authorizer interface {
	Check(context.Context, Intent) (Decision, error)
}
type ActorConfig struct {
	ID          ActorID
	ReadScopes  []Scope
	WriteScopes []Scope
	Operations  []string
	Authorizer  Authorizer
}
type Progress struct {
	Run    RunID
	StepID string
	Target Ref
	Phase  string // resolving | ready | dispatching | verifying | stopped
	At     time.Time
}

type World interface {
	Environment(context.Context) (Environment, error)
	NewActor(context.Context, ActorConfig) (Actor, error)
	RequestPermissions(context.Context, PermissionRequest) ([]Permission, error)
	Close(context.Context) error
}
type Actor interface {
	ID() ActorID
	Observe(context.Context, ObserveRequest) (Observation, error)
	ReadText(context.Context, TextRequest) (TextResult, error)
	Changes(context.Context, ChangeRequest) (ChangeSet, error)
	Watch(context.Context, WatchRequest) (Stream, error)
	ResolveAnchor(context.Context, Anchor) (ResolvedAnchor, error)
	Capture(context.Context, CaptureRequest) (CaptureResult, error)
	ReadAsset(context.Context, AssetID) (Asset, error)
	Execute(context.Context, Plan) (Receipt, error)
	GetReceipt(context.Context, RunID) (Receipt, error)
	Cancel(context.Context, RunID) (Receipt, error)
	Progress(context.Context, RunID) (<-chan Progress, error)
	Close() error
}
