// POC only: official Go MCP SDK with a per-connection QuickJS subprocess.
// The parent owns the native helper, receipts, and cross-process seat locks.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	quickjs "github.com/buke/quickjs-go"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

type input struct {
	Operation    string `json:"operation" jsonschema:"exec, status, result or cancel"`
	ExecutionID  string `json:"execution_id" jsonschema:"stable execution identifier"`
	Code         string `json:"code,omitempty" jsonschema:"approved JavaScript body for exec"`
	Detail       string `json:"detail,omitempty" jsonschema:"optional full original result detail"`
	IncludeImage bool   `json:"include_image,omitempty" jsonschema:"include requested PNG on result query"`
}

type record struct {
	ID             string
	Hash           string
	State          string
	Output         []string
	Error          string
	Cancel         context.CancelFunc
	Done           chan struct{}
	RunCtx         context.Context
	NativeIDs      []string
	NativeReceipts map[string]host.Reply
	NativeError    *dw.Fault
	Observations   []map[string]any
	Actions        []map[string]any
	Captures       []map[string]any
}

type scriptCommand struct {
	Ctx    context.Context
	Code   string
	Record *record
}

type supervisor struct {
	mu           sync.Mutex
	records      map[string]*record
	active       *record
	commands     chan scriptCommand
	native       *host.Client
	nativeMu     sync.Mutex
	assetsDir    string
	remoteNative func(context.Context, string, string, json.RawMessage) (host.Reply, error)
	// POC-only injection point for end-to-end receipt fault tests. Production
	// server construction leaves this nil and always uses the real native host.
	testNative func(context.Context, string, string, json.RawMessage) (host.Reply, error)
	onDone     func(*record)
	child      *scriptChild
	childDead  bool
	refApps    map[string]string
	refParents map[string]string
}

func newSupervisor(native *host.Client, assetsDir string) *supervisor {
	s := &supervisor{records: make(map[string]*record), commands: make(chan scriptCommand), native: native, assetsDir: assetsDir, refApps: make(map[string]string), refParents: make(map[string]string)}
	s.startScriptChild()
	return s
}

func (s *supervisor) imageFor(id string) ([]byte, error) {
	s.mu.Lock()
	r := s.records[id]
	if r == nil {
		s.mu.Unlock()
		return nil, errors.New("execution not found")
	}
	var bodies []json.RawMessage
	for _, nativeID := range r.NativeIDs {
		if reply, ok := r.NativeReceipts[nativeID]; ok && reply.Error == nil {
			bodies = append(bodies, append(json.RawMessage(nil), reply.Result...))
		}
	}
	s.mu.Unlock()
	for _, body := range bodies {
		var captured struct {
			Files []struct {
				Path  string `json:"path"`
				Bytes int    `json:"bytes"`
			} `json:"files"`
		}
		if json.Unmarshal(body, &captured) != nil {
			continue
		}
		for _, file := range captured.Files {
			if file.Bytes <= 0 || file.Bytes > 4<<20 || s.assetsDir == "" {
				continue
			}
			base, err := filepath.Abs(s.assetsDir)
			if err != nil {
				continue
			}
			path, err := filepath.Abs(file.Path)
			if err != nil {
				continue
			}
			rel, err := filepath.Rel(base, path)
			if err != nil || rel == ".." || len(rel) >= 3 && rel[:3] == "../" {
				continue
			}
			data, err := os.ReadFile(path)
			if err != nil {
				return nil, err
			}
			if len(data) != file.Bytes || len(data) < 8 || string(data[:8]) != "\x89PNG\r\n\x1a\n" {
				return nil, errors.New("capture is not a verified PNG")
			}
			return data, nil
		}
	}
	return nil, errors.New("no requested capture image under this execution")
}

