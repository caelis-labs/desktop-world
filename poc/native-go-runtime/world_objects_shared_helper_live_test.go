package main

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Isolated architectural POC: two MCP/QuickJS Sessions use one native helper
// process. This tests whether the observed ScreenCaptureKit failure is tied to
// separate native processes; it is not a product Session implementation.
func TestTwoRealObjectSessionsCanShareOneNativeCaptureOwner(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	helper := os.Getenv("DTW_POC_HELPER")
	if title == "" || helper == "" {
		t.Skip("requires selected real window and isolated core helper")
	}
	t.Setenv("DTW_POC_CHILD", "1")
	t.Setenv("DTW_POC_LEGACY_OUTPUT", "0")
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	assets := t.TempDir()
	native, err := host.Start(ctx, host.Options{Executable: helper, AssetsDir: assets, InputMode: dw.InputModeCooperative})
	if err != nil {
		t.Fatal(err)
	}
	defer native.Close()
	if err := native.BeginTurn(ctx, "session"); err != nil {
		t.Fatal(err)
	}
	open := func(label string) *mcp.ClientSession {
		supervisor := newSupervisor(native, assets)
		t.Cleanup(func() {
			if supervisor.child != nil {
				_ = supervisor.child.cmd.Process.Kill()
			}
		})
		serverTransport, clientTransport := mcp.NewInMemoryTransports()
		serverSession, err := newServerWithSupervisor(supervisor).Connect(ctx, serverTransport, nil)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = serverSession.Close() })
		client := mcp.NewClient(&mcp.Implementation{Name: "shared-capture-" + label, Version: "1"}, nil)
		session, err := client.Connect(ctx, clientTransport, nil)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = session.Close() })
		return session
	}
	a, b := open("A"), open("B")
	call := func(session *mcp.ClientSession, id, code string) (*mcp.CallToolResult, error) {
		return session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
	}
	for _, peer := range []struct {
		label   string
		session *mcp.ClientSession
	}{{"A", a}, {"B", b}} {
		code := `state.owner=` + string(mustJSON(peer.label)) + `;const app=await dtw.app('Obsidian');state.win=await app.window(` + string(mustJSON(title)) + `);print(state.owner+' '+state.win.id);`
		result, err := call(peer.session, "shared-"+peer.label+"-setup", code)
		if err != nil || result.IsError || !strings.Contains(result.Content[0].(*mcp.TextContent).Text, peer.label+" W") {
			t.Fatalf("Session %s shared native setup: %v %+v", peer.label, err, result)
		}
	}
	type reply struct {
		label     string
		text      string
		err       error
		toolError bool
		spent     time.Duration
	}
	start := make(chan struct{})
	done := make(chan reply, 2)
	for _, peer := range []struct {
		label   string
		session *mcp.ClientSession
	}{{"A", a}, {"B", b}} {
		go func(label string, session *mcp.ClientSession) {
			<-start
			started := time.Now()
			result, err := call(session, "shared-"+label+"-capture", `await state.win.capture();print(state.owner);`)
			message := ""
			if result != nil && len(result.Content) > 0 {
				message = result.Content[0].(*mcp.TextContent).Text
			}
			done <- reply{label: label, text: message, err: err, toolError: result != nil && result.IsError, spent: time.Since(started)}
		}(peer.label, peer.session)
	}
	close(start)
	for range 2 {
		got := <-done
		t.Logf("Session %s shared native capture=%s result=%q error=%v tool_error=%t", got.label, got.spent.Round(time.Millisecond), got.text, got.err, got.toolError)
		if got.err != nil || got.toolError || !strings.Contains(got.text, "capture: 1 image(s) ready") ||
			!strings.Contains(got.text, " · "+got.label) {
			t.Errorf("Session %s shared owner capture failed: %v %q", got.label, got.err, got.text)
		}
	}
	if !t.Failed() {
		for _, peer := range []struct {
			label   string
			session *mcp.ClientSession
		}{{"A", a}, {"B", b}} {
			result, err := peer.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
				"operation": "result", "execution_id": "shared-" + peer.label + "-capture", "include_image": true,
			}})
			if err != nil || result.IsError || len(result.Content) != 2 {
				t.Errorf("Session %s on-demand image: %v %+v", peer.label, err, result)
				continue
			}
			image, ok := result.Content[1].(*mcp.ImageContent)
			if !ok || image.MIMEType != "image/png" || len(image.Data) < 8 || string(image.Data[:8]) != "\x89PNG\r\n\x1a\n" {
				t.Errorf("Session %s shared native image invalid", peer.label)
			}
		}
	}
}
