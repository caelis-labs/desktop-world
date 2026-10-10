package host

import (
	"bufio"
	"context"
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"strings"
	"testing"
	"time"
)

// The same subprocess test runs on Windows CI without requiring UI permission.
// It validates the real inherited-pipe transport, separately from desktop acceptance.
func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "serve" {
		os.Exit(transportChild())
	}
	os.Exit(m.Run())
}
func transportChild() int {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	in, out, err := helper.InheritedControlFiles()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}
	defer in.Close()
	defer out.Close()
	w, f, err := dwtest.New(ctx)
	if err != nil {
		return 2
	}
	f.Form()
	f.Enqueue("invoke", dwtest.Behavior{Block: make(chan struct{}), Before: func(*dwtest.Fixture) { fmt.Fprintln(os.Stderr, "slow-running") }})
	policy := dw.InputShared
	for i, arg := range os.Args {
		if arg == "--input-policy" && i+1 < len(os.Args) {
			policy = dw.InputPolicy(os.Args[i+1])
		}
	}
	// A controlled wrong-policy child exercises the fail-closed handshake.
	if strings.Contains(strings.Join(os.Args[1:], " "), "test-wrong-policy") {
		policy = dw.InputShared
	}
	server, err := helper.New(ctx, w, helper.Config{Managed: true, FullOutput: true, InputPolicy: policy})
	if err != nil {
		return 2
	}
	go func() { server.ServeControl(ctx, in, out); cancel(); os.Stdin.Close() }()
	err = server.Serve(ctx, os.Stdin, os.Stdout)
	closeCtx, closeCancel := context.WithTimeout(context.Background(), time.Second)
	defer closeCancel()
	w.Close(closeCtx)
	if err != nil && ctx.Err() == nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}
	return 0
}

func TestManagedHandshakeReportsTrustedInputPolicy(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, policy := range []dw.InputPolicy{"", dw.InputShared, dw.InputNoShared} {
		t.Run(string(policy), func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			c, err := Start(ctx, Options{Executable: executable, InputPolicy: policy})
			if err != nil {
				t.Fatal(err)
			}
			defer c.Close()
			want := policy
			if want == "" {
				want = dw.InputShared
			}
			if c.Hello.InputPolicy != want {
				t.Fatalf("effective policy=%q, want %q", c.Hello.InputPolicy, want)
			}
		})
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if c, err := Start(ctx, Options{Executable: executable, InputPolicy: dw.InputNoShared, AssetsDir: "test-wrong-policy"}); err == nil {
		c.Close()
		t.Fatal("accepted a helper advertising a weaker policy")
	}
}
func TestManagedInheritedTransportCancelsBusyProvider(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	logR, logW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer logR.Close()
	defer logW.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	c, err := Start(ctx, Options{Executable: executable, Stderr: logW})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	if c.Hello.CoreNoAuth {
		t.Skip("grant transport contract does not apply to the core POC helper")
	}
	if err = c.BeginTurn(ctx, "t1"); err != nil {
		t.Fatal(err)
	}
	reply, err := c.Call(ctx, "t1", "inventory", "observe", dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionOutline, Fields: []string{"name", "app"}})
	if err != nil || reply.Error != nil {
		t.Fatal(reply, err)
	}
	var ob dw.Observation
	if err = protocol.Decode(reply.Result, &ob); err != nil {
		t.Fatal(err)
	}
	var app, button dw.Ref
	for _, o := range ob.Objects {
		if o.Name.Value != nil && *o.Name.Value == "提交" {
			app = o.App
			button = o.Ref
		}
	}
	if app == "" || button == "" {
		t.Fatal("fixture app/button missing")
	}
	if err = c.Grant(ctx, "t1", app); err != nil {
		t.Fatal(err)
	}
	done := make(chan Reply, 1)
	errors := make(chan error, 1)
	go func() {
		r, e := c.Call(ctx, "t1", "slow", "act", struct{ Steps []dw.Step }{[]dw.Step{{ID: "blocked", Op: "invoke", Target: dw.Target{Ref: button}}, {ID: "never", Op: "invoke", Target: dw.Target{Ref: button}}}})
		done <- r
		errors <- e
	}()
	started := make(chan bool, 1)
	go func() { scan := bufio.NewScanner(logR); started <- scan.Scan() && scan.Text() == "slow-running" }()
	select {
	case ready := <-started:
		if !ready {
			t.Fatal("child did not start provider")
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	start := time.Now()
	if err = c.EndTurn(ctx, "t1"); err != nil {
		t.Fatal(err)
	}
	if time.Since(start) > time.Second {
		t.Fatal("control queued behind provider")
	}
	var result Reply
	select {
	case result = <-done:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	if e := <-errors; e != nil {
		t.Fatal(e)
	}
	var receipt dw.Receipt
	if err = protocol.Decode(result.Result, &receipt); err != nil {
		t.Fatal(err)
	}
	if receipt.Outcome == "completed" || receipt.Steps[1].State != "skipped" {
		t.Fatal("continued after authority ended", receipt)
	}
	original, err := c.Reconcile(ctx, "t1", "slow")
	if err != nil || original.ID != result.ID || string(original.Result) != string(result.Result) {
		t.Fatal("lost original result", original, err)
	}
	late, err := c.Call(ctx, "t1", "late", "act", struct{ Steps []dw.Step }{[]dw.Step{{ID: "denied", Op: "invoke", Target: dw.Target{Ref: button}}}})
	if err != nil || late.Error == nil || late.Error.Code != "turn_expired" {
		t.Fatal(late, err)
	}
}

func TestHostRejectsDifferentInputModeBeforeGrants(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	// The controlled child intentionally ignores the new option and advertises
	// the default shared mode. The host cannot mistake it for cooperative input.
	if c, err := Start(ctx, Options{Executable: executable, InputMode: dw.InputModeCooperative}); err == nil {
		c.Close()
		t.Fatal("accepted wrong helper input mode")
	}
	if c, err := Start(ctx, Options{Executable: executable, InputMode: "invalid"}); err == nil {
		c.Close()
		t.Fatal("accepted invalid input mode")
	}
}