func (s *supervisor) scriptLoop() {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	// Bound script-owned JS memory and stack inside the disposable child. The
	// parent keeps native receipts and MCP control alive if the child runs out.
	rt := quickjs.NewRuntime(quickjs.WithMemoryLimit(64<<20), quickjs.WithGCThreshold(8<<20), quickjs.WithMaxStackSize(1<<20))
	defer rt.Close()
	// Bare context retains ECMAScript Promise but does not register QuickJS
	// std/os modules or timers. The approved script surface is supplied below.
	ctx := rt.NewBareContext()
	defer ctx.Close()
	ctx.Globals().Set("state", ctx.NewObject())
	var running context.Context = context.Background()
	rt.SetInterruptHandler(func() int {
		if running.Err() != nil {
			return 1
		}
		return 0
	})
	for command := range s.commands {
		running = command.Ctx
		outputs := []string{}
		printBytes := 0
		printFn := ctx.NewFunction(func(_ *quickjs.Context, _ *quickjs.Value, args []*quickjs.Value) *quickjs.Value {
			for _, arg := range args {
				value := arg.ToString()
				if len(outputs) >= 32 || printBytes+len(value) > 8192 {
					return ctx.ThrowError(errors.New("print limit exceeded; query original execution details"))
				}
				outputs = append(outputs, value)
				printBytes += len(value)
			}
			return ctx.Undefined()
		})
		ctx.Globals().Set("print", printFn)
		// Go's timer resolves a JS Promise on the context's owner thread.
		sleepFn := ctx.NewFunction(func(ctx *quickjs.Context, _ *quickjs.Value, args []*quickjs.Value) *quickjs.Value {
			ms := int64(1)
			if len(args) > 0 {
				ms = int64(args[0].Int32())
			}
			if ms < 0 {
				ms = 0
			}
			if ms > 10000 {
				ms = 10000
			}
			return ctx.NewPromise(func(resolve, _ func(*quickjs.Value)) {
				go func() {
					select {
					case <-time.After(time.Duration(ms) * time.Millisecond):
					case <-command.Ctx.Done():
					}
					ctx.Schedule(func(inner *quickjs.Context) {
						v := inner.NewString("done")
						resolve(v)
						v.Free()
					})
				}()
			})
		})
		global := ctx.NewObject()
		global.Set("sleep", sleepFn)
		nativeFn := ctx.NewFunction(func(ctx *quickjs.Context, _ *quickjs.Value, args []*quickjs.Value) *quickjs.Value {
			if len(args) != 2 {
				return ctx.ThrowTypeError("native call needs operation and arguments")
			}
			op := args[0].ToString()
			if op != "observe" && op != "read" && op != "sync" && op != "act" && op != "capture" && op != "get" && op != "cancel" && op != "grants" && op != "revoke_grant" {
				return ctx.ThrowTypeError("unknown desktop operation")
			}
			body := json.RawMessage(args[1].ToString())
			if len(body) > 65536 || !json.Valid(body) {
				return ctx.ThrowTypeError("native arguments are invalid or too large")
			}
			if s.native == nil && s.remoteNative == nil {
				return ctx.ThrowError(errors.New("native helper is not configured in this POC"))
			}
			s.mu.Lock()
			if len(command.Record.NativeIDs) >= 32 {
				s.mu.Unlock()
				return ctx.ThrowError(errors.New("native call limit exceeded"))
			}
			id := fmt.Sprintf("%s-native-%d", command.Record.ID, len(command.Record.NativeIDs)+1)
			command.Record.NativeIDs = append(command.Record.NativeIDs, id)
			s.mu.Unlock()
			return ctx.NewPromise(func(resolve, _ func(*quickjs.Value)) {
				go func() {
					var reply host.Reply
					var callErr error
					if s.remoteNative != nil {
						reply, callErr = s.remoteNative(command.Ctx, id, op, body)
					} else {
						s.nativeMu.Lock()
						reply, callErr = s.native.Call(command.Ctx, "session", id, op, body)
						s.nativeMu.Unlock()
					}
					if callErr != nil {
						// The request may already have reached the native provider.
						reply = host.Reply{ID: id}
						reply.Error = dw.NewFault("native_unknown", callErr.Error(), "never_automatically")
					}
					s.mu.Lock()
					if command.Record.NativeReceipts == nil {
						command.Record.NativeReceipts = make(map[string]host.Reply)
					}
					command.Record.NativeReceipts[id] = reply
					if reply.Error != nil {
						command.Record.NativeError = reply.Error
					}
					if reply.Error == nil {
						facts := nativeFacts(op, id, reply.Result)
						switch op {
						case "observe":
							if facts != nil {
								command.Record.Observations = append(command.Record.Observations, facts)
							}
						case "act":
							if facts != nil {
								command.Record.Actions = append(command.Record.Actions, facts)
							}
						case "capture":
							if facts != nil {
								command.Record.Captures = append(command.Record.Captures, facts)
							}
						}
					}
					s.mu.Unlock()
					data, _ := protocol.Marshal(reply)
					ctx.Schedule(func(inner *quickjs.Context) {
						v := inner.NewString(string(data))
						resolve(v)
						v.Free()
					})
				}()
			})
		})
		global.Set("native", nativeFn)
		if os.Getenv("DTW_POC_TEST_BLOCK") == "1" {
			global.Set("_testBlock", ctx.NewFunction(func(ctx *quickjs.Context, _ *quickjs.Value, _ []*quickjs.Value) *quickjs.Value {
				time.Sleep(5 * time.Second)
				return ctx.Undefined()
			}))
		}
		ctx.Globals().Set("dtw", global)
		wrapper := ctx.Eval(`dtw.call = async (op, args={}) => { const reply = JSON.parse(await dtw.native(op, JSON.stringify(args))); if (reply.error) { const e = new Error(reply.error.message); e.code = reply.error.code; throw e; } return reply.result; }; for (const op of ['observe','read','sync','act','capture','get','cancel','grants']) dtw[op] = args => dtw.call(op,args); dtw.revokeGrant = args => dtw.call('revoke_grant',args);`)
		if wrapper != nil {
			wrapper.Free()
		}
		disclosure := ctx.Eval(disclosureJS)
		if disclosure != nil {
			disclosure.Free()
		}
		world := ctx.Eval(worldJS)
		if world != nil {
			world.Free()
		}
		wrapped := "(async () => {\n" + command.Code + "\n})()"
		value := ctx.Eval(wrapped)
		var failure string
		if value == nil {
			failure = "JavaScript runtime returned no value"
		} else {
			if !value.IsException() {
				settled := ctx.Await(value)
				if settled != value {
					value.Free()
					value = settled
				}
			}
			if value == nil {
				failure = "JavaScript await returned no value"
			} else if value.IsException() {
				failure = ctx.Exception().Error()
			}
			if value != nil {
				value.Free()
			}
		}
		s.mu.Lock()
		command.Record.Output = outputs
		if command.Ctx.Err() != nil {
			command.Record.State = "cancelled"
			command.Record.Error = "cancelled"
		} else if failure != "" {
			command.Record.State = "failed"
			command.Record.Error = failure
		} else {
			command.Record.State = "completed"
		}
		if s.active == command.Record {
			s.active = nil
		}
		close(command.Record.Done)
		s.mu.Unlock()
		if s.onDone != nil {
			s.onDone(command.Record)
		}
		running = context.Background()
	}
}

