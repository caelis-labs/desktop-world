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

// Opt-in exact-window real-app semantic action. It uses AXScrollToVisible on a
// uniquely located control and creates no note or other persistent data.
func TestSelectedRealAppExactWindowSemanticScroll(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	if title == "" || os.Getenv("DTW_POC_HELPER") == "" || os.Getenv("DTW_POC_WRITE_WINDOW") != title {
		t.Skip("set exact selected real window title as DTW_POC_WRITE_WINDOW and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-selected-real-scroll", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const title=` + string(mustJSON(title)) + `;
const grants=await dtw.grants();
const active=(grants.grants??[]).filter(g=>g.window_title===title&&g.state==='active'&&g.application);
if(active.length!==1)throw Error('exact grant is not active');
const own=await dtw.observe({scope:{refs:[active[0].application]},projection:'outline',fields:['name','role','app'],
match:{within:active[0].application,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:4000}});
const wins=(own.objects??[]).filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(own.coverage?.complete!==true||own.coverage?.dirty||wins.length!==1)throw Error('exact window unresolved');
dtw.index(own);
const ob=await dtw.observe({scope:{refs:[wins[0].ref]},projection:'outline',fields:['name','role','capabilities'],
match:{within:wins[0].ref,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
const hits=(ob.objects??[]).filter(o=>o.name?.status==='known'&&o.name.value==='新建笔记');
if(ob.coverage?.complete!==true||ob.coverage?.dirty||hits.length!==1)throw Error('exact control unresolved');
const cap=(hits[0].capabilities??[]).find(c=>c.name==='scroll_into_view');
if(cap?.support!=='supported'||cap?.availability!=='available')throw Error('semantic scroll unsupported');
const shown=dtw.disclose(ob,{fields:['kind','role','name','capabilities']});
if(shown.items.length!==1||!shown.items[0].id.startsWith('W'))throw Error('window-scoped display identity unresolved');
const receipt=await dtw.act({steps:[{id:'scroll',op:'scroll_into_view',target:{id:shown.items[0].id}}]});
print(JSON.stringify({target_id:shown.items[0].id,outcome:receipt.outcome,delivery:receipt.steps?.[0]?.delivery,
channel:receipt.steps?.[0]?.channel,verification:receipt.steps?.[0]?.verification,
restoration:receipt.input?.restoration,fault:receipt.steps?.[0]?.fault?.code??receipt.fault?.code}));`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "selected-real-semantic-scroll", "code": code,
	}})
	if err != nil || res.IsError {
		t.Fatalf("selected real semantic scroll failed: %v, %+v", err, res)
	}
	lines := res.StructuredContent.(map[string]any)["print"].([]any)
	if len(lines) != 1 {
		t.Fatalf("expected one receipt summary, got %d", len(lines))
	}
	var facts map[string]any
	if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
		t.Fatal(err)
	}
	t.Logf("selected real exact-window semantic scroll: %+v", facts)
	if id, ok := facts["target_id"].(string); !ok || len(id) < 4 || id[:1] != "W" ||
		facts["outcome"] != "completed" || facts["channel"] != "semantic" || facts["restoration"] != "not_borrowed" {
		t.Fatal("selected real semantic action did not complete in background")
	}
}
