package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

var compatiblePluginNode = regexp.MustCompile(`^v24\.[0-9]+\.[0-9]+$`)

// runPluginNode is only a Lite install adapter. The same packaged JavaScript
// MCP server and native helper run in Full and Lite; this does not implement MCP.
func runPluginNode(args []string) error {
	nodePath := os.Getenv("DTW_NODE_PATH")
	if nodePath == "" {
		return fmt.Errorf("DTW_NODE_PATH is required; set it to an absolute compatible Node 24 executable path, or install the Full archive")
	}
	if !filepath.IsAbs(nodePath) {
		return fmt.Errorf("DTW_NODE_PATH must be an absolute executable path, got %q", nodePath)
	}
	info, err := os.Stat(nodePath)
	if err != nil || info.IsDir() {
		return fmt.Errorf("DTW_NODE_PATH does not name a readable Node executable: %q", nodePath)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	version, err := exec.CommandContext(ctx, nodePath, "--version").Output()
	if err != nil {
		return fmt.Errorf("could not check DTW_NODE_PATH version: %w", err)
	}
	got := strings.TrimSpace(string(version))
	if !compatiblePluginNode.MatchString(got) {
		return fmt.Errorf("incompatible Node %q at DTW_NODE_PATH; Desktop World Lite requires Node 24.x, or install the Full archive", got)
	}
	helper, err := os.Executable()
	if err != nil {
		return err
	}
	server := filepath.Join(filepath.Dir(helper), "..", "mcp", "server.mjs")
	if info, err := os.Stat(server); err != nil || info.IsDir() {
		return fmt.Errorf("packaged MCP server missing next to the Lite helper")
	}
	cmd := exec.Command(nodePath, append([]string{server}, args...)...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("packaged MCP server exited: %w", err)
	}
	return nil
}
