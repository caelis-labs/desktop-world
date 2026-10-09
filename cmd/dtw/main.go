// dtw exposes discovery and a persistent, host-owned stdio helper.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"runtime"
	"runtime/debug"
	"syscall"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/auditlog"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"github.com/caelis-labs/desktop-world/local"
	"github.com/caelis-labs/desktop-world/protocol"
)

var releaseVersion = "dev"

const usage = `Desktop World — native desktop operations, persistent JSON sessions.

dtw version                Protocol, build revision and platform; no desktop access.
dtw doctor                 Read-only permission/environment probe; never prompts.
dtw schema [operation] [action]     JSON request schema; no native desktop access.
dtw session [host options] Trusted owner facade for TS/Python/Rust SDKs.
dtw auth list|add|revoke --session OWNER_FILE   Dynamic application grants.
dtw cursor-overlay         Internal click-through Agent pointer display (stdin control).
dtw plugin-node [MCP options]  Internal Lite launcher; requires absolute DTW_NODE_PATH (Node 24).
dtw serve [host options]   New World for the lifetime of this stdio process.

serve host options:
  --input-policy POLICY  Host ceiling: shared_input (default) or no_shared_input.
	--input-mode MODE      shared (default) or cooperative (short foreground transactions).
  --write-app NAME       Approve one exact application name; pending until uniquely observed. Repeatable.
  --write-app-window TITLE  Grant its owning app; resolves duplicate app names by exact window title.
  --desktop-write        Explicitly grant desktop-wide writes instead of named apps.
  --raw-input            Permit absolute Point input; requires --desktop-write.
  --assets-dir PATH      Enable capture and save PNG files in this host-chosen directory.
  --audit PATH           Metadata-only JSONL; collision selects a new session file.
  --audit-mode MODE      rotate (default), append (single writer) or create.
  --full-output          Typed wire facts and exact timestamps; default is compact UI facts.
  --host-control         Managed mode: private inherited request/reply pipes.
                         No startup write flags. Host controls application grants per turn.

Send one JSON object per line; receive hello, then {id,protocol,world,result,error}.
Supported verbs: observe, read, sync, act, capture, get, cancel.
Example read:
{"id":"inventory-1","op":"observe","args":{"scope":{"desktop":true},"projection":"summary","fields":["name","role"],"budget":{"max_results":64}}}

For writes, get schema act ACTION as needed. args contains steps and optional timeout_ms.
Helper supplies epoch and request_id from the stable envelope id. Reuse that id
and the SAME body after transport uncertainty; never blindly replay effects.
New process = new epoch, invalid old Refs, no persisted exactly-once guarantee.
No network listener, login, API key, automatic permission prompts, or implicit rebinding after an app restart.
Stdout is JSON only except --help; diagnostics go to stderr. --json is accepted.
`

type names []string

func (n *names) String() string     { return fmt.Sprint([]string(*n)) }
func (n *names) Set(v string) error { *n = append(*n, v); return nil }

