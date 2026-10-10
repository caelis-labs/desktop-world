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

// Read-only, opt-in locator for the one note created by this POC. It reports
// only metadata for that exact test note and exact delete controls.
func TestSelectedRealAppCleanupLocator(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	appName := os.Getenv("DTW_POC_REAL_APP_NAME")
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	note := os.Getenv("DTW_POC_REAL_TEST_NOTE")
	if appName == "" || title == "" || note == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set selected disposable app, exact window and test note")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-selected-cleanup-locator", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const appName=` + string(mustJSON(appName)) + `,title=` + string(mustJSON(title)) + `,note=` + string(mustJSON(note)) + `;
const all=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:1024,max_visited_nodes:10000,read_deadline_ms:4000}});
const apps=all.objects.filter(o=>o.kind==='application'&&o.name?.status==='known'&&o.name.value===appName);
if(apps.length!==1)throw Error('selected app unresolved');
const own=await dtw.observe({scope:{refs:[apps[0].ref]},projection:'outline',fields:['name','role','app'],
match:{within:apps[0].ref,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:4000}});
const wins=(own.objects??[]).filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(own.coverage?.complete!==true||wins.length!==1)throw Error('exact test window unresolved');
const probe=async(name)=>{const ob=await dtw.observe({scope:{refs:[wins[0].ref]},projection:'outline',
fields:['name','role','bounds','capabilities'],match:{within:wins[0].ref,name_equals:name},
budget:{max_results:16,max_visited_nodes:1200,max_output_bytes:32768,max_depth:14,read_deadline_ms:5000}});
const hits=(ob.objects??[]).filter(o=>o.name?.status==='known'&&o.name.value===name);
return {matches:hits.length,complete:ob.coverage?.complete,dirty:ob.coverage?.dirty,truncated:ob.coverage?.truncated,
items:hits.slice(0,10).map(o=>({kind:o.kind,role:o.role,bounds:o.bounds?.value?.rect??null,
capabilities:(o.capabilities??[]).filter(c=>c.support==='supported').map(c=>c.name)}))};};
const noteHits=await probe(note);const deleteHits=await probe('删除文件');
const appDelete=await dtw.observe({scope:{refs:[apps[0].ref]},projection:'outline',fields:['name','role','capabilities'],
match:{within:apps[0].ref,name_equals:'删除文件'},
budget:{max_results:16,max_visited_nodes:1200,max_output_bytes:32768,max_depth:14,read_deadline_ms:5000}});
const appHits=(appDelete.objects??[]).filter(o=>o.name?.status==='known'&&o.name.value==='删除文件');
print(JSON.stringify({window_matches:wins.length,note:noteHits,delete_control:deleteHits,
app_delete:{matches:appHits.length,complete:appDelete.coverage?.complete,dirty:appDelete.coverage?.dirty,
roles:appHits.map(o=>o.role),capabilities:appHits.map(o=>(o.capabilities??[]).filter(c=>c.support==='supported').map(c=>c.name))}}));`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "selected-real-cleanup-locator", "code": code,
	}})
	if err != nil || res.IsError {
		t.Fatalf("cleanup locator failed: %v, %+v", err, res)
	}
	lines := res.StructuredContent.(map[string]any)["print"].([]any)
	if len(lines) != 1 {
		t.Fatalf("expected one cleanup summary, got %d", len(lines))
	}
	var facts map[string]any
	if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
		t.Fatal(err)
	}
	t.Logf("selected real test-note cleanup locator: %+v", facts)
}