func brief(r *record) map[string]any {
	out := map[string]any{"execution_id": r.ID, "state": r.State}
	if len(r.Output) > 0 {
		out["print"] = r.Output
	}
	if r.Error != "" {
		out["error"] = map[string]any{"code": r.State, "message": r.Error}
	}
	if len(r.NativeIDs) > 0 {
		out["native_request_ids"] = append([]string(nil), r.NativeIDs...)
		var pending []string
		for _, id := range r.NativeIDs {
			if _, ok := r.NativeReceipts[id]; !ok {
				pending = append(pending, id)
			}
		}
		if len(pending) > 0 {
			out["native_pending_ids"] = pending
		}
	}
	if r.NativeError != nil {
		out["native_error"] = map[string]any{"code": r.NativeError.Code, "message": r.NativeError.Message, "retry_class": r.NativeError.RetryClass}
	}
	if len(r.Observations) > 0 {
		out["observations"] = r.Observations
	}
	if len(r.Actions) > 0 {
		out["actions"] = r.Actions
	}
	if len(r.Captures) > 0 {
		out["captures"] = r.Captures
	}
	return out
}

func nativeFacts(op, id string, result json.RawMessage) map[string]any {
	switch op {
	case "observe":
		var v struct {
			Coverage struct {
				Complete           bool     `json:"complete"`
				Truncated          bool     `json:"truncated"`
				Dirty              bool     `json:"dirty"`
				Continuation       string   `json:"continuation"`
				UnavailableSources []string `json:"unavailable_sources"`
			} `json:"coverage"`
		}
		if json.Unmarshal(result, &v) != nil {
			return nil
		}
		out := map[string]any{"native_request_id": id, "complete": v.Coverage.Complete, "truncated": v.Coverage.Truncated, "dirty": v.Coverage.Dirty}
		if v.Coverage.Continuation != "" {
			out["more"] = true
		}
		if len(v.Coverage.UnavailableSources) > 0 {
			out["unavailable_sources"] = v.Coverage.UnavailableSources
		}
		return out
	case "act":
		var v struct {
			RunID      string `json:"run_id"`
			Outcome    string `json:"outcome"`
			SeatHealth string `json:"seat_health"`
			Steps      []struct {
				ID           string `json:"id"`
				Channel      string `json:"channel"`
				Delivery     string `json:"delivery"`
				Verification string `json:"verification"`
				Fault        *struct {
					Code string `json:"code"`
				} `json:"fault"`
			} `json:"steps"`
			Input *struct {
				Restoration       string `json:"restoration"`
				RestorationReason string `json:"restoration_reason"`
			} `json:"input"`
		}
		if json.Unmarshal(result, &v) != nil {
			return nil
		}
		out := map[string]any{"native_request_id": id, "run_id": v.RunID, "outcome": v.Outcome, "seat_health": v.SeatHealth}
		steps := make([]map[string]any, 0, len(v.Steps))
		for _, step := range v.Steps {
			item := map[string]any{"id": step.ID, "channel": step.Channel, "delivery": step.Delivery, "verification": step.Verification}
			if step.Fault != nil {
				item["fault"] = step.Fault.Code
			}
			steps = append(steps, item)
		}
		out["steps"] = steps
		if v.Input != nil {
			out["restoration"] = v.Input.Restoration
			if v.Input.RestorationReason != "" {
				out["restoration_reason"] = v.Input.RestorationReason
			}
		}
		return out
	case "capture":
		var v struct {
			Files []struct {
				Path string `json:"path"`
			} `json:"files"`
		}
		if json.Unmarshal(result, &v) != nil {
			return nil
		}
		return map[string]any{"native_request_id": id, "images": len(v.Files)}
	}
	return nil
}

