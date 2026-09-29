// Package protocol exposes a strict, transport-neutral agent protocol. UI text
// is untrusted data; it must never be promoted to host or system instructions.
package protocol

import (
	"context"
	"encoding/json"
	"errors"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/wire"
)

const Version = wire.Version

type Request struct {
	Protocol string
	World    dw.Epoch
	Op       string
	Args     json.RawMessage
}
type Response struct {
	Protocol string
	World    dw.Epoch
	Result   any
	Error    *dw.Fault
}

func Marshal(v any) ([]byte, error)     { return wire.Marshal(v) }
func Decode(data []byte, out any) error { return wire.Unmarshal(data, out) }

// Handler is bound by the trusted host to one actor and one epoch. Agent
// requests cannot change actor identity or invoke OS permission prompts.
type Handler struct {
	Actor dw.Actor
	Epoch dw.Epoch
}

func (h Handler) Handle(ctx context.Context, data []byte) ([]byte, error) {
	var req Request
	if e := Decode(data, &req); e != nil {
		return nil, e
	}
	if req.Protocol != Version {
		return nil, dw.Invalid("unsupported protocol version")
	}
	if h.Actor == nil {
		return nil, dw.Invalid("handler actor is required")
	}
	if req.World != h.Epoch {
		return nil, dw.NewFault("epoch_mismatch", "wrong world epoch", "reobserve")
	}
	var result any
	var err error
	switch req.Op {
	case "world.observe":
		var r dw.ObserveRequest
		if err = Decode(req.Args, &r); err == nil {
			result, err = h.Actor.Observe(ctx, r)
		}
	case "world.read":
		var r dw.TextRequest
		if err = Decode(req.Args, &r); err == nil {
			result, err = h.Actor.ReadText(ctx, r)
		}
	case "world.sync":
		var r dw.ChangeRequest
		if err = Decode(req.Args, &r); err == nil {
			result, err = h.Actor.Changes(ctx, r)
		}
	case "world.act":
		var p dw.Plan
		if err = Decode(req.Args, &p); err == nil {
			result, err = h.Actor.Execute(ctx, p)
		}
	case "world.capture":
		var r dw.CaptureRequest
		if err = Decode(req.Args, &r); err == nil {
			result, err = h.Actor.Capture(ctx, r)
		}
	case "world.run.get", "world.run.cancel":
		var r struct{ RunID dw.RunID }
		if err = Decode(req.Args, &r); err == nil {
			if req.Op == "world.run.get" {
				result, err = h.Actor.GetReceipt(ctx, r.RunID)
			} else {
				result, err = h.Actor.Cancel(ctx, r.RunID)
			}
		}
	default:
		return nil, dw.Invalid("unknown operation")
	}
	var f *dw.Fault
	if err != nil && !errors.As(err, &f) {
		f = dw.NewFault("request_failed", "request failed", "reobserve")
	}
	return Marshal(Response{Protocol: Version, World: h.Epoch, Result: result, Error: f})
}

// Operations is the negotiated v0.1 allowlist. Schemas do not include actor IDs,
// native handles, arbitrary attribute access or executable expressions.
func Operations() []string {
	return []string{"world.observe", "world.read", "world.sync", "world.act", "world.capture", "world.run.get", "world.run.cancel"}
}
