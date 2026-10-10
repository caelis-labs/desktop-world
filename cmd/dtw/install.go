package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
)

const installHelp = `usage: dtw install AGENT [--scope user|project] [--project DIR] [--to DIR]

AGENT: plugin, codex, claude, antigravity, gemini, cursor
The plugin target requires --to. Other targets default to user scope.
Project scope uses the current directory unless --project is given.`

type installOptions struct {
	agent, scope, project, to string
	root, home                string
	claude                    func(marketplace, scope, project string) error
}

func runInstall(args []string) error {
	if len(args) == 0 || args[0] == "--help" || args[0] == "-h" {
		fmt.Println(installHelp)
		return nil
	}
	fs := flag.NewFlagSet("install", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	scope := fs.String("scope", "user", "user or project")
	project := fs.String("project", "", "project directory")
	to := fs.String("to", "", "portable plugin destination")
	if err := fs.Parse(args[1:]); err != nil || fs.NArg() != 0 {
		return fmt.Errorf("%s", installHelp)
	}
	if *scope != "user" && *scope != "project" {
		return errors.New("scope must be user or project")
	}
	self, err := os.Executable()
	if err != nil {
		return err
	}
	self, err = filepath.EvalSymlinks(self)
	if err != nil {
		return err
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return err
	}
	working, err := os.Getwd()
	if err != nil {
		return err
	}
	if *project != "" {
		working, err = filepath.Abs(*project)
		if err != nil {
			return err
		}
	}
	opts := installOptions{agent: args[0], scope: *scope, project: working, to: *to,
		root: filepath.Dir(filepath.Dir(self)), home: home, claude: installClaudeCLI}
	if err := installAgent(opts); err != nil {
		return err
	}
	if opts.agent == "plugin" {
		fmt.Printf("dtw portable plugin copied to %s\n", opts.to)
		return nil
	}
	fmt.Printf("dtw installed for %s (%s)\n", opts.agent, opts.scope)
	return nil
}

func installAgent(o installOptions) error {
	if o.agent != "plugin" && o.agent != "codex" && o.agent != "claude" &&
		o.agent != "antigravity" && o.agent != "gemini" && o.agent != "cursor" {
		return fmt.Errorf("unknown agent %q\n%s", o.agent, installHelp)
	}
	if o.scope != "user" && o.scope != "project" {
		return errors.New("scope must be user or project")
	}
	if o.agent != "plugin" && o.to != "" {
		return errors.New("--to is only for dtw install plugin")
	}
	if err := checkPackage(o.root); err != nil {
		return err
	}
	base := o.home
	if o.scope == "project" {
		base = o.project
	}
	switch o.agent {
	case "plugin":
		if o.to == "" {
			return errors.New("dtw install plugin requires --to DIR")
		}
		return copyPackage(o.root, o.to, "portable")
	case "codex":
		plugin := filepath.Join(base, "plugins", "desktop-world")
		if o.scope == "user" {
			plugin = filepath.Join(base, ".codex", "plugins", "desktop-world")
		}
		if err := copyPackage(o.root, plugin, "portable"); err != nil {
			return err
		}
		market := filepath.Join(base, ".agents", "plugins", "marketplace.json")
		if _, err := addCodexMarketplace(market, base, plugin); err != nil {
			return err
		}
		if err := configureCodexMCP(filepath.Join(base, ".codex", "config.toml"),
			filepath.Join(plugin, "bin", executableName("dtw"))); err != nil {
			return err
		}
		return copyFile(filepath.Join(o.root, "skills", "desktop-world", "SKILL.md"),
			filepath.Join(base, ".agents", "skills", "desktop-world", "SKILL.md"))
	case "claude":
		marketRoot := filepath.Join(o.home, ".local", "share", "dtw", "claude-marketplace")
		if err := copyPackage(o.root, filepath.Join(marketRoot, "plugins", "desktop-world"), "portable"); err != nil {
			return err
		}
		if err := writeJSON(filepath.Join(marketRoot, ".claude-plugin", "marketplace.json"), map[string]any{
			"name": "dtw-local", "owner": map[string]any{"name": "Caelis Labs"},
			"plugins": []any{map[string]any{"name": "desktop-world", "source": "./plugins/desktop-world"}},
		}); err != nil {
			return err
		}
		if o.claude == nil {
			return errors.New("Claude Code CLI is needed to enable the plugin")
		}
		return o.claude(marketRoot, o.scope, o.project)
	case "antigravity":
		plugin := filepath.Join(base, ".agents", "plugins", "dtw-antigravity")
		if o.scope == "user" {
			plugin = filepath.Join(base, ".gemini", "config", "plugins", "desktop-world")
		}
		if err := copyPackage(o.root, plugin, "antigravity"); err != nil {
			return err
		}
		if err := writeJSON(filepath.Join(plugin, "plugin.json"), map[string]any{
			"$schema": "https://antigravity.google/schemas/v1/plugin.json",
			"name":    "desktop-world", "description": "Native desktop actions with DTW",
		}); err != nil {
			return err
		}
		return writeJSON(filepath.Join(plugin, "mcp_config.json"), serverMap(filepath.Join(plugin, "bin", executableName("dtw"))))
	case "gemini":
		if o.scope != "user" {
			return errors.New("Gemini CLI extensions are installed at user scope; use --scope user")
		}
		plugin := filepath.Join(base, ".gemini", "extensions", "desktop-world")
		if err := copyPackage(o.root, plugin, "gemini"); err != nil {
			return err
		}
		return writeJSON(filepath.Join(plugin, "gemini-extension.json"), map[string]any{
			"name": "desktop-world", "version": strings.TrimPrefix(releaseVersion, "v"),
			"description": "Native desktop actions with DTW",
			"mcpServers": map[string]any{"dtw": map[string]any{
				"command": "${extensionPath}/bin/" + executableName("dtw"),
				"cwd":     "${extensionPath}",
			}},
		})
	case "cursor":
		plugin := filepath.Join(base, ".cursor", "plugins", "desktop-world")
		if err := copyPackage(o.root, plugin, "portable"); err != nil {
			return err
		}
		if err := mergeMCP(filepath.Join(base, ".cursor", "mcp.json"), filepath.Join(plugin, "bin", executableName("dtw"))); err != nil {
			return err
		}
		return copyFile(filepath.Join(o.root, "skills", "desktop-world", "SKILL.md"),
			filepath.Join(base, ".cursor", "skills", "desktop-world", "SKILL.md"))
	}
	return nil
}

func executableName(name string) string {
	if runtime.GOOS == "windows" {
		return name + ".exe"
	}
	return name
}

func checkPackage(root string) error {
	for _, name := range []string{"plugin.json", "mcp.json", ".mcp.json", filepath.Join(".claude-plugin", "plugin.json"),
		filepath.Join("bin", executableName("dtw")), filepath.Join("libexec", executableName("dtw-helper")),
		filepath.Join("skills", "desktop-world", "SKILL.md"), "LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.md"} {
		if _, err := os.Stat(filepath.Join(root, name)); err != nil {
			return fmt.Errorf("install needs the built DTW package; missing %s: %w", name, err)
		}
	}
	return nil
}

func copyPackage(src, dst, flavor string) error {
	if filepath.Clean(src) == filepath.Clean(dst) {
		return nil
	}
	marker := "plugin.json"
	if flavor == "gemini" {
		marker = "gemini-extension.json"
	}
	if info, err := os.Stat(dst); err == nil {
		if !info.IsDir() {
			return fmt.Errorf("install target is not a directory: %s", dst)
		}
		if data, readErr := os.ReadFile(filepath.Join(dst, marker)); readErr == nil {
			var manifest struct {
				Name string `json:"name"`
			}
			if json.Unmarshal(data, &manifest) != nil || manifest.Name != "desktop-world" {
				return fmt.Errorf("install target belongs to another plugin: %s", dst)
			}
		} else if entries, listErr := os.ReadDir(dst); listErr == nil && len(entries) != 0 {
			return fmt.Errorf("install target is nonempty and not a DTW plugin: %s", dst)
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	files := []string{filepath.Join("bin", executableName("dtw")),
		filepath.Join("libexec", executableName("dtw-helper")),
		filepath.Join("skills", "desktop-world", "SKILL.md"), "LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.md"}
	if flavor == "portable" {
		files = append(files, "plugin.json", "mcp.json", ".mcp.json", filepath.Join(".claude-plugin", "plugin.json"))
	}
	for _, name := range files {
		if err := copyFile(filepath.Join(src, name), filepath.Join(dst, name)); err != nil {
			return err
		}
	}
	return nil
}

func copyFile(src, dst string) error {
	data, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	info, err := os.Stat(src)
	if err != nil {
		return err
	}
	return writeAtomic(dst, data, info.Mode().Perm())
}

func writeAtomic(path string, data []byte, mode os.FileMode) error {
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(path), ".dtw-install-*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if _, err = f.Write(data); err != nil {
		f.Close()
		return err
	}
	if err = f.Chmod(mode); err != nil {
		f.Close()
		return err
	}
	if err = f.Close(); err != nil {
		return err
	}
	return os.Rename(f.Name(), path)
}

func writeJSON(path string, value any) error {
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	return writeAtomic(path, append(data, '\n'), existingMode(path, 0644))
}

func existingMode(path string, fallback os.FileMode) os.FileMode {
	if info, err := os.Stat(path); err == nil {
		return info.Mode().Perm()
	}
	return fallback
}

func readObject(path string) (map[string]any, error) {
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return make(map[string]any), nil
	}
	if err != nil {
		return nil, err
	}
	var object map[string]any
	if err := json.Unmarshal(data, &object); err != nil || object == nil {
		return nil, fmt.Errorf("invalid JSON in %s", path)
	}
	return object, nil
}

func serverMap(binary string) map[string]any {
	return map[string]any{"mcpServers": map[string]any{"dtw": map[string]any{"command": binary}}}
}

func mergeMCP(path, binary string) error {
	object, err := readObject(path)
	if err != nil {
		return err
	}
	servers, ok := object["mcpServers"].(map[string]any)
	if object["mcpServers"] != nil && !ok {
		return fmt.Errorf("mcpServers is not an object in %s", path)
	}
	if servers == nil {
		servers = make(map[string]any)
	}
	if old, exists := servers["dtw"]; exists {
		entry, _ := old.(map[string]any)
		if entry["command"] != binary {
			return fmt.Errorf("existing dtw MCP server differs in %s", path)
		}
		return nil
	}
	servers["dtw"] = map[string]any{"command": binary}
	object["mcpServers"] = servers
	return writeJSON(path, object)
}

func addCodexMarketplace(path, base, plugin string) (string, error) {
	object, err := readObject(path)
	if err != nil {
		return "", err
	}
	name, _ := object["name"].(string)
	if name == "" {
		name = "dtw-local"
		object["name"] = name
	}
	rel, err := filepath.Rel(base, plugin)
	if err != nil || strings.HasPrefix(rel, "..") {
		return "", errors.New("plugin must stay within the install scope")
	}
	pathRef := "./" + filepath.ToSlash(rel)
	plugins, ok := object["plugins"].([]any)
	if object["plugins"] != nil && !ok {
		return "", fmt.Errorf("plugins is not an array in %s", path)
	}
	entry := map[string]any{"name": "desktop-world", "source": map[string]any{
		"source": "local", "path": pathRef},
		"policy":   map[string]any{"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
		"category": "Productivity"}
	found := false
	for index, value := range plugins {
		item, _ := value.(map[string]any)
		if item["name"] == "desktop-world" {
			source, _ := item["source"].(map[string]any)
			if source["path"] != pathRef {
				return "", fmt.Errorf("existing desktop-world entry differs in %s", path)
			}
			plugins[index] = entry
			found = true
		}
	}
	if !found {
		plugins = append(plugins, entry)
	}
	object["plugins"] = plugins
	return name, writeJSON(path, object)
}

func configureCodexMCP(path, binary string) error {
	data, err := os.ReadFile(path)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	heading := `[mcp_servers.dtw]`
	if index := strings.Index(string(data), heading); index >= 0 {
		block := string(data[index+len(heading):])
		if next := strings.Index(block, "\n["); next >= 0 {
			block = block[:next]
		}
		if strings.Contains(block, "command = "+strconv.Quote(binary)) {
			return nil
		}
		return fmt.Errorf("existing dtw MCP server differs in %s", path)
	}
	if len(data) > 0 && data[len(data)-1] != '\n' {
		data = append(data, '\n')
	}
	data = append(data, []byte("\n"+heading+"\ncommand = "+strconv.Quote(binary)+"\n")...)
	return writeAtomic(path, data, existingMode(path, 0644))
}

func installClaudeCLI(marketplace, scope, project string) error {
	if _, err := exec.LookPath("claude"); err != nil {
		return fmt.Errorf("Claude Code CLI unavailable: %w", err)
	}
	command := exec.Command("claude", "plugin", "install", "desktop-world", "--marketplace", marketplace, "--scope", scope)
	command.Dir = project
	command.Stdout, command.Stderr = os.Stdout, os.Stderr
	if err := command.Run(); err != nil {
		return fmt.Errorf("Claude plugin installation failed: %w", err)
	}
	return nil
}