func toolText(out map[string]any) string {
	var lines []string
	// Printed facts have one canonical model-facing home: structuredContent.
	// Repeating the same AX node in text caused the adapter to receive it twice.
	if observations, ok := out["observations"].([]map[string]any); ok {
		for _, ob := range observations {
			if ob["complete"] != true || ob["dirty"] == true {
				lines = append(lines, fmt.Sprintf("observation %v: incomplete (truncated=%v, dirty=%v, more=%v)", ob["native_request_id"], ob["truncated"], ob["dirty"], ob["more"] == true))
			}
		}
	}
	if actions, ok := out["actions"].([]map[string]any); ok {
		for _, action := range actions {
			line := fmt.Sprintf("action %v: %v; seat=%v", action["run_id"], action["outcome"], action["seat_health"])
			if restoration, ok := action["restoration"]; ok {
				line += fmt.Sprintf("; restoration=%v", restoration)
			}
			if steps, ok := action["steps"].([]map[string]any); ok {
				for _, step := range steps {
					line += fmt.Sprintf("; %v=%v/%v via %v", step["id"], step["delivery"], step["verification"], step["channel"])
				}
			}
			lines = append(lines, line)
		}
	}
	if err, ok := out["error"].(map[string]any); ok {
		lines = append(lines, fmt.Sprintf("%v: %v", err["code"], err["message"]))
	}
	if err, ok := out["native_error"].(map[string]any); ok {
		lines = append(lines, fmt.Sprintf("native %v: %v", err["code"], err["message"]))
	}
	state, _ := out["state"].(string)
	id, _ := out["execution_id"].(string)
	if state != "completed" || len(lines) == 0 {
		lines = append(lines, "state: "+state)
	}
	lines = append(lines, "execution_id: "+id)
	return strings.Join(lines, "\n")
}