func main() {
	if len(os.Args) > 1 && os.Args[1] == "plugin-node" {
		if err := runPluginNode(os.Args[2:]); err != nil {
			fmt.Fprintln(os.Stderr, "Desktop World Lite:", err)
			os.Exit(1)
		}
		return
	}
	if err := run(); err != nil {
		b, _ := json.Marshal(map[string]any{"error": map[string]string{"code": "command_failed", "message": err.Error()}})
		fmt.Println(string(b))
		os.Exit(1)
	}
}
func run() (runErr error) {
	args := os.Args[1:]
	if len(args) > 0 && args[0] == "--json" {
		args = args[1:]
	}
	if len(args) == 0 || args[0] == "--help" || args[0] == "help" {
		fmt.Print(usage)
		return nil
	}
	if args[0] == "session" {
		return runSession(args[1:])
	}
	if args[0] == "auth" {
		return runAuth(args[1:])
	}
	if args[0] == "cursor-overlay" {
		if len(args) != 1 {
			return fmt.Errorf("cursor-overlay takes no arguments")
		}
		return runCursorOverlay()
	}
	if args[0] == "version" || args[0] == "--version" || args[0] == "-v" {
		v := map[string]any{"version": releaseVersion, "protocol": helper.Version, "host_control": helper.ControlVersion, "go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH}
		if info, ok := debug.ReadBuildInfo(); ok {
			for _, setting := range info.Settings {
				if setting.Key == "vcs.revision" || setting.Key == "vcs.modified" || setting.Key == "vcs.time" {
					v[setting.Key] = setting.Value
				}
			}
		}
		return json.NewEncoder(os.Stdout).Encode(v)
	}
	if args[0] == "schema" {
		var s any = helper.SchemaIndex()
		if len(args) > 3 {
			return fmt.Errorf("use schema [operation] or schema act ACTION")
		}
		if len(args) >= 2 {
			operationSchema := helper.Schema(args[1])
			if operationSchema == nil {
				return fmt.Errorf("unknown operation")
			}
			if len(args) == 3 {
				if args[1] != "act" {
					return fmt.Errorf("action selector requires schema act")
				}
				operationSchema = helper.ActionSchema(args[2])
				if operationSchema == nil {
					return fmt.Errorf("unknown action")
				}
			}
			s = operationSchema
		}
		return json.NewEncoder(os.Stdout).Encode(s)
	}
	if args[0] != "doctor" && args[0] != "serve" {
		return fmt.Errorf("unknown command; use --help")
	}
	var c helper.Config
	var apps, appWindows names
	var auditPath, auditMode, inputPolicy, inputMode string
	f := flag.NewFlagSet(args[0], flag.ContinueOnError)
	f.SetOutput(os.Stderr)
	f.StringVar(&inputMode, "input-mode", "", "trusted delivery mode: shared or cooperative (short foreground transactions)")
	f.StringVar(&inputPolicy, "input-policy", "", "trusted ceiling: shared_input or no_shared_input")
	f.Var(&apps, "write-app", "allow exact live application name")
	f.Var(&appWindows, "write-app-window", "allow the application owning an exact window title")
	f.BoolVar(&c.DesktopWrite, "desktop-write", false, "allow desktop writes")
	f.BoolVar(&c.RawInput, "raw-input", false, "allow absolute Point input")
	f.StringVar(&c.AssetsDir, "assets-dir", "", "capture destination")
	f.StringVar(&auditPath, "audit", "", "metadata-only audit file")
	f.StringVar(&auditMode, "audit-mode", "rotate", "rotate (default), append or create")
	f.BoolVar(&c.FullOutput, "full-output", false, "retain typed wire facts and per-object timestamps")
	f.BoolVar(&c.Managed, "host-control", false, "trusted host control on private inherited pipes")
	openWorld := backgroundPOCFlag(f)
	if err := f.Parse(args[1:]); err != nil {
		return err
	}
	if f.NArg() != 0 {
		return fmt.Errorf("unexpected positional arguments")
	}
	if args[0] == "doctor" && len(args) > 1 {
		return fmt.Errorf("doctor takes no host permission flags")
	}
	c.InputMode = dw.InputMode(inputMode)
	if err := c.InputMode.Validate(); err != nil {
		return err
	}
	c.InputPolicy = dw.InputPolicy(inputPolicy)
	if err := c.InputPolicy.Validate(); err != nil {
		return err
	}
	c.WriteApps = apps
	c.WriteAppWindows = appWindows
	c.Capture = c.AssetsDir != ""
	if c.DesktopWrite && len(apps)+len(appWindows) > 0 {
		return fmt.Errorf("choose write-app scopes or desktop-write, not both")
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	var controlIn, controlOut *os.File
	if c.Managed {
		var err error
		controlIn, controlOut, err = helper.InheritedControlFiles()
		if err != nil {
			return err
		}

		for _, f := range []*os.File{controlIn, controlOut} {
			if f == nil {
				return fmt.Errorf("host-control requires inherited pipes")
			}
			info, err := f.Stat()
			if err != nil || info.Mode()&os.ModeNamedPipe == 0 {
				return fmt.Errorf("host-control requires private inherited pipes")
			}
			defer f.Close()
		}
	}
	openCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
	w, err := openWorld(openCtx, local.Options{InputMode: c.InputMode})
	cancel()
	if err != nil {
		return err
	}
	defer func() {
		closeCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		if err := w.Close(closeCtx); err != nil {
			fmt.Fprintln(os.Stderr, "close:", err)
			if runErr == nil {
				runErr = err
			}
		}
	}()
	if args[0] == "doctor" {
		probeCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		env, err := w.Environment(probeCtx)
		if err != nil {
			return err
		}
		b, err := protocol.Marshal(struct {
			Protocol, Auth string
			Environment    any
		}{helper.Version, "OS permissions; no API credential", env})
		if err != nil {
			return err
		}
		fmt.Println(string(b))
		return nil
	}
	if auditPath != "" {
		f, err := auditlog.Open(auditPath, auditMode)
		if err != nil {
			return err
		}
		defer f.Close()
		c.Audit = f
		c.AuditPath = f.Path
	}
	initCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
	server, err := helper.New(initCtx, w, c)
	cancel()
	if err != nil {
		return err
	}
	// Closing stdin on a signal unblocks the scanner, then Serve cancels calls
	// and the World gets a bounded opportunity to release owned input.
	go func() { <-ctx.Done(); _ = os.Stdin.Close() }()
	if c.Managed {
		go func() {
			if err := server.ServeControl(ctx, controlIn, controlOut); err != nil {
				fmt.Fprintln(os.Stderr, "host control:", err)
			}
			stop() // Loss of the private owner channel stops the data server too.
		}()
	}
	serveErr := server.Serve(ctx, os.Stdin, os.Stdout)
	if ctx.Err() != nil {
		return nil
	}
	return serveErr
}
