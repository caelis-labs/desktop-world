package main

import (
	"context"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in selected-App chain test. The only action is semantic scroll to an
// already named control; no keyboard, pointer or note data is sent.
func TestSelectedRealAppObjectSurfaceReadAndSemanticScroll(t *testing.T) {
	if os.Getenv("DTW_TEST_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_TEST_REAL_WINDOW_TITLE")
	if title == "" || os.Getenv("DTW_TEST_HELPER") == "" {
		t.Skip("requires selected exact window title and native helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_TEST_CHILD=1", "DTW_TEST_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "selected-object-surface", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, operation, code string) *mcp.CallToolResult {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": operation, "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		if res.StructuredContent != nil {
			t.Fatalf("duplicated model content: %+v", res.StructuredContent)
		}
		return res
	}
	code := `if(dtw.grants!==undefined||dtw.revokeGrant!==undefined)throw Error('DTW grants still exposed');const app=await dtw.app('Obsidian');const win=await app.window(` + string(mustJSON(title)) + `);const control=await win.one({name:'新建笔记'});const name=await control.read('name');if(name?.status!=='known'||name.value!=='新建笔记')throw Error('control name readback failed');state.win=win;print(control);`
	first := call("world-real-find", "exec", code)
	if first.IsError {
		t.Fatalf("real object lookup: %s", first.Content[0].(*mcp.TextContent).Text)
	}
	line := first.Content[0].(*mcp.TextContent).Text
	if !strings.Contains(line, "新建笔记") || !strings.Contains(line, "scrollIntoView") || !strings.Contains(line, "W") || strings.Contains(line, "native_request") {
		t.Fatalf("real model result unusable: %q", line)
	}
	fields := strings.Fields(strings.TrimPrefix(line, "world-real-find · "))
	if len(fields) < 2 {
		t.Fatalf("missing control address: %q", line)
	}
	id := fields[0]
	second := call("world-real-scroll", "exec", `await dtw.at(`+string(mustJSON(id))+`).scrollIntoView();`)
	if second.IsError {
		t.Fatalf("real semantic route: %s", second.Content[0].(*mcp.TextContent).Text)
	}
	line = second.Content[0].(*mcp.TextContent).Text
	if !strings.Contains(line, id+".scrollIntoView: completed, verified via semantic") || strings.Contains(line, "restoration=failed") {
		t.Fatalf("real action not correlated/verified: %q", line)
	}
	captured := call("world-real-capture", "exec", `await state.win.capture();`)
	if captured.IsError || len(captured.Content) != 1 || !strings.Contains(captured.Content[0].(*mcp.TextContent).Text, "capture: 1 image(s) ready") {
		t.Fatalf("on-demand capture summary: %s", captured.Content[0].(*mcp.TextContent).Text)
	}
	imageResult, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "result", "execution_id": "world-real-capture", "include_image": true,
	}})
	if err != nil || imageResult.IsError || imageResult.StructuredContent != nil || len(imageResult.Content) != 2 {
		t.Fatalf("on-demand MCP image unavailable: %v %+v", err, imageResult)
	}
	image, ok := imageResult.Content[1].(*mcp.ImageContent)
	if !ok || image.MIMEType != "image/png" || len(image.Data) < 8 || string(image.Data[:8]) != "\x89PNG\r\n\x1a\n" {
		t.Fatal("capture result is not a PNG MCP image")
	}
	t.Logf("selected real model results: %q; %q; %q; on-demand PNG bytes=%d",
		first.Content[0].(*mcp.TextContent).Text, line,
		captured.Content[0].(*mcp.TextContent).Text, len(image.Data))
}