func (s *supervisor) call(ctx context.Context, in input) map[string]any {
	if in.ExecutionID == "" {
		return map[string]any{"error": map[string]any{"code": "invalid_request", "message": "execution_id is required"}}
	}
	if len(in.ExecutionID) > 128 {
		return map[string]any{"error": map[string]any{"code": "invalid_request", "message": "execution_id is too long"}}
	}
	s.mu.Lock()
	r := s.records[in.ExecutionID]
	switch in.Operation {
	case "exec":
		if in.Code == "" || len(in.Code) > 65536 {
			s.mu.Unlock()
			return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "invalid_request", "message": "code is required and must fit 64 KiB"}}
		}
		digest := sha256.Sum256([]byte(in.Code))
		hash := hex.EncodeToString(digest[:])
		if r != nil {
			if r.Hash != hash {
				s.mu.Unlock()
				return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "execution_conflict", "message": "same ID, different script"}}
			}
			done := r.Done
			s.mu.Unlock()
			select {
			case <-done:
				s.mu.Lock()
				out := brief(r)
				s.mu.Unlock()
				return out
			case <-ctx.Done():
				return map[string]any{"execution_id": in.ExecutionID, "state": "running"}
			}
		}
		if s.childDead {
			s.mu.Unlock()
			return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "state_lost", "message": "script subprocess exited; Session JavaScript state is lost"}}
		}
		if s.active != nil {
			s.mu.Unlock()
			return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "script_busy", "message": "query or cancel the active execution"}}
		}
		if len(s.records) >= 128 {
			s.mu.Unlock()
			return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "history_full", "message": "session execution history is full"}}
		}
		runCtx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		r = &record{ID: in.ExecutionID, Hash: hash, State: "running", Done: make(chan struct{}), Cancel: cancel, RunCtx: runCtx}
		s.records[r.ID] = r
		s.active = r
		s.mu.Unlock()
		s.commands <- scriptCommand{Ctx: runCtx, Code: in.Code, Record: r}
		select {
		case <-r.Done:
			s.mu.Lock()
			out := brief(r)
			s.mu.Unlock()
			return out
		case <-ctx.Done():
			s.cancelRecord(r)
			return map[string]any{"execution_id": in.ExecutionID, "state": "cancelling"}
		}
	case "status", "result":
		if r == nil {
			s.mu.Unlock()
			return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "execution_not_found", "message": "no execution with this ID"}}
		}
		out := brief(r)
		if in.Operation == "result" && len(r.NativeReceipts) > 0 {
			out["native_receipts"] = r.NativeReceipts
		}
		s.mu.Unlock()
		return out
	case "cancel":
		if r == nil {
			s.mu.Unlock()
			return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "execution_not_found", "message": "no execution with this ID"}}
		}
		if r.State == "running" {
			r.State = "cancelling"
			r.Cancel()
			go s.cancelScript(r.ID)
		}
		out := brief(r)
		s.mu.Unlock()
		return out
	default:
		s.mu.Unlock()
		return map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "invalid_request", "message": "unknown operation"}}
	}
}

