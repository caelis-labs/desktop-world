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

// Opt-in, read-only DTW capture of the one temporary RTF opened in WPS.
func TestSelectedWPSTemporaryDocumentCapture(t *testing.T) {
	if os.Getenv("DTW_TEST_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_TEST_WPS_TITLE")
	output := os.Getenv("DTW_TEST_WPS_CAPTURE")
	if title == "" || output == "" || os.Getenv("DTW_TEST_HELPER") == "" {
		t.Skip("requires selected temporary WPS title, private output path and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_TEST_CHILD=1", "DTW_TEST_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-wps-temp-capture", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const app=await dtw.app('WPS Office');const win=await app.window(` + string(mustJSON(title)) + `);state.win=win;print(win);await win.capture();`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "wps-temp-capture", "code": code,
	}})
	if err != nil || res == nil || res.IsError || len(res.Content) != 1 || res.StructuredContent != nil {
		t.Fatalf("WPS capture setup: %v %+v", err, res)
	}
	line := res.Content[0].(*mcp.TextContent).Text
	if !strings.Contains(line, title) || !strings.Contains(line, "capture: 1 image(s) ready") {
		t.Fatalf("WPS capture result not tied to exact test window: %s", line)
	}
	imageResult, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "result", "execution_id": "wps-temp-capture", "include_image": true,
	}})
	if err != nil || imageResult == nil || imageResult.IsError || len(imageResult.Content) != 2 {
		t.Fatalf("WPS test window image unavailable: %v %+v", err, imageResult)
	}
	img, ok := imageResult.Content[1].(*mcp.ImageContent)
	if !ok || img.MIMEType != "image/png" || len(img.Data) < 8 || string(img.Data[:8]) != "\x89PNG\r\n\x1a\n" {
		t.Fatal("WPS capture is not a PNG MCP image")
	}
	if err := os.WriteFile(output, img.Data, 0600); err != nil {
		t.Fatal(err)
	}
	t.Logf("%s; private PNG bytes=%d", line, len(img.Data))
}
