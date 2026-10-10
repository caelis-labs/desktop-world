//go:build darwin && cgo && dtw_poc_exactgrant

// exacthelper is the isolated real native helper for exact-window grant POC.
// A normal build never compiles its backend identity hook or this executable.
package main

import (
	"bufio"
	"context"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"syscall"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/auditlog"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"github.com/caelis-labs/desktop-world/local"
	"github.com/caelis-labs/desktop-world/protocol"
)

func serveControl(ctx context.Context, server *helper.Server, gate *gate, in io.Reader, out io.Writer) error {
	scanner := bufio.NewScanner(in)
	scanner.Buffer(make([]byte, 4096), 16384)
	for scanner.Scan() {
		if err := ctx.Err(); err != nil {
			return err
		}
		var req helper.ControlRequest
		var reply helper.Response
		if err := protocol.Decode(scanner.Bytes(), &req); err != nil {
			reply = helper.Response{Protocol: helper.ControlVersion, Error: dw.Invalid("invalid host control request")}
		} else {
			reply = server.Control(ctx, req)
			if reply.Error == nil {
				switch req.Op {
				case "begin_turn":
					gate.reset(req.Turn)
				case "end_turn":
					gate.reset("")
				case "declare", "grant", "revoke", "grants":
					status := reply
					if req.Op != "grants" {
						status = server.Control(ctx, helper.ControlRequest{ID: "poc-grant-sync", Op: "grants", Turn: req.Turn})
					}
					if status.Error == nil {
						if grants, ok := status.Result.(helper.GrantStatus); ok {
							gate.syncStatus(ctx, grants)
							if req.Op == "grants" {
								reply.Result = gate.visibleStatus(ctx, grants)
							}
						}
					}
				}
			}
		}
		encoded, err := protocol.Marshal(reply)
		if err != nil {
			return err
		}
		if _, err := fmt.Fprintln(out, string(encoded)); err != nil {
			return err
		}
	}
	return scanner.Err()
}

func run() error {
	if len(os.Args) < 2 || os.Args[1] != "serve" {
		return fmt.Errorf("POC helper supports serve only")
	}
	var config helper.Config
	var inputMode, inputPolicy, auditPath, auditMode string
	flags := flag.NewFlagSet("serve", flag.ContinueOnError)
	flags.StringVar(&inputMode, "input-mode", "", "native input mode")
	flags.StringVar(&inputPolicy, "input-policy", "", "host input policy")
	flags.StringVar(&config.AssetsDir, "assets-dir", "", "host-selected capture directory")
	flags.StringVar(&auditPath, "audit", "", "metadata audit path")
	flags.StringVar(&auditMode, "audit-mode", "rotate", "audit mode")
	flags.BoolVar(&config.Managed, "host-control", false, "private control pipes")
	flags.BoolVar(&config.FullOutput, "full-output", false, "full typed results")
	if err := flags.Parse(os.Args[2:]); err != nil {
		return err
	}
	if flags.NArg() != 0 || !config.Managed {
		return fmt.Errorf("POC helper requires managed host control")
	}
	config.InputMode, config.InputPolicy = dw.InputMode(inputMode), dw.InputPolicy(inputPolicy)
	if err := config.InputMode.Validate(); err != nil {
		return err
	}
	if err := config.InputPolicy.Validate(); err != nil {
		return err
	}
	config.Capture = config.AssetsDir != ""
	controlIn, controlOut, err := helper.InheritedControlFiles()
	if err != nil {
		return err
	}
	defer controlIn.Close()
	defer controlOut.Close()
	for _, f := range []*os.File{controlIn, controlOut} {
		info, err := f.Stat()
		if err != nil || info.Mode()&os.ModeNamedPipe == 0 {
			return fmt.Errorf("host control requires inherited pipes")
		}
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	openCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
	world, err := local.Open(openCtx, local.Options{InputMode: config.InputMode})
	cancel()
	if err != nil {
		return err
	}
	defer func() {
		c, done := context.WithTimeout(context.Background(), 2*time.Second)
		defer done()
		_ = world.Close(c)
	}()
	source, ok := world.(identitySource)
	if !ok {
		return fmt.Errorf("native world has no POC window identity resolver")
	}
	gate := newGate(source)
	if auditPath != "" {
		file, err := auditlog.Open(auditPath, auditMode)
		if err != nil {
			return err
		}
		defer file.Close()
		config.Audit, config.AuditPath = file, file.Path
	}
	initCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
	server, err := helper.New(initCtx, guardedWorld{World: world, gate: gate}, config)
	cancel()
	if err != nil {
		return err
	}
	go func() { <-ctx.Done(); _ = os.Stdin.Close() }()
	go func() {
		if err := serveControl(ctx, server, gate, controlIn, controlOut); err != nil && ctx.Err() == nil {
			fmt.Fprintln(os.Stderr, "POC host control:", err)
		}
		stop()
	}()
	err = server.Serve(ctx, os.Stdin, os.Stdout)
	if ctx.Err() != nil {
		return nil
	}
	return err
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "exact POC helper:", err)
		os.Exit(1)
	}
}
