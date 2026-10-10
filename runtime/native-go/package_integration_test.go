package main

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestPackagedNativeMCPNoNode(t *testing.T) {
	binary := os.Getenv("DTW_PACKAGED_DTW")
	if binary == "" {
		t.Skip("set DTW_PACKAGED_DTW to the local native package binary")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	command := exec.Command(binary)
	command.Env = append(os.Environ(), "PATH=/usr/bin:/bin", "DTW_POC_HELPER=/nonexistent", "DTW_POC_VIRTUAL=0", "DTW_POC_LEGACY_OUTPUT=0")
	client := mcp.NewClient(&mcp.Implementation{Name: "native-package-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: command}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	listed, err := session.ListTools(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(listed.Tools) != 1 || listed.Tools[0].Name != "exec" {
		t.Fatalf("tools: %+v", listed.Tools)
	}
	for _, call := range []struct{ id, code, want string }{
		{"package-one", `state.packageValue = 7; print('native ready')`, "native ready"},
		{"package-two", `print(state.packageValue + 1)`, "8"},
	} {
		result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": call.id, "code": call.code,
		}})
		if err != nil {
			t.Fatal(err)
		}
		if result.IsError || len(result.Content) != 1 {
			t.Fatalf("%s: %+v", call.id, result)
		}
		line := result.Content[0].(*mcp.TextContent).Text
		if !strings.Contains(line, call.id+" · "+call.want) {
			t.Fatalf("%s: %q", call.id, line)
		}
		if result.StructuredContent != nil {
			t.Fatalf("%s: duplicated structured output: %#v", call.id, result.StructuredContent)
		}
	}
}

func TestPackagedTerminalGuardOwnedApp(t *testing.T) {
	binary := os.Getenv("DTW_PACKAGED_DTW")
	app := os.Getenv("DTW_TERMINAL_FIXTURE_APP")
	title := os.Getenv("DTW_TERMINAL_FIXTURE_TITLE")
	if binary == "" || app == "" || title == "" {
		t.Skip("set packaged binary and owned terminal fixture identity")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 18*time.Second)
	defer cancel()
	command := exec.Command(binary)
	command.Env = append(os.Environ(), "PATH=/usr/bin:/bin", "DTW_POC_LEGACY_OUTPUT=0", "DTW_POC_VIRTUAL=0")
	client := mcp.NewClient(&mcp.Implementation{Name: "terminal-guard-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: command}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := "const app = await dtw.app(" + strconv.Quote(app) + ", {window:" + strconv.Quote(title) + "}); " +
		"const win = await app.window(" + strconv.Quote(title) + "); " +
		"const receipt = await dtw.act({steps:[{id:'blocked.focus',op:'focus',target:{id:win.id}}]}); " +
		"print(receipt.steps[0].fault?.code ?? receipt.steps[0].state)"
	result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "terminal-guard", "code": code,
	}})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Content) != 1 {
		t.Fatalf("content: %+v", result)
	}
	text := result.Content[0].(*mcp.TextContent).Text
	t.Logf("pre-dispatch terminal result: %s", text)
	if !strings.Contains(text, "terminal_application_blocked") || strings.Contains(text, "dispatched") {
		t.Fatalf("terminal guard was not a proven pre-dispatch block: %s", text)
	}
}

func TestPackagedReadOnlyObjectOutput(t *testing.T) {
	binary, app := os.Getenv("DTW_PACKAGED_DTW"), os.Getenv("DTW_PACKAGED_READ_APP")
	if binary == "" || app == "" {
		t.Skip("set packaged binary and a permitted read-only app")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	command := exec.Command(binary)
	command.Env = append(os.Environ(), "PATH=/usr/bin:/bin", "DTW_POC_LEGACY_OUTPUT=0", "DTW_POC_VIRTUAL=0")
	client := mcp.NewClient(&mcp.Implementation{Name: "object-output-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: command}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "app-read", "code": "print(await dtw.app(" + strconv.Quote(app) + "))",
	}})
	if err != nil || len(result.Content) != 1 {
		t.Fatalf("object read: %v %+v", err, result)
	}
	line := result.Content[0].(*mcp.TextContent).Text
	t.Logf("real app model result (%d bytes): %s", len(line), line)
	if result.IsError || !strings.Contains(line, app) || strings.Contains(line, "observation incomplete") ||
		strings.Contains(line, "native_request") || result.StructuredContent != nil {
		t.Fatalf("object result contains internal discovery noise: %s", line)
	}
	full, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "result", "execution_id": "app-read", "detail": "full",
	}})
	if err != nil || len(full.Content) != 1 || !strings.Contains(full.Content[0].(*mcp.TextContent).Text, "native_receipts") {
		t.Fatalf("full original result unavailable: %v %+v", err, full)
	}
}

func TestPackagedOwnedSemanticAction(t *testing.T) {
	binary := os.Getenv("DTW_PACKAGED_DTW")
	app := os.Getenv("DTW_PACKAGED_ACTION_APP")
	title := os.Getenv("DTW_PACKAGED_ACTION_WINDOW")
	logPath := os.Getenv("DTW_PACKAGED_ACTION_LOG")
	if binary == "" || app == "" || title == "" || logPath == "" {
		t.Skip("set packaged binary and owned action fixture")
	}
	before := countFixtureEvent(t, logPath, "submit")
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	command := exec.Command(binary)
	command.Env = append(os.Environ(), "PATH=/usr/bin:/bin", "DTW_POC_LEGACY_OUTPUT=0", "DTW_POC_VIRTUAL=0")
	client := mcp.NewClient(&mcp.Implementation{Name: "package-action-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: command}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := "const app = await dtw.app(" + strconv.Quote(app) + ", {window:" + strconv.Quote(title) + "}); " +
		"const win = await app.window(" + strconv.Quote(title) + "); " +
		"const button = await win.one({name:'POC submit'}); await button.invoke();"
	result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "owned-invoke", "code": code,
	}})
	if err != nil || len(result.Content) != 1 {
		t.Fatalf("action: %v %+v", err, result)
	}
	line := result.Content[0].(*mcp.TextContent).Text
	t.Logf("owned action model result: %s", line)
	if result.IsError || !strings.Contains(line, ".invoke: dispatched; effect unverified") || strings.Contains(line, "observation incomplete") {
		t.Fatalf("action result: %s", line)
	}
	if after := countFixtureEvent(t, logPath, "submit"); after != before+1 {
		t.Fatalf("callback count %d -> %d, want exactly one", before, after)
	} else {
		t.Logf("owned callback readback: %d -> %d", before, after)
	}
}

func countFixtureEvent(t *testing.T, path, event string) int {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, line := range strings.Split(string(data), "\n") {
		var row struct {
			Event string `json:"event"`
		}
		if json.Unmarshal([]byte(line), &row) == nil && row.Event == event {
			count++
		}
	}
	return count
}
