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

// Opt-in real-provider comparison. It prints only match counts/roles and
// coverage for the explicitly selected app; no note text or other app names.
func TestSelectedRealAppReadOnlyObserve(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	appName := os.Getenv("DTW_POC_REAL_APP_NAME")
	if appName == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set explicitly selected real app and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-selected-real-app", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const selected=` + string(mustJSON(appName)) + `;
const all=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:1024,max_visited_nodes:10000,read_deadline_ms:4000}});
const apps=all.objects.filter(o=>o.kind==='application'&&o.name?.status==='known'&&o.name.value===selected);
if(apps.length!==1){print(JSON.stringify({app_matches:apps.length,desktop_complete:all.coverage?.complete,desktop_more:!!all.coverage?.continuation}));}
else{state.selectedRealApp=apps[0].ref;
const own=await dtw.observe({scope:{refs:[state.selectedRealApp]},projection:'summary',fields:['name','role','app'],budget:{max_results:128,max_visited_nodes:1024,max_depth:4,read_deadline_ms:4000}});
const windows=own.objects.filter(o=>o.kind==='window'&&o.app===state.selectedRealApp);
const controls=[];const outline=[];const targeted=[];
for(const win of windows){let continuation;let tree;let pages=0;const objects=[];let dirty=false;let truncated=false;
do{tree=await dtw.observe({scope:{refs:[win.ref]},projection:'outline',fields:['name','role'],
budget:{max_results:512,max_visited_nodes:1200,max_output_bytes:262144,max_depth:14,read_deadline_ms:5000},continuation});
objects.push(...(Array.isArray(tree.objects)?tree.objects:[]));continuation=tree.coverage?.continuation;dirty ||= !!tree.coverage?.dirty;truncated ||= !!tree.coverage?.truncated;pages++;
}while(continuation&&pages<12);
const roles={};for(const o of objects)roles[o.role]=(roles[o.role]??0)+1;
const buttons=objects.filter(o=>o.role==='button').slice(0,6).map(o=>o.name?.status==='known'?o.name.value.slice(0,60):'unknown');
outline.push({count:objects.length,unique_refs:new Set(objects.map(o=>o.ref)).size,pages,
complete:tree.coverage?.complete&&!continuation&&!dirty&&!truncated,dirty,truncated,more:!!continuation,
unavailable:tree.coverage?.unavailable_sources??[],roles,buttons});
for(const o of objects){const label=o.name?.status==='known'?o.name.value:'';
if(/new note|new file|create note|新建笔记|新建文件/i.test(label))controls.push({role:o.role,label});}
const exact=await dtw.observe({scope:{refs:[win.ref]},projection:'outline',fields:['name','role','capabilities','bounds'],
match:{within:win.ref,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_output_bytes:32768,max_depth:14,read_deadline_ms:5000}});
const hits=(exact.objects??[]).filter(o=>o.name?.status==='known'&&o.name.value==='新建笔记');
targeted.push({matches:hits.length,complete:exact.coverage?.complete,dirty:exact.coverage?.dirty,
truncated:exact.coverage?.truncated,more:!!exact.coverage?.continuation,
roles:hits.map(o=>o.role),capabilities:hits.map(o=>o.capabilities)});}
print(JSON.stringify({app_matches:apps.length,desktop_complete:all.coverage?.complete,desktop_more:!!all.coverage?.continuation,
own_complete:own.coverage?.complete,own_dirty:own.coverage?.dirty,own_truncated:own.coverage?.truncated,
own_more:!!own.coverage?.continuation,own_objects:own.objects.length,own_windows:windows.length,
window_roles:windows.map(o=>o.role),window_names:windows.map(o=>o.name?.status==='known'?o.name.value:'unknown'),
outline,new_note_controls:controls.slice(0,8),targeted}));}`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "selected-real-app-readonly", "code": code,
	}})
	if err != nil || res.IsError {
		t.Fatalf("real app read-only observation failed: %v, %+v", err, res)
	}
	out := res.StructuredContent.(map[string]any)
	lines := out["print"].([]any)
	if len(lines) != 1 {
		t.Fatalf("expected one sanitized summary, got %d", len(lines))
	}
	var facts map[string]any
	if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
		t.Fatal(err)
	}
	t.Logf("selected real app read-only facts: %+v", facts)
	if facts["app_matches"] != float64(1) || facts["own_windows"] == nil || facts["own_windows"].(float64) < 1 {
		t.Fatal("selected real app AX window absent from current MCP chain")
	}
}
