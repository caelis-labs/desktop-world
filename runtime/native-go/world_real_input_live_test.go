package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in, one-shot physical input against the user's disposable Obsidian vault.
// The new note is reconciled by its exact file before cleanup; an uncertain
// delivery is never repeated by this test.
func TestSelectedObsidianVirtualClickCreatesOneNote(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	vault := os.Getenv("DTW_POC_TEST_VAULT")
	if os.Getenv("DTW_POC_REAL_INPUT_ONCE") != "1" || title == "" || vault == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires explicitly selected disposable Obsidian window and vault")
	}
	if !strings.Contains(title, "DevNote - Obsidian") || filepath.Base(vault) != "DevNote" {
		t.Fatal("selected title/vault do not identify the authorized disposable Obsidian vault")
	}
	notePath := filepath.Join(vault, "未命名.md")
	if _, err := os.Stat(notePath); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("expected exact test note path to be absent before input: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-real-virtual-click", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) string {
		t.Helper()
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
		if err != nil || res == nil || len(res.Content) != 1 || res.StructuredContent != nil {
			t.Fatalf("%s result: %v %+v", id, err, res)
		}
		line := res.Content[0].(*mcp.TextContent).Text
		t.Logf("%s: %s", id, line)
		if res.IsError {
			t.Fatalf("%s failed; inspect original execution ID before any new input", id)
		}
		return line
	}
	setup := `const app=await dtw.app('Obsidian');const win=await app.window(` + string(mustJSON(title)) + `);state.win=win;state.newNote=await win.one({name:'新建笔记'});print(state.newNote);`
	if line := call("real-virtual-setup", setup); !strings.Contains(line, "新建笔记") {
		t.Fatal("exact new note control missing")
	}
	started := time.Now()
	line := call("real-virtual-new-note-click", `await state.newNote.click();`)
	if !strings.Contains(line, ".click:") {
		t.Fatalf("click output cannot be paired with script target: %s", line)
	}
	original, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "result", "execution_id": "real-virtual-new-note-click", "detail": "full",
	}})
	if err != nil || original == nil || len(original.Content) != 1 {
		t.Fatalf("original click receipt unavailable: %v %+v", err, original)
	}
	full := original.Content[0].(*mcp.TextContent).Text
	if !strings.Contains(full, `"channel": "targeted_foreground"`) {
		t.Fatalf("click did not take the selected foreground route: %s", full)
	}
	deadline := time.Now().Add(2 * time.Second)
	var created os.FileInfo
	for time.Now().Before(deadline) {
		created, err = os.Stat(notePath)
		if err == nil {
			break
		}
		if !errors.Is(err, os.ErrNotExist) {
			t.Fatal(err)
		}
		time.Sleep(40 * time.Millisecond)
	}
	if created == nil {
		t.Fatalf("foreground click was dispatched but the selected vault has no new note; no replay. receipt=%s", full)
	}
	if !created.Mode().IsRegular() || created.ModTime().Before(started.Add(-time.Second)) {
		t.Fatalf("new note identity/freshness not proven: mode=%s mtime=%s", created.Mode(), created.ModTime())
	}
	expected := ""
	markerPath := ""
	defer func() {
		for _, path := range []string{notePath, markerPath} {
			if path == "" {
				continue
			}
			current, statErr := os.Stat(path)
			if errors.Is(statErr, os.ErrNotExist) {
				continue
			}
			if statErr != nil || !os.SameFile(created, current) {
				t.Logf("exact test note identity changed; leave cleanup for review: %v", statErr)
				continue
			}
			contents, readErr := os.ReadFile(path)
			if readErr != nil || (len(contents) != 0 && strings.TrimSpace(string(contents)) != expected) {
				t.Logf("test note content differs from the expected marker; leave cleanup for review: read_error=%v bytes=%d", readErr, len(contents))
				continue
			}
			if removeErr := os.Remove(path); removeErr != nil {
				t.Errorf("remove exact test note: %v", removeErr)
				continue
			}
			t.Log("removed only the exact test note created by this click")
		}
	}()
	t.Logf("independent effect: exact new note exists, size=%d, mtime=%s", created.Size(), created.ModTime().Format(time.RFC3339Nano))
	call("real-virtual-post-window", `const app=await dtw.app('Obsidian');print((await app.windows()).map(w=>String(w)).join(' | '));`)
	if os.Getenv("DTW_POC_REAL_INPUT_CASE") == "editor_capture" {
		output := os.Getenv("DTW_POC_REAL_CAPTURE")
		if output == "" {
			t.Fatal("private empty-note capture path required")
		}
		call("real-virtual-empty-note-capture", `const app=await dtw.app('Obsidian');const windows=await app.windows();
const own=windows.filter(w=>w.name.startsWith('未命名 - DevNote - Obsidian'));
if(own.length!==1)throw Error('fresh note window unresolved');await own[0].capture();`)
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "result", "execution_id": "real-virtual-empty-note-capture", "include_image": true,
		}})
		if err != nil || res == nil || res.IsError || len(res.Content) != 2 {
			t.Fatalf("empty note image unavailable: %v %+v", err, res)
		}
		img, ok := res.Content[1].(*mcp.ImageContent)
		if !ok || img.MIMEType != "image/png" || len(img.Data) < 8 || string(img.Data[:8]) != "\x89PNG\r\n\x1a\n" {
			t.Fatal("empty note capture is not a PNG MCP image")
		}
		if err := os.WriteFile(output, img.Data, 0600); err != nil {
			t.Fatal(err)
		}
		t.Logf("private empty-note screenshot bytes=%d", len(img.Data))
	}
	if os.Getenv("DTW_POC_REAL_INPUT_CASE") == "editor_probe" {
		call("real-virtual-editor-probe", `const app=await dtw.app('Obsidian');const windows=await app.windows();
const own=windows.filter(w=>w.name.startsWith('未命名 - DevNote - Obsidian'));
if(own.length!==1)throw Error('fresh note window unresolved');
const rows=[];for(const role of ['text_area','text_field','document']){
 const ob=await dtw.observe({scope:{ids:[own[0].id]},projection:'outline',
  fields:['name','role','capabilities'],match:{within_id:own[0].id,role},
  budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
 rows.push({role,complete:ob.coverage?.complete,truncated:ob.coverage?.truncated,
  count:ob.objects?.length,items:(ob.objects??[]).slice(0,5).map(o=>({role:o.role,
   name:o.name?.status==='known'?o.name.value.slice(0,70):'',
   actions:(o.capabilities??[]).filter(c=>c.support==='supported'&&c.availability==='available').map(c=>c.name)}))});}
print(JSON.stringify(rows));`)
	}
	if os.Getenv("DTW_POC_REAL_INPUT_CASE") == "keyboard" {
		expected = "DTW-POC-VIRTUAL-中文🙂"
		markerPath = filepath.Join(vault, expected+".md")
		if _, err := os.Stat(markerPath); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("test marker note already exists before input: %v", err)
		}
		line = call("real-virtual-type-text", `const app=await dtw.app('Obsidian');const windows=await app.windows();
const own=windows.filter(w=>w.name.startsWith('未命名 - DevNote - Obsidian'));
if(own.length!==1)throw Error('fresh note window unresolved');await own[0].typeText('DTW-POC-VIRTUAL-中文🙂');`)
		if !strings.Contains(line, ".typeText:") {
			t.Fatalf("keyboard output cannot be paired with the note window: %s", line)
		}
		deadline = time.Now().Add(3 * time.Second)
		for time.Now().Before(deadline) {
			contents, readErr := os.ReadFile(notePath)
			if readErr != nil && !errors.Is(readErr, os.ErrNotExist) {
				t.Fatal(readErr)
			}
			if string(contents) == expected {
				t.Log("independent keyboard effect: exact Unicode marker stored in the test note")
				return
			}
			if renamed, statErr := os.Stat(markerPath); statErr == nil && os.SameFile(created, renamed) && renamed.Size() == 0 {
				t.Log("independent keyboard effect: exact Unicode marker became the test note title; editor focus remains unproved")
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatal("keyboard dispatch produced neither exact test content nor exact test title; no replay")
	}
	if os.Getenv("DTW_POC_REAL_INPUT_CASE") == "editor_body" {
		expected = "DTW-POC-BODY-中文🙂"
		markerPath = filepath.Join(vault, expected+".md")
		if _, err := os.Stat(markerPath); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("test marker note already exists before body input: %v", err)
		}
		line = call("real-virtual-editor-body", `const app=await dtw.app('Obsidian');const windows=await app.windows();
const own=windows.filter(w=>w.name.startsWith('未命名 - DevNote - Obsidian'));
if(own.length!==1)throw Error('fresh note window unresolved');
await dtw.transaction(tx=>{tx.click(own[0],{u:0.43,v:0.23});tx.typeText(own[0],'DTW-POC-BODY-中文🙂');});`)
		if !strings.Contains(line, ".click#1") || !strings.Contains(line, ".typeText#2") {
			t.Fatalf("body action result detached from script steps: %s", line)
		}
		deadline = time.Now().Add(3 * time.Second)
		for time.Now().Before(deadline) {
			contents, readErr := os.ReadFile(notePath)
			if readErr != nil && !errors.Is(readErr, os.ErrNotExist) {
				t.Fatal(readErr)
			}
			if strings.TrimSpace(string(contents)) == expected {
				t.Logf("independent Obsidian body readback: exact Unicode marker in the created note, bytes=%d", len(contents))
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatal("body input did not appear in the exact created note; no replay")
	}
}
