// A dedicated no-business MCP for measuring what a real model adapter forwards.
package main

import (
	"context"
	"os"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

type input struct {
	Operation string `json:"operation" jsonschema:"probe"`
}

func main() {
	mode := os.Getenv("DTW_TOKEN_MODE")
	server := mcp.NewServer(&mcp.Implementation{Name: "dtw-token-probe", Version: "0.0.0-poc"}, nil)
	mcp.AddTool(server, &mcp.Tool{Name: "exec", Description: "Return one fixed synthetic probe result"},
		func(_ context.Context, _ *mcp.CallToolRequest, in input) (*mcp.CallToolResult, map[string]any, error) {
			_ = in
			facts := map[string]any{"status": "ok", "value": 42, "print": "probe=42"}
			if mode == "single" {
				return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: "probe=42; status=ok"}}}, nil, nil
			}
			return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: "probe=42; status=ok"}}}, facts, nil
		})
	if err := server.Run(context.Background(), &mcp.StdioTransport{}); err != nil {
		os.Exit(1)
	}
}
