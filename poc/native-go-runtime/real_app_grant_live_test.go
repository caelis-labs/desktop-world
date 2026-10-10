package main

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in exact-window grant status for one explicitly selected disposable app.
// It sends no input and does not treat a pending grant as authorization.
func TestSelectedRealAppExactWindowGrantStatus(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	if title == "" || os.Getenv("DTW_POC_HELPER") == "" || os.Getenv("DTW_POC_WRITE_WINDOW") != title {
		t.Skip("set exact selected real window title as DTW_POC_WRITE_WINDOW and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-selected-real-grant", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const title=` + string(mustJSON(title)) + `;
const status=await dtw.grants();
const selected=(status.grants??[]).filter(g=>g.window_title===title);
print(JSON.stringify({matches:selected.length,states:selected.map(g=>g.state),reasons:selected.map(g=>g.reason),
application_bound:selected.map(g=>!!g.application)}));`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "selected-real-grant-status", "code": code,
	}})
	if err != nil || res.IsError {
		t.Fatalf("selected real grant status failed: %v, %+v", err, res)
	}
	lines := res.StructuredContent.(map[string]any)["print"].([]any)
	if len(lines) != 1 {
		t.Fatalf("expected one grant summary, got %d", len(lines))
	}
	var facts map[string]any
	if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
		t.Fatal(err)
	}
	t.Logf("selected real exact-window grant: %+v", facts)
	if facts["matches"] != float64(1) {
		t.Fatal("selected real title declaration is not uniquely represented")
	}
}
