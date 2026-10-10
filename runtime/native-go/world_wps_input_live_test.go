package main

import (
	"bytes"
	"context"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in input only to the exact temporary RTF created for this POC. The
// screenshot was reviewed before choosing a point inside its document page.
func TestSelectedWPSTemporaryDocumentInput(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_WPS_TITLE")
	path := os.Getenv("DTW_POC_WPS_FILE")
	if os.Getenv("DTW_POC_WPS_INPUT_ONCE") != "1" || title == "" || path == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires opt-in exact temporary WPS document and native helper")
	}
	if title != "dtw-wps-cua-poc-20261010.rtf" || path != "/private/tmp/"+title {
		t.Fatal("WPS input target differs from the reviewed temporary document")
	}
	before, err := os.ReadFile(path)
	if err != nil || !bytes.Contains(before, []byte("DTW WPS POC baseline")) || bytes.Contains(before, []byte("DTW-WPS-INPUT")) {
		t.Fatalf("temporary document precondition failed: read=%v bytes=%d", err, len(before))
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-wps-temp-input", Version: "1"}, nil)
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
			t.Fatalf("%s failed; inspect original ID before any new input", id)
		}
		return line
	}
	setup := `const app=await dtw.app('WPS Office');state.win=await app.window(` + string(mustJSON(title)) + `);print(state.win);`
	if line := call("wps-input-setup", setup); !strings.Contains(line, title) {
		t.Fatal("exact temporary WPS window unavailable")
	}
	line := call("wps-input-sequence", `await dtw.transaction(tx=>{tx.click(state.win,{u:0.47,v:0.33});tx.typeText(state.win,' DTW-WPS-INPUT');tx.press(state.win,'S',['primary']);});`)
	if !strings.Contains(line, ".click#1") || !strings.Contains(line, ".typeText#2") || !strings.Contains(line, ".press#3") {
		t.Fatalf("WPS action result detached from script steps: %s", line)
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		after, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(after, []byte("DTW-WPS-INPUT")) {
			t.Logf("independent WPS file readback contains exact marker; before=%d after=%d bytes", len(before), len(after))
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	t.Fatal("WPS received the input sequence but temporary RTF did not contain the exact marker; no replay")
}
