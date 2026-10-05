package main

import (
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/internal/auditlog"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"os"
	"testing"
	"time"
)

// Compiled with go test -c only. Production dtw contains no fixture switch.
// The SDK fixture uses the production session supervisor and real inherited pipes.
func TestMain(m *testing.M) {
	if len(os.Args) > 1 {
		switch os.Args[1] {
		case "session", "auth":
			main()
			os.Exit(0)
		case "serve":
			if err := sdkFixture(); err != nil {
				fmt.Fprintln(os.Stderr, err)
				os.Exit(2)
			}
			os.Exit(0)
		}
	}
	os.Exit(m.Run())
}
func sdkFixture() error {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	in, out, err := helper.InheritedControlFiles()
	if err != nil {
		return err
	}
	defer in.Close()
	defer out.Close()
	w, f, err := dwtest.New(ctx)
	if err != nil {
		return err
	}
	defer func() { c, stop := context.WithTimeout(context.Background(), time.Second); defer stop(); w.Close(c) }()
	f.Form()
	f.Enqueue("invoke", dwtest.Behavior{Block: make(chan struct{})})
	f.SetFocus("field")
	f.Add(dwtest.Node{ID: "other", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Other")}})
	f.Add(dwtest.Node{ID: "other-window", App: "other", Parent: "other", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("Other Window")}})
	f.Add(dwtest.Node{ID: "other-field", App: "other", Parent: "other-window", Window: "other-window", Object: dw.Object{Kind: dw.KindUI, Role: "text_field", Name: dw.Known("Other Field")}})
	f.Update("field", func(n *dwtest.Node) { n.Object.States["checked"] = dw.Known(false) })
	config := helper.Config{Managed: true, FullOutput: true}
	for i, arg := range os.Args {
		if i+1 < len(os.Args) {
			switch arg {
			case "--input-policy":
				config.InputPolicy = dw.InputPolicy(os.Args[i+1])
			case "--audit":
				log, e := auditlog.Open(os.Args[i+1], "rotate")
				if e != nil {
					return e
				}
				defer log.Close()
				config.Audit = log
				config.AuditPath = log.Path
			}
		}
	}
	server, err := helper.New(ctx, w, config)
	if err != nil {
		return err
	}
	go func() { server.ServeControl(ctx, in, out); cancel(); os.Stdin.Close() }()
	serveErr := server.Serve(ctx, os.Stdin, os.Stdout)
	if ctx.Err() != nil {
		return nil
	}
	return serveErr
}