func newServer(native *host.Client, assetsDir string) *mcp.Server {
	s := newSupervisor(native, assetsDir)
	return newServerWithSupervisor(s)
}

func newServerWithSupervisor(s *supervisor) *mcp.Server {
	server := mcp.NewServer(&mcp.Implementation{Name: "dtw-poc", Version: "0.0.0-poc"}, nil)
	mcp.AddTool(server, &mcp.Tool{Name: "exec", Description: "POC: run approved JavaScript; query and cancel by original execution ID"}, func(ctx context.Context, _ *mcp.CallToolRequest, in input) (*mcp.CallToolResult, any, error) {
		out := s.call(ctx, in)
		if os.Getenv("DTW_POC_TEXT_OUTPUT") == "1" {
			content := []mcp.Content{&mcp.TextContent{Text: modelResultText(in, out)}}
			if in.Operation == "result" && in.IncludeImage {
				png, err := s.imageFor(in.ExecutionID)
				if err != nil {
					content[0] = &mcp.TextContent{Text: in.ExecutionID + " · image unavailable: " + err.Error()}
					return &mcp.CallToolResult{Content: content, IsError: true}, nil, nil
				}
				content = append(content, &mcp.ImageContent{Data: png, MIMEType: "image/png"})
			}
			_, isError := out["error"]
			return &mcp.CallToolResult{Content: content, IsError: isError}, nil, nil
		}
		compact := os.Getenv("DTW_POC_COMPACT_OUTPUT") == "1" && in.Operation == "exec"
		message := toolText(out)
		if compact {
			view := compactExec(out)
			message = compactToolText(view)
			out = compactStructured(view)
		}
		_, err := json.Marshal(out)
		if err != nil {
			return nil, nil, err
		}
		_, isError := out["error"]
		content := []mcp.Content{&mcp.TextContent{Text: message}}
		if in.Operation == "result" && in.IncludeImage {
			png, imageErr := s.imageFor(in.ExecutionID)
			if imageErr != nil {
				failure := map[string]any{"execution_id": in.ExecutionID, "error": map[string]any{"code": "image_unavailable", "message": imageErr.Error()}}
				return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: toolText(failure)}}, IsError: true}, failure, nil
			}
			content = append(content, &mcp.ImageContent{Data: png, MIMEType: "image/png"})
		}
		return &mcp.CallToolResult{Content: content, IsError: isError}, out, nil
	})
	return server
}

func runServer(ctx context.Context) error {
	var native *host.Client
	var assetsDir string
	if helper := os.Getenv("DTW_POC_HELPER"); helper != "" {
		var err error
		assetsDir = os.Getenv("DTW_POC_ASSETS")
		if assetsDir == "" {
			assetsDir, err = os.MkdirTemp("", "dtw-native-go-poc-assets-")
			if err != nil {
				return err
			}
			defer os.RemoveAll(assetsDir)
		}
		if err := os.MkdirAll(assetsDir, 0700); err != nil {
			return err
		}
		native, err = host.Start(ctx, host.Options{Executable: helper, AssetsDir: assetsDir, InputMode: dw.InputModeCooperative})
		if err != nil {
			return err
		}
		defer native.Close()
		if err := native.BeginTurn(ctx, "session"); err != nil {
			return err
		}
		if os.Getenv("DTW_POC_NO_AUTH") != "1" {
			if app := os.Getenv("DTW_POC_WRITE_APP"); app != "" {
				if err := native.Declare(ctx, "session", app, ""); err != nil {
					return err
				}
			}
			if title := os.Getenv("DTW_POC_WRITE_WINDOW"); title != "" {
				if err := native.Declare(ctx, "session", "", title); err != nil {
					return err
				}
			}
		}
	}
	return newServer(native, assetsDir).Run(ctx, &mcp.StdioTransport{})
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--script-child" {
		if err := runScriptChild(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	if err := runServer(context.Background()); err != nil && !errors.Is(err, context.Canceled) {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
