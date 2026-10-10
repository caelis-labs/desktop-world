package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func fixturePackage(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	for name, content := range map[string]string{
		"plugin.json": `{"name":"desktop-world"}`,
		"mcp.json":    `{"mcpServers":{"dtw":{"type":"stdio","command":"./bin/dtw"}}}`,
		".mcp.json":   `{"mcpServers":{"dtw":{"command":"${CLAUDE_PLUGIN_ROOT}/bin/dtw"}}}`,
		filepath.Join(".claude-plugin", "plugin.json"):         `{"name":"desktop-world"}`,
		filepath.Join("bin", executableName("dtw")):            "dtw binary",
		filepath.Join("libexec", executableName("dtw-helper")): "helper binary",
		filepath.Join("skills", "desktop-world", "SKILL.md"):   "---\nname: desktop-world\ndescription: test\n---\n",
		"LICENSE":                "MPL-2.0",
		"NOTICE":                 "notice",
		"THIRD_PARTY_NOTICES.md": "third party",
	} {
		path := filepath.Join(root, name)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0755); err != nil {
			t.Fatal(err)
		}
	}
	return root
}

func TestInstallPortableAndCodexPreservesMarketplace(t *testing.T) {
	src, home, project := fixturePackage(t), t.TempDir(), t.TempDir()
	opts := installOptions{root: src, home: home, project: project, agent: "plugin", scope: "user", to: filepath.Join(t.TempDir(), "portable")}
	if err := installAgent(opts); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(opts.to, ".claude-plugin", "plugin.json")); err != nil {
		t.Fatal(err)
	}
	market := filepath.Join(project, ".agents", "plugins", "marketplace.json")
	if err := writeJSON(market, map[string]any{"name": "team-local", "plugins": []any{map[string]any{"name": "other", "source": "./plugins/other"}}}); err != nil {
		t.Fatal(err)
	}
	opts.agent, opts.scope, opts.to = "codex", "project", ""
	for range 2 {
		if err := installAgent(opts); err != nil {
			t.Fatal(err)
		}
	}
	data, err := os.ReadFile(market)
	if err != nil {
		t.Fatal(err)
	}
	var value struct {
		Name    string           `json:"name"`
		Plugins []map[string]any `json:"plugins"`
	}
	if err := json.Unmarshal(data, &value); err != nil || value.Name != "team-local" || len(value.Plugins) != 2 {
		t.Fatalf("marketplace changed unrelated entries: %s %v", data, err)
	}
	config, err := os.ReadFile(filepath.Join(project, ".codex", "config.toml"))
	if err != nil || strings.Count(string(config), `[mcp_servers.dtw]`) != 1 {
		t.Fatalf("Codex MCP not configured exactly once: %s %v", config, err)
	}
	if _, err := os.Stat(filepath.Join(project, ".agents", "skills", "desktop-world", "SKILL.md")); err != nil {
		t.Fatal(err)
	}
}

func TestInstallAgentAdapters(t *testing.T) {
	src, home, project := fixturePackage(t), t.TempDir(), t.TempDir()
	for _, agent := range []string{"antigravity", "gemini", "cursor"} {
		opts := installOptions{root: src, home: home, project: project, agent: agent, scope: "user"}
		if err := installAgent(opts); err != nil {
			t.Fatalf("%s: %v", agent, err)
		}
	}
	ag := filepath.Join(home, ".gemini", "config", "plugins", "desktop-world")
	manifest, err := readObject(filepath.Join(ag, "plugin.json"))
	if err != nil || len(manifest) != 3 {
		t.Fatalf("Antigravity manifest must use its closed schema: %#v %v", manifest, err)
	}
	config, err := readObject(filepath.Join(ag, "mcp_config.json"))
	if err != nil {
		t.Fatal(err)
	}
	command := config["mcpServers"].(map[string]any)["dtw"].(map[string]any)["command"]
	if command != filepath.Join(ag, "bin", executableName("dtw")) {
		t.Fatalf("Antigravity command: %v", command)
	}
	if _, err := os.Stat(filepath.Join(home, ".gemini", "extensions", "desktop-world", "gemini-extension.json")); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(home, ".cursor", "skills", "desktop-world", "SKILL.md")); err != nil {
		t.Fatal(err)
	}
	if _, err := readObject(filepath.Join(home, ".cursor", "mcp.json")); err != nil {
		t.Fatal(err)
	}
}

func TestInstallClaudeUsesPluginCLI(t *testing.T) {
	src, home, project := fixturePackage(t), t.TempDir(), t.TempDir()
	called := false
	opts := installOptions{root: src, home: home, project: project, agent: "claude", scope: "project",
		claude: func(marketplace, scope, cwd string) error {
			called = true
			if scope != "project" || cwd != project {
				t.Fatalf("scope/cwd: %s %s", scope, cwd)
			}
			if _, err := os.Stat(filepath.Join(marketplace, ".claude-plugin", "marketplace.json")); err != nil {
				t.Fatal(err)
			}
			if _, err := os.Stat(filepath.Join(marketplace, "plugins", "desktop-world", ".mcp.json")); err != nil {
				t.Fatal(err)
			}
			return nil
		}}
	if err := installAgent(opts); err != nil || !called {
		t.Fatalf("Claude CLI not called: %v", err)
	}
}

func TestInstallMCPConflictDoesNotOverwrite(t *testing.T) {
	path := filepath.Join(t.TempDir(), "mcp.json")
	if err := writeJSON(path, map[string]any{"mcpServers": map[string]any{
		"dtw": map[string]any{"command": "/other/dtw"}, "other": map[string]any{"command": "/other/server"}}}); err != nil {
		t.Fatal(err)
	}
	if err := mergeMCP(path, "/new/dtw"); err == nil {
		t.Fatal("conflicting server should not be replaced")
	}
	data, err := os.ReadFile(path)
	if err != nil || !strings.Contains(string(data), "/other/dtw") {
		t.Fatalf("existing config changed: %s %v", data, err)
	}
}

func TestInstallPreservesPrivateConfigMode(t *testing.T) {
	path := filepath.Join(t.TempDir(), "mcp.json")
	if err := os.WriteFile(path, []byte(`{"mcpServers":{"other":{"command":"/other"}}}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := mergeMCP(path, "/dtw/bin/dtw"); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm() != 0600 {
		t.Fatalf("private config permissions changed: %v %v", info, err)
	}
}
