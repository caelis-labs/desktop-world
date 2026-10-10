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

// One-shot, opt-in real-app mutation. Only run for the user's disposable test
// vault. Never automatically repeat this test after an uncertain outcome.
func TestSelectedRealAppCreateOneNoteByExactInvoke(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	if os.Getenv("DTW_POC_REAL_INVOKE_ONCE") != "1" || title == "" ||
		os.Getenv("DTW_POC_HELPER") == "" || os.Getenv("DTW_POC_WRITE_WINDOW") != title {
		t.Skip("explicit one-shot disposable real-app invoke not enabled")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-selected-real-invoke", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const title=` + string(mustJSON(title)) + `;
const grants=await dtw.grants();
const active=(grants.grants??[]).filter(g=>g.window_title===title&&g.state==='active'&&g.application);
if(active.length!==1)throw Error('exact grant is not active');
const app=active[0].application;
const own=await dtw.observe({scope:{refs:[app]},projection:'outline',fields:['name','role','app'],
match:{within:app,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:4000}});
const wins=(own.objects??[]).filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(own.coverage?.complete!==true||own.coverage?.dirty||wins.length!==1)throw Error('exact window unresolved');
const ob=await dtw.observe({scope:{refs:[wins[0].ref]},projection:'outline',fields:['name','role','capabilities'],
match:{within:wins[0].ref,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
const hits=(ob.objects??[]).filter(o=>o.name?.status==='known'&&o.name.value==='新建笔记');
if(ob.coverage?.complete!==true||ob.coverage?.dirty||hits.length!==1)throw Error('exact control unresolved');
const cap=(hits[0].capabilities??[]).find(c=>c.name==='invoke');
if(cap?.support!=='supported'||cap?.availability!=='available')throw Error('semantic invoke unsupported');
const receipt=await dtw.act({steps:[{id:'create-one',op:'invoke',target:{ref:hits[0].ref}}]});
const after=await dtw.observe({scope:{refs:[app]},projection:'summary',fields:['name','role','app'],
budget:{max_results:32,max_visited_nodes:512,max_depth:4,read_deadline_ms:4000}});
const windows=(after.objects??[]).filter(o=>o.kind==='window'&&o.app===app);
print(JSON.stringify({outcome:receipt.outcome,delivery:receipt.steps?.[0]?.delivery,
channel:receipt.steps?.[0]?.channel,verification:receipt.steps?.[0]?.verification,
restoration:receipt.input?.restoration,fault:receipt.steps?.[0]?.fault?.code??receipt.fault?.code,
after_coverage_complete:after.coverage?.complete,after_windows:windows.length,
after_window_names:windows.map(o=>o.name?.status==='known'?o.name.value:'unknown')}));`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "selected-real-create-note-once", "code": code,
	}})
	if err != nil {
		t.Fatal(err)
	}
	if res.IsError {
		t.Fatalf("one-shot invoke returned error; inspect original ID, never replay: %+v", res.StructuredContent)
	}
	lines := res.StructuredContent.(map[string]any)["print"].([]any)
	if len(lines) != 1 {
		t.Fatalf("expected one receipt summary, got %d", len(lines))
	}
	var facts map[string]any
	if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
		t.Fatal(err)
	}
	t.Logf("one-shot selected real app invoke: %+v", facts)
	if facts["outcome"] != "completed" || facts["channel"] != "semantic" {
		t.Fatal("one-shot semantic invoke did not report completion; inspect original receipt")
	}
}
