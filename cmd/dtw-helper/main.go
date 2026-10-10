// dtw-helper is the private native engine process used by the Go MCP runtime.
// Its command surface is intentionally limited to the host protocol and the
// nonactivating virtual cursor overlay.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	dw "github.com/caelis-labs/desktop-world/internal/world"
	"github.com/caelis-labs/desktop-world/internal/ipc/helper"
	"github.com/caelis-labs/desktop-world/internal/platform"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "dtw-helper:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 1 && args[0] == "cursor-overlay" {
		return runCursorOverlay()
	}
	if len(args) == 1 && args[0] == "version" {
		return json.NewEncoder(os.Stdout).Encode(map[string]string{"protocol": helper.Version})
	}
	if len(args) == 0 || args[0] != "serve" {
		return errors.New("private helper accepts serve or cursor-overlay")
	}
	var cfg helper.Config
	var inputMode, inputPolicy string
	f := flag.NewFlagSet("serve", flag.ContinueOnError)
	f.SetOutput(os.Stderr)
	f.BoolVar(&cfg.Managed, "host-control", false, "private host control pipes")
	f.BoolVar(&cfg.FullOutput, "full-output", false, "full internal receipts")
	f.StringVar(&inputMode, "input-mode", "", "native input mode")
	f.StringVar(&inputPolicy, "input-policy", "", "input ceiling")
	f.StringVar(&cfg.AssetsDir, "assets-dir", "", "private capture directory")
	if err := f.Parse(args[1:]); err != nil {
		return err
	}
	if f.NArg() != 0 || !cfg.Managed || !cfg.FullOutput {
		return errors.New("helper requires managed host control and full internal receipts")
	}
	cfg.InputMode = dw.InputMode(inputMode)
	cfg.InputPolicy = dw.InputPolicy(inputPolicy)
	cfg.Capture = cfg.AssetsDir != ""
	if err := cfg.InputMode.Validate(); err != nil {
		return err
	}
	if err := cfg.InputPolicy.Validate(); err != nil {
		return err
	}
	controlIn, controlOut, err := helper.InheritedControlFiles()
	if err != nil {
		return err
	}
	for _, pipe := range []*os.File{controlIn, controlOut} {
		if pipe == nil {
			return errors.New("missing inherited host control pipe")
		}
		info, err := pipe.Stat()
		if err != nil || info.Mode()&os.ModeNamedPipe == 0 {
			return errors.New("host control requires private inherited pipes")
		}
		defer pipe.Close()
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	openCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
	world, err := openNativeWorld(openCtx, local.Options{InputMode: cfg.InputMode})
	cancel()
	if err != nil {
		return err
	}
	defer func() {
		closeCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = world.Close(closeCtx)
	}()
	initCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
	server, err := helper.New(initCtx, world, cfg)
	cancel()
	if err != nil {
		return err
	}
	go func() { <-ctx.Done(); _ = os.Stdin.Close() }()
	go func() {
		if err := server.ServeControl(ctx, controlIn, controlOut); err != nil {
			fmt.Fprintln(os.Stderr, "host control:", err)
		}
		stop()
	}()
	err = server.Serve(ctx, os.Stdin, os.Stdout)
	if ctx.Err() != nil {
		return nil
	}
	return err
}
