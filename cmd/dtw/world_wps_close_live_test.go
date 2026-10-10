package main

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Closes only the exact temporary WPS document after its saved marker was
// independently read back. It does not quit WPS or touch other documents.
func TestSelectedWPSTemporaryDocumentClose(t *testing.T) {
	if os.Getenv("DTW_TEST_CHILD") == "1" {
		return
	}
	title := "dtw-wps-cua-poc-20261010.rtf"
	path := "/private/tmp/" + title
	if os.Getenv("DTW_TEST_WPS_CLOSE_ONCE") != "1" || os.Getenv("DTW_TEST_HELPER") == "" {
		t.Skip("requires opt-in closure of the exact temporary WPS document")
	}
	contents, err := os.ReadFile(path)
	if err != nil || !bytes.Contains(contents, []byte("DTW-WPS-INPUT")) {
		t.Fatalf("saved temporary document not verified before close: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_TEST_CHILD=1", "DTW_TEST_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-wps-temp-close", Version: "1"}, nil)
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
		if err != nil || res == nil || res.IsError || len(res.Content) != 1 || res.StructuredContent != nil {
			if res != nil && len(res.Content) == 1 {
				t.Fatalf("%s result: %v %s", id, err, res.Content[0].(*mcp.TextContent).Text)
			}
			t.Fatalf("%s result: %v %+v", id, err, res)
		}
		line := res.Content[0].(*mcp.TextContent).Text
		t.Logf("%s: %s", id, line)
		return line
	}
	setup := `const app=await dtw.app('WPS Office');const windows=await app.windows();
const exact=windows.filter(w=>w.name==='dtw-wps-cua-poc-20261010.rtf');
if(exact.length===0)print('exact temporary document window already absent');
else if(exact.length===1){state.win=exact[0];print(state.win);}
else throw Error('multiple exact temporary document windows');`
	if line := call("wps-close-setup", setup); strings.Contains(line, "already absent") {
		t.Log("fresh App observation confirms the temporary document was already closed; no input sent")
		return
	} else if !strings.Contains(line, title) {
		t.Fatal("temporary document window unavailable")
	}
	line := call("wps-close-shortcut", `await state.win.press('W',['primary']);`)
	if !strings.Contains(line, ".press:") {
		t.Fatalf("close shortcut detached from exact window: %s", line)
	}
	deadline := time.Now().Add(2 * time.Second)
	for attempt := 1; time.Now().Before(deadline); attempt++ {
		line = call(fmt.Sprintf("wps-close-readback-%d", attempt), `const app=await dtw.app('WPS Office');print((await app.windows()).map(w=>w.name).join(' | '));`)
		if !strings.Contains(line, title) {
			t.Log("exact temporary document window closed; WPS application retained")
			return
		}
		time.Sleep(150 * time.Millisecond)
	}
	t.Fatal("temporary document window remained after one close shortcut; no replay")
}
