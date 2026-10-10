// Package helper hosts the same SDK in a persistent, parent-owned stdio process.
// It is an experimental distribution adapter, not a second desktop engine.
package helper

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/ipc/protocol"
)

const Version = "desktop-world/helper-v0.1"

type Config struct {
	InputPolicy dw.InputPolicy
	InputMode   dw.InputMode
	Capture     bool
	AssetsDir   string
	Audit       io.Writer
	AuditPath   string
	FullOutput  bool
	Managed     bool // Session lifecycle arrives over the private owner pipe.
}

type Request struct {
	ID   string
	Op   string
	Args json.RawMessage
	Turn string // Trusted host metadata, outside agent args.
}

type Response struct {
	ID       string
	Protocol string
	World    dw.Epoch
	Result   any
	Error    *dw.Fault
}

type Server struct {
	world     dw.World
	actor     dw.Actor
	epoch     dw.Epoch
	config    Config
	mu        sync.Mutex
	turns     *turnLifecycle
	discovery dw.Actor
}

// New exposes the native desktop engine without a DTW application grant gate.
// The embedding application owns user authorization; OS permissions still apply.
func New(ctx context.Context, w dw.World, c Config) (*Server, error) {
	if err := c.InputMode.Validate(); err != nil {
		return nil, err
	}
	c.InputMode = c.InputMode.Effective()
	if err := c.InputPolicy.Validate(); err != nil {
		return nil, err
	}
	if c.InputPolicy == "" {
		c.InputPolicy = dw.InputShared
	}
	env, err := w.Environment(ctx)
	if err != nil {
		return nil, err
	}
	if env.InputMode.Effective() != c.InputMode {
		return nil, dw.Invalid("helper input mode does not match the opened world")
	}
	ops := []string{"observe", "read", "sync", "bind", "bind_focus", "wait", "resolve_anchor"}
	scopes := []dw.Scope{{Desktop: true}}
	if len(scopes) > 0 {
		ops = append(ops, "focus", "invoke", "set_value", "set_expanded", "set_checked", "set_selected", "scroll_into_view", "pointer.move", "pointer.click", "pointer.drag", "pointer.scroll", "keyboard.type_text", "keyboard.press")
	}
	if c.Capture {
		ops = append(ops, "capture", "read_asset")
		if c.AssetsDir == "" {
			return nil, dw.Invalid("capture requires a host-selected assets directory")
		}
		if err := os.MkdirAll(c.AssetsDir, 0700); err != nil {
			return nil, err
		}
		c.AssetsDir, err = filepath.Abs(c.AssetsDir)
		if err != nil {
			return nil, err
		}
	}
	turns := &turnLifecycle{used: map[string]bool{}}
	if !c.Managed {
		if err := turns.begin("session"); err != nil {
			return nil, err
		}
	}
	a, err := w.NewActor(ctx, dw.ActorConfig{InputPolicy: c.InputPolicy, ID: "helper-agent", ReadScopes: []dw.Scope{{Desktop: true}}, WriteScopes: scopes, Operations: ops})
	if err != nil {
		return nil, err
	}
	discovery, err := w.NewActor(ctx, dw.ActorConfig{ID: "helper-discovery", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if err != nil {
		a.Close()
		return nil, err
	}
	return &Server{world: w, actor: a, epoch: env.Epoch, config: c, turns: turns, discovery: discovery}, nil
}

func (s *Server) Handle(ctx context.Context, r Request) Response {
	out := Response{ID: r.ID, Protocol: Version, World: s.epoch}
	if r.Op != "get" && r.Op != "cancel" {
		var release func()
		var err error
		turn := r.Turn
		if !s.config.Managed {
			if turn != "" {
				out.Error = dw.Invalid("turn requires managed mode")
				return out
			}
			turn = "session"
		}
		ctx, release, err = s.turns.bind(ctx, turn)
		if err != nil {
			out.Error = asFault(err)
			return out
		}
		defer release()
	}
	if r.ID == "" || len(r.ID) > 128 || strings.ContainsAny(r.ID, "\r\n") {
		out.Error = dw.Invalid("id requires 1..128 characters")
		return out
	}
	if len(r.Args) == 0 {
		r.Args = json.RawMessage(`{}`)
	}
	op := operation(r.Op)
	if op == "" {
		out.Error = dw.Invalid("unknown operation; use schema for supported verbs")
		return out
	}
	if r.Op == "observe" || r.Op == "sync" {
		// Core budgets include its envelope; reserve the helper's additional ID
		// and protocol bytes so the public budget still describes the whole reply.
		h, _ := protocol.Marshal(Response{ID: r.ID, Protocol: Version, World: s.epoch, Result: json.RawMessage(`{}`)})
		p, _ := protocol.Marshal(protocol.Response{Protocol: protocol.Version, World: s.epoch, Result: json.RawMessage(`{}`)})
		delta := len(h) - len(p)
		adjust := func(n int) (int, error) {
			if n == 0 {
				n = 16384
			}
			if n <= delta {
				return 0, dw.Invalid("output budget cannot fit helper envelope")
			}
			return n - delta, nil
		}
		if r.Op == "observe" {
			var q dw.ObserveRequest
			if err := protocol.Decode(r.Args, &q); err != nil {
				out.Error = asFault(err)
				return out
			}
			var err error
			q.Budget.MaxOutputBytes, err = adjust(q.Budget.MaxOutputBytes)
			if err != nil {
				out.Error = asFault(err)
				return out
			}
			r.Args, _ = protocol.Marshal(q)
		} else {
			var q dw.ChangeRequest
			if err := protocol.Decode(r.Args, &q); err != nil {
				out.Error = asFault(err)
				return out
			}
			var err error
			q.MaxOutputBytes, err = adjust(q.MaxOutputBytes)
			if err != nil {
				out.Error = asFault(err)
				return out
			}
			r.Args, _ = protocol.Marshal(q)
		}
	}
	if r.Op == "act" {
		var p dw.Plan
		if err := protocol.Decode(r.Args, &p); err != nil {
			out.Error = asFault(err)
			return out
		}
		if p.Epoch != "" || p.RequestID != "" {
			out.Error = dw.Invalid("helper owns epoch/request_id; use a stable envelope id")
			return out
		}
		p.Epoch = s.epoch
		p.RequestID = dw.RequestID(string(s.epoch) + ":" + r.ID)
		if s.config.Managed {
			p.RequestID = dw.RequestID(string(s.epoch) + ":" + r.Turn + ":" + r.ID)
		}
		r.Args, _ = protocol.Marshal(p)
	}
	b, err := protocol.Marshal(protocol.Request{Protocol: protocol.Version, World: s.epoch, Op: op, Args: r.Args})
	if err != nil {
		out.Error = asFault(err)
		return out
	}
	data, err := (protocol.Handler{Actor: s.actor, Epoch: s.epoch}).Handle(ctx, b)
	if err != nil {
		out.Error = asFault(err)
		return out
	}
	// Preserve arbitrary result JSON without reinterpreting Fact/revision values.
	var reply struct {
		Result json.RawMessage `json:"result"`
		Error  *dw.Fault       `json:"error"`
	}
	var raw map[string]json.RawMessage
	if err = json.Unmarshal(data, &raw); err != nil {
		out.Error = asFault(err)
		return out
	}
	reply.Result = raw["result"]
	if len(raw["error"]) > 0 {
		if err = protocol.Decode(raw["error"], &reply.Error); err != nil {
			out.Error = asFault(err)
			return out
		}
	}
	out.Result = reply.Result
	out.Error = reply.Error
	if r.Op == "capture" && out.Error == nil {
		var captured dw.CaptureResult
		if err = protocol.Decode(reply.Result, &captured); err != nil {
			out.Error = asFault(err)
			return out
		}
		type saved struct {
			Asset dw.AssetID
			Path  string
			Bytes int
		}
		files := []saved{}
		for _, tile := range captured.Tiles {
			asset, err := s.actor.ReadAsset(ctx, tile.Asset)
			if err != nil {
				out.Error = asFault(err)
				return out
			}
			// The model never supplies an output path. CreateTemp supplies a fresh,
			// private filename, preventing collisions and path traversal.
			f, err := os.CreateTemp(s.config.AssetsDir, "capture-*.png")
			if err != nil {
				out.Error = asFault(err)
				return out
			}
			_, writeErr := f.Write(asset.Bytes)
			closeErr := f.Close()
			if writeErr != nil {
				out.Error = asFault(writeErr)
				return out
			}
			if closeErr != nil {
				out.Error = asFault(closeErr)
				return out
			}
			files = append(files, saved{tile.Asset, f.Name(), len(asset.Bytes)})
		}
		out.Result = struct {
			Capture dw.CaptureResult
			Files   []saved
		}{captured, files}
	}
	return out
}

func operation(op string) string {
	switch op {
	case "observe", "read", "sync", "act", "capture":
		return "world." + op
	case "get":
		return "world.run.get"
	case "cancel":
		return "world.run.cancel"
	}
	return ""
}

func Schema(op string) map[string]any {
	args := protocol.ArgumentsSchema(operation(op))
	if args == nil {
		return nil
	}
	if op == "act" {
		p := args["properties"].(map[string]any)
		delete(p, "epoch")
		delete(p, "request_id")
		args["required"] = []string{"steps"}
	}
	return map[string]any{"type": "object", "additionalProperties": false, "required": []string{"id", "op", "args"}, "properties": map[string]any{"id": map[string]any{"type": "string", "minLength": 1, "maxLength": 128, "description": "Caller-generated stable ID. Retry an uncertain request with the SAME id and body. A new id can repeat effects."}, "op": map[string]any{"const": op}, "args": args}}
}

func Schemas() map[string]any {
	out := map[string]any{}
	for _, op := range []string{"observe", "read", "sync", "act", "capture", "get", "cancel"} {
		out[op] = Schema(op)
	}
	return out
}

func asFault(err error) *dw.Fault {
	var f *dw.Fault
	if errors.As(err, &f) {
		return f
	}
	return dw.NewFault("helper_failed", err.Error(), "reobserve")
}

// Serve allows a bounded number of in-flight requests so receipt lookup/cancel
// is not trapped behind a pending act. Output frames and audit rows are atomic.
// EOF cancels in-flight work; it is not proof that no effect reached the OS.
func (s *Server) Serve(ctx context.Context, in io.Reader, out io.Writer) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	defer s.actor.Close()
	defer s.discovery.Close()
	defer s.turns.stop()
	if closer, ok := in.(io.ReadCloser); ok {
		go func() { <-ctx.Done(); _ = closer.Close() }()
	}
	env, err := s.world.Environment(ctx)
	if err != nil {
		return err
	}
	write := func(v any) error {
		s.mu.Lock()
		defer s.mu.Unlock()
		b, e := protocol.Marshal(v)
		if e != nil {
			return e
		}
		_, e = fmt.Fprintln(out, string(b))
		return e
	}
	if s.config.Audit != nil {
		if e := json.NewEncoder(s.config.Audit).Encode(map[string]any{"event": "session_start", "epoch": s.epoch, "at": time.Now().UTC(), "input_mode": s.config.InputMode, "input_policy": s.config.InputPolicy}); e != nil {
			return e
		}
	}
	if err = write(struct {
		Type, Protocol   string
		Environment      dw.Environment
		Capture, Managed bool
		CoreNoAuth       bool `json:"core_no_auth"`
		InputPolicy      dw.InputPolicy
		InputMode        dw.InputMode
		AuditPath        string
		Instructions     string
	}{"hello", Version, env, s.config.Capture, s.config.Managed, true, s.config.InputPolicy, s.config.InputMode, s.config.AuditPath, "Private native desktop engine. One JSON request per line; original receipt IDs remain authoritative. Application authorization belongs to the embedding host."}); err != nil {
		return err
	}
	var wg sync.WaitGroup
	defer func() { cancel(); wg.Wait() }()
	// Reserve control capacity so queued reads/writes cannot starve cancellation.
	dataPermits := make(chan struct{}, 2)
	controlPermits := make(chan struct{}, 2)
	scanner := bufio.NewScanner(in)
	scanner.Buffer(make([]byte, 4096), (1<<20)+1)
	for scanner.Scan() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		data := append([]byte(nil), scanner.Bytes()...)
		var r Request
		if err = protocol.Decode(data, &r); err != nil {
			if e := write(Response{Protocol: Version, World: s.epoch, Error: asFault(err)}); e != nil {
				return e
			}
			continue
		}
		permits := dataPermits
		if r.Op == "get" || r.Op == "cancel" {
			permits = controlPermits
		}
		select {
		case permits <- struct{}{}:
		default:
			if e := write(Response{ID: r.ID, Protocol: Version, World: s.epoch, Error: dw.NewFault("helper_busy", "at most four in-flight calls", "read_only")}); e != nil {
				return e
			}
			continue
		}
		wg.Add(1)
		go func(r Request, inputBytes int) {
			defer wg.Done()
			defer func() { <-permits }()
			start := time.Now()
			callCtx, stop := context.WithTimeout(ctx, 12*time.Second)
			defer stop()
			response := s.Handle(callCtx, r)
			encoded, e := protocol.Marshal(response)
			if e != nil {
				cancel()
				return
			}
			fullBytes := len(encoded)
			if !s.config.FullOutput {
				encoded, e = protocol.CompactJSON(encoded)
				if e != nil {
					cancel()
					return
				}
			}
			s.mu.Lock()
			defer s.mu.Unlock()
			if _, e = fmt.Fprintln(out, string(encoded)); e != nil {
				cancel()
			}
			if s.config.Audit != nil {
				row := map[string]any{"at": start.UTC(), "elapsed_ms": float64(time.Since(start).Microseconds()) / 1000, "id": r.ID, "op": r.Op, "request_bytes": inputBytes, "response_bytes": len(encoded), "full_response_bytes": fullBytes}
				if response.Error != nil {
					row["error_code"] = response.Error.Code
				}
				if r.Op == "act" {
					var receipt dw.Receipt
					if b, ok := response.Result.(json.RawMessage); ok && protocol.Decode(b, &receipt) == nil {
						row["outcome"] = receipt.Outcome
						row["steps"] = len(receipt.Steps)
						deliveries := map[string]int{}
						for _, step := range receipt.Steps {
							deliveries[string(step.Delivery)]++
						}
						row["deliveries"] = deliveries
						row["run_id"] = receipt.RunID
					}
				}
				if r.Op == "observe" {
					var ob dw.Observation
					if b, ok := response.Result.(json.RawMessage); ok && protocol.Decode(b, &ob) == nil {
						row["objects"] = len(ob.Objects)
						row["visited_nodes"] = ob.Coverage.VisitedNodes
						row["complete"] = ob.Coverage.Complete
						row["truncated"] = ob.Coverage.Truncated
					}
				}
				if e := json.NewEncoder(s.config.Audit).Encode(row); e != nil {
					cancel()
				}
			}
		}(r, len(data))
	}
	return scanner.Err()
}

