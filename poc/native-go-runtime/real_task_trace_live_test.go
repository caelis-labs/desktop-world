package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in trace of one read/semantic-action/capture task in the user's selected
// disposable Obsidian. Every default MCP result is saved exactly as the model
// client sees it; no full AX tree or image is exported from the JS process.
func TestSelectedRealAppTaskResultTrace(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	tracePath := os.Getenv("DTW_POC_TASK_TRACE")
	if title == "" || tracePath == "" || os.Getenv("DTW_POC_HELPER") == "" ||
		os.Getenv("DTW_POC_WRITE_WINDOW") != title {
		t.Skip("set selected exact Obsidian window, helper and trace output")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-real-task-trace", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	compactScript := os.Getenv("DTW_POC_COMPACT_OUTPUT") == "1"
	emit := func(normal, compact string) string {
		if compactScript {
			return compact
		}
		return normal
	}
	var trace strings.Builder
	fmt.Fprintf(&trace, "# Selected Obsidian DTW task; exact model-visible MCP results\nbase=%s\nwindow=%s\ncompact=%t\n\n", "7f63e60f9f20587e13d5f816682adc106814cfb3", title, compactScript)
	defer func() {
		if err := os.WriteFile(tracePath, []byte(trace.String()), 0600); err != nil {
			t.Errorf("save task trace: %v", err)
		}
	}()
	call := func(id, purpose, code string, requireSuccess bool) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
		fmt.Fprintf(&trace, "## %s | %s\n", id, purpose)
		if err != nil {
			fmt.Fprintf(&trace, "transport_error=%v\n\n", err)
			t.Fatalf("%s: %v", id, err)
		}
		var textContent string
		for _, block := range res.Content {
			if textBlock, ok := block.(*mcp.TextContent); ok {
				textContent += textBlock.Text
			}
		}
		structured, _ := json.MarshalIndent(res.StructuredContent, "", "  ")
		compactJSON, _ := json.Marshal(res.StructuredContent)
		fmt.Fprintf(&trace, "error=%v\ncontent.text (%d UTF-8 bytes):\n%s\nstructuredContent (compact JSON %d UTF-8 bytes; pretty text below):\n%s\n\n",
			res.IsError, len(textContent), textContent, len(compactJSON), structured)
		if res.IsError && requireSuccess {
			t.Fatalf("%s returned error; inspect its original execution ID", id)
		}
		out, ok := res.StructuredContent.(map[string]any)
		if !ok || requireSuccess && out["state"] != "completed" {
			t.Fatalf("%s incomplete: %+v", id, res.StructuredContent)
		}
		return out
	}
	call("task-01-grant", "Identify exact selected window grant", `const title=`+string(mustJSON(title))+`;
const g=await dtw.grants();const own=(g.grants??[]).filter(x=>x.window_title===title&&x.state==='active'&&x.application);
if(own.length!==1)throw Error('selected exact grant unresolved');
state.taskApp=own[0].application;
`+emit(`print(JSON.stringify({grant:'active',window:title}));`, `print('grant active');`), true)
	windowResult := call("task-02-window", "Locate one selected window in the granted App", `const title=`+string(mustJSON(title))+`;
const ob=await dtw.observe({scope:{refs:[state.taskApp]},projection:'outline',fields:['name','role','app','bounds'],
match:{within:state.taskApp,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:4000}});
const wins=(ob.objects??[]).filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(ob.coverage?.complete!==true||ob.coverage?.dirty||wins.length!==1)throw Error('selected window unresolved');
state.taskWindow=wins[0].ref;dtw.index(ob);
const shown=dtw.disclose(ob,{fields:['kind','role','name']});
state.taskWindowId=shown.items.find(i=>i.kind==='window'&&i.name?.value===title)?.id;
if(!state.taskWindowId)throw Error('window display ID absent');
`+emit(`print(JSON.stringify(shown));`, `print(state.taskWindowId+' Obsidian "'+title+'"');`), false)
	if windowResult["state"] != "completed" {
		call("task-02b-window", "One narrower clean window lookup after incomplete AX read", `const title=`+string(mustJSON(title))+`;
const ob=await dtw.observe({scope:{refs:[state.taskApp]},projection:'summary',fields:['name','role','app','bounds'],
match:{within:state.taskApp,name_equals:title},budget:{max_results:16,max_visited_nodes:128,max_depth:4,read_deadline_ms:4000}});
const wins=(ob.objects??[]).filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(ob.coverage?.complete!==true||ob.coverage?.dirty||wins.length!==1)throw Error('selected window still unresolved');
state.taskWindow=wins[0].ref;dtw.index(ob);
const shown=dtw.disclose(ob,{fields:['kind','role','name']});
state.taskWindowId=shown.items.find(i=>i.kind==='window'&&i.name?.value===title)?.id;
if(!state.taskWindowId)throw Error('window display ID absent');
`+emit(`print(JSON.stringify(shown));`, `print(state.taskWindowId+' Obsidian "'+title+'"');`), true)
	}
	call("task-03-control", "Find New Note and its native capabilities", `const ob=await dtw.observe({scope:{ids:[state.taskWindowId]},projection:'outline',fields:['name','role','capabilities','bounds'],
match:{within_id:state.taskWindowId,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
const hits=(ob.objects??[]).filter(o=>o.name?.status==='known'&&o.name.value==='新建笔记');
if(ob.coverage?.complete!==true||ob.coverage?.dirty||hits.length!==1)throw Error('New Note control unresolved');
const shown=dtw.disclose(ob,{fields:['kind','role','name','capabilities']});
state.taskControlId=shown.items[0]?.id;
if(!state.taskControlId)throw Error('control display ID absent');
`+emit(`print(JSON.stringify(shown));`, `const usable=(hits[0].capabilities??[]).filter(c=>c.support==='supported'&&c.availability==='available').map(c=>c.name);
print(state.taskControlId+' '+hits[0].role+' "新建笔记" '+usable.join(','));`), true)
	call("task-04-repeat", "Recheck the same control without repeating unchanged facts", `const ob=await dtw.observe({scope:{ids:[state.taskWindowId]},projection:'outline',fields:['name','role','capabilities','bounds'],
match:{within_id:state.taskWindowId,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
`+emit(`print(JSON.stringify(dtw.disclose(ob,{fields:['kind','role','name','capabilities']})));`, `const d=dtw.disclose(ob,{fields:['kind','role','name','capabilities']});
if(ob.coverage?.complete!==true||ob.coverage?.dirty)throw Error('recheck incomplete');
print(d.items.length?JSON.stringify(d):state.taskControlId+' unchanged');`), true)
	call("task-05-act", "Ensure the selected control is visible via semantic route", `const r=await dtw.act({steps:[{id:'ensure-visible',op:'scroll_into_view',target:{id:state.taskControlId}}]});
`+emit(`print(JSON.stringify({outcome:r.outcome,channel:r.steps?.[0]?.channel,delivery:r.steps?.[0]?.delivery,
verification:r.steps?.[0]?.verification,restoration:r.input?.restoration,
fault:r.steps?.[0]?.fault?.code??r.fault?.code}));`, `if(r.outcome!=='completed')throw Error('semantic action incomplete');`), true)
	call("task-06-confirm", "Reobserve same control after action", `const ob=await dtw.observe({scope:{ids:[state.taskWindowId]},projection:'outline',fields:['name','role','capabilities','bounds'],
match:{within_id:state.taskWindowId,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
`+emit(`print(JSON.stringify(dtw.disclose(ob,{fields:['kind','role','name','capabilities']})));`, `const d=dtw.disclose(ob,{fields:['kind','role','name','capabilities']});
if(ob.coverage?.complete!==true||ob.coverage?.dirty)throw Error('confirmation incomplete');
print(d.items.length?JSON.stringify(d):state.taskControlId+' unchanged');`), true)
	call("task-07-capture-discover", "Find the visual capture target for the selected window", `const title=`+string(mustJSON(title))+`;
const ob=await dtw.observe({scope:{refs:[state.taskApp]},projection:'capture_windows',fields:['name','role','app'],
budget:{max_results:32,max_visited_nodes:512,max_depth:4,read_deadline_ms:4000}});
const wins=(ob.objects??[]).filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(ob.coverage?.complete!==true||ob.coverage?.dirty||wins.length!==1)throw Error('visual capture target unresolved');
state.taskCaptureRef=wins[0].ref;
`+emit(`print(JSON.stringify({capture_target:'found',coverage:'complete'}));`, `print('capture target ready');`), true)
	call("task-07-capture", "Capture only the selected Obsidian window; no image in default result", `const r=await dtw.capture({kind:'window_content',target:state.taskCaptureRef,max_pixel_width:640,max_pixel_height:480});
`+emit(`print(JSON.stringify({files:r.files?.length,tiles:r.capture?.tiles?.length,
target_known:!!r.capture?.target}));`, `if(r.files?.length!==1)throw Error('capture missing');`), true)
	call("task-08-broad", "Diagnose a broad window outline without exposing its private tree", `const ob=await dtw.observe({scope:{ids:[state.taskWindowId]},projection:'outline',fields:['name','role'],
budget:{max_results:128,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
state.taskBroadContinuation=ob.coverage?.continuation;
`+emit(`print(JSON.stringify({objects:ob.objects?.length,complete:ob.coverage?.complete,dirty:ob.coverage?.dirty,
truncated:ob.coverage?.truncated,more:!!ob.coverage?.continuation,
unavailable:ob.coverage?.unavailable_sources??[]}));`, `print((ob.objects?.length??0)+' nodes seen');`), true)
}
