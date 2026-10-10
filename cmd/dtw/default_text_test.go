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

func TestStandalonePOCDefaultsToOneTextResult(t *testing.T) {
	if os.Getenv("DTW_TEST_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_TEST_CHILD=1", "DTW_TEST_LEGACY_OUTPUT=0", "DTW_TEST_TEXT_OUTPUT=0", "DTW_TEST_HELPER=", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "default-text", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	listed, err := session.ListTools(ctx, nil)
	if err != nil || len(listed.Tools) != 1 || listed.Tools[0].Name != "exec" {
		t.Fatalf("single exec tool: %v %+v", err, listed)
	}
	result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "default-text", "code": `print('W1/B1 Submit · button · invoke')`,
	}})
	if err != nil || result.IsError || result.StructuredContent != nil || len(result.Content) != 1 {
		t.Fatalf("default result: %v %+v", err, result)
	}
	line := result.Content[0].(*mcp.TextContent).Text
	if line != "default-text · W1/B1 Submit · button · invoke" || strings.Contains(line, "execution_id") {
		t.Fatalf("default model result: %q", line)
	}
}