// SchemaIndex keeps the discovery entry point smaller than the parameter schemas.
func SchemaIndex() map[string]any {
	return map[string]any{"protocol": Version, "operations": []string{"observe", "read", "sync", "act", "capture", "get", "cancel"}, "next": "dtw schema OP; dtw schema act ACTION"}
}

// ActionSchema discloses only the selected action, retaining the authoritative schema.
func ActionSchema(op string) map[string]any {
	if !dw.IsWrite(op) && op != "bind" && op != "bind_focus" && op != "wait" {
		return nil
	}
	out := Schema("act")
	args := out["properties"].(map[string]any)["args"].(map[string]any)
	item := args["properties"].(map[string]any)["steps"].(map[string]any)["items"].(map[string]any)
	properties := item["properties"].(map[string]any)
	arm := map[string]string{"bind": "bind", "bind_focus": "bind_focus", "set_value": "set_value", "set_expanded": "set_expanded", "set_checked": "set_checked", "set_selected": "set_selected", "keyboard.type_text": "type_text", "keyboard.press": "press", "pointer.click": "click", "pointer.drag": "drag", "pointer.scroll": "scroll"}[op]
	for _, key := range []string{"bind", "bind_focus", "set_value", "set_expanded", "set_checked", "set_selected", "type_text", "press", "click", "drag", "scroll"} {
		if key != arm {
			delete(properties, key)
		}
	}
	properties["op"].(map[string]any)["enum"] = []string{op}
	if op == "bind" || op == "wait" {
		delete(properties, "target")
	}
	var variants []any
	for _, raw := range item["oneOf"].([]any) {
		variant := raw.(map[string]any)
		if variant["properties"].(map[string]any)["op"].(map[string]any)["const"] == op {
			delete(variant, "not")
			variants = append(variants, variant)
		}
	}
	item["oneOf"] = variants
	item["description"] = "Only the selected action is disclosed. Preserve request ID/body on uncertainty; unsupported semantic actions never fall back to physical input."
	return out
}
