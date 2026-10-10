package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Uses two windows in one owned fixture. The sibling request asks to set an
// already-empty field to empty, so even a gate regression cannot alter data.
// The expected result is a real helper authorizer denial before dispatch.
func TestExactWindowGrantDeniesOwnedSiblingAtRealActionBoundary(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	approved := os.Getenv("DTW_POC_EXACT_TITLE")
	sibling := os.Getenv("DTW_POC_EXACT_SIBLING_TITLE")
	if approved == "" || sibling == "" || approved == sibling || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires exact owned two-window fixture and POC helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+approved, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "exact-owned-window-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || res.IsError {
			t.Fatalf("%s: %v %+v", id, err, res)
		}
		return res.StructuredContent.(map[string]any)
	}
	setup := `const approved=` + string(mustJSON(approved)) + `,sibling=` + string(mustJSON(sibling)) + `;
const grants=await dtw.grants();const own=(grants.grants??[]).filter(x=>x.window_title===approved&&x.state==='active'&&x.application);
if(own.length!==1)throw Error('exact grant not bound');
const app=own[0].application;
const ob=await dtw.observe({scope:{refs:[app]},projection:'summary',fields:['name','role','app'],budget:{max_results:32,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
if(!ob.coverage?.complete||ob.coverage?.dirty||ob.coverage?.truncated)throw Error('App window inventory incomplete');
const findWindow=title=>{const hits=ob.objects.filter(x=>x.kind==='window'&&x.app===app&&x.name?.status==='known'&&x.name.value===title);if(hits.length!==1)throw Error('window not unique: '+title);return hits[0]};
state.approved=findWindow(approved);state.sibling=findWindow(sibling);
const findField=async(win,name)=>{const o=await dtw.observe({scope:{refs:[win.ref]},projection:'outline',fields:['name','role','app'],match:{within:win.ref,name_equals:name},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});const hits=o.objects.filter(x=>x.name?.status==='known'&&x.name.value===name);if(!o.coverage?.complete||o.coverage?.dirty||hits.length!==1)throw Error('field unresolved: '+name);return hits[0]};
state.field=await findField(state.approved,'POC text');state.siblingField=await findField(state.sibling,'POC sibling text');
print(JSON.stringify({approved_window:state.approved.ref,sibling_window:state.sibling.ref,distinct:state.approved.ref!==state.sibling.ref}));`
	ready := call("exact-setup", setup)
	if !strings.Contains(fmt.Sprint(ready["print"]), `"distinct":true`) {
		t.Fatalf("windows not distinct: %+v", ready)
	}
	negative := call("exact-sibling-deny", `let outcome;try{const r=await dtw.act({steps:[{id:'sibling-noop',op:'set_value',target:{ref:state.siblingField.ref},set_value:{text:''}}]});outcome={outcome:r.outcome,delivery:r.steps?.[0]?.delivery,fault:r.steps?.[0]?.fault?.code??r.fault?.code};}catch(e){outcome={error:e.code??e.message}}print(JSON.stringify(outcome));`)
	t.Logf("owned sibling no-op result: %v", negative["print"])
	if !strings.Contains(fmt.Sprint(negative["print"]), "permission_denied") {
		t.Fatalf("sibling was not denied at helper boundary: %+v", negative)
	}
	positive := call("exact-approved-set", `const r=await dtw.act({steps:[{id:'approved',op:'set_value',target:{ref:state.field.ref},set_value:{text:'POC-中文🙂'}}]});print(JSON.stringify({outcome:r.outcome,delivery:r.steps?.[0]?.delivery,verification:r.steps?.[0]?.verification,channel:r.steps?.[0]?.channel}));`)
	t.Logf("approved owned window: %v", positive["print"])
	if !strings.Contains(fmt.Sprint(positive["print"]), `"outcome":"completed"`) || !strings.Contains(fmt.Sprint(positive["print"]), `"verification":"verified"`) {
		t.Fatalf("approved target not verified: %+v", positive)
	}
	full, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "exact-sibling-deny"}})
	if err != nil || full.IsError {
		t.Fatalf("original sibling receipt: %v %+v", err, full)
	}
	t.Logf("original denied execution retained: %v", full.StructuredContent)
}

func TestExactHelperRetainsExplicitOwnedAppScope(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	name, sibling := os.Getenv("DTW_POC_EXPLICIT_APP_NAME"), os.Getenv("DTW_POC_EXACT_SIBLING_TITLE")
	if name == "" || sibling == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires owned App name, sibling title and POC helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_APP="+name, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "explicit-owned-app-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	code := `const name=` + string(mustJSON(name)) + `,title=` + string(mustJSON(sibling)) + `;
const g=await dtw.grants();const own=(g.grants??[]).filter(x=>x.name===name&&x.state==='active'&&x.application);
if(own.length!==1)throw Error('explicit App grant unresolved');
const app=own[0].application;
const ob=await dtw.observe({scope:{refs:[app]},projection:'summary',fields:['name','role','app'],match:{within:app,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
const wins=ob.objects.filter(x=>x.kind==='window'&&x.name?.status==='known'&&x.name.value===title);
if(!ob.coverage?.complete||ob.coverage?.dirty||wins.length!==1)throw Error('sibling window unresolved');
const field=await dtw.observe({scope:{refs:[wins[0].ref]},projection:'outline',fields:['name','role'],match:{within:wins[0].ref,name_equals:'POC sibling text'},budget:{max_results:8,max_visited_nodes:256,max_depth:12,read_deadline_ms:3000}});
const hits=field.objects.filter(x=>x.name?.status==='known'&&x.name.value==='POC sibling text');
if(!field.coverage?.complete||field.coverage?.dirty||hits.length!==1)throw Error('sibling field unresolved');
const set=async text=>dtw.act({steps:[{id:'app-scope',op:'set_value',target:{ref:hits[0].ref},set_value:{text}}]});
const written=await set('APP-SCOPE-POC');
if(written.outcome!=='completed'||written.steps?.[0]?.verification!=='verified')throw Error('explicit App scope write unverified');
const cleared=await set('');
if(cleared.outcome!=='completed'||cleared.steps?.[0]?.verification!=='verified')throw Error('explicit App scope cleanup unverified');
print(JSON.stringify({write:written.outcome,cleanup:cleared.outcome,channel:written.steps?.[0]?.channel}));`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "explicit-app-sibling", "code": code}})
	if err != nil || res.IsError {
		t.Fatalf("explicit App scope: %v %+v", err, res)
	}
	printed := fmt.Sprint(res.StructuredContent.(map[string]any)["print"])
	if !strings.Contains(printed, `"write":"completed"`) || !strings.Contains(printed, `"cleanup":"completed"`) {
		t.Fatalf("explicit App scope: %s", printed)
	}
	t.Logf("owned explicit App grant retained: %s", printed)
}

func TestExactWindowGrantDoesNotFollowOwnedSameTitleReplacement(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, control, logPath := os.Getenv("DTW_POC_REPLACE_TITLE"), os.Getenv("DTW_POC_REPLACE_CONTROL"), os.Getenv("DTW_POC_REPLACE_LOG")
	if title == "" || control == "" || logPath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires owned replacement fixture and POC helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "exact-owned-replacement-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || res.IsError {
			t.Fatalf("%s: %v %+v", id, err, res)
		}
		return res.StructuredContent.(map[string]any)
	}
	setup := `const title=` + string(mustJSON(title)) + `;
const grants=await dtw.grants();const own=(grants.grants??[]).filter(x=>x.window_title===title&&x.state==='active'&&x.application);
if(own.length!==1)throw Error('grant identity unresolved');state.app=own[0].application;
const ob=await dtw.observe({scope:{refs:[state.app]},projection:'summary',fields:['name','role','app'],match:{within:state.app,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
const wins=ob.objects.filter(x=>x.kind==='window'&&x.name?.value===title);
if(!ob.coverage?.complete||ob.coverage?.dirty||wins.length!==1)throw Error('old window unresolved');state.oldWindow=wins[0];
const fields=await dtw.observe({scope:{refs:[state.oldWindow.ref]},projection:'outline',fields:['name','role'],match:{within:state.oldWindow.ref,name_equals:'POC text'},budget:{max_results:8,max_visited_nodes:256,max_depth:12,read_deadline_ms:3000}});
const found=fields.objects.filter(x=>x.name?.value==='POC text');if(!fields.coverage?.complete||fields.coverage?.dirty||found.length!==1)throw Error('old field unresolved');state.oldField=found[0];
print(JSON.stringify({old_window:state.oldWindow.ref,old_field:state.oldField.ref}));`
	first := call("replacement-setup", setup)
	t.Logf("bound original window: %v", first["print"])
	if err := os.WriteFile(control, []byte("replace_main\n"), 0600); err != nil {
		t.Fatal(err)
	}
	var replaced bool
	for i := 0; i < 40; i++ {
		data, err := os.ReadFile(logPath)
		if err == nil && strings.Contains(string(data), `"event":"replaced_main"`) {
			replaced = true
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	if !replaced {
		t.Fatal("fixture did not replace original owned native window")
	}
	stale := call("replacement-stale-ref", `let status;try{const r=await dtw.act({steps:[{id:'stale-noop',op:'set_value',target:{ref:state.oldField.ref},set_value:{text:''}}]});status={outcome:r.outcome,delivery:r.steps?.[0]?.delivery,fault:r.steps?.[0]?.fault?.code??r.fault?.code}}catch(e){status={error:e.code??e.message}}print(JSON.stringify(status));`)
	t.Logf("stale original ref: %v", stale["print"])
	if strings.Contains(fmt.Sprint(stale["print"]), `"outcome":"completed"`) {
		t.Fatalf("stale original window accepted: %+v", stale)
	}
	newWindow := call("replacement-fresh-ref", `const title=`+string(mustJSON(title))+`;
const ob=await dtw.observe({scope:{refs:[state.app]},projection:'summary',fields:['name','role','app'],match:{within:state.app,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
const wins=ob.objects.filter(x=>x.kind==='window'&&x.name?.value===title&&x.lifecycle==='live');
if(!ob.coverage?.complete||ob.coverage?.dirty||wins.length!==1||wins[0].ref===state.oldWindow.ref)throw Error('new window unresolved');state.newWindow=wins[0];
const fields=await dtw.observe({scope:{refs:[state.newWindow.ref]},projection:'outline',fields:['name','role'],match:{within:state.newWindow.ref,name_equals:'POC text'},budget:{max_results:8,max_visited_nodes:256,max_depth:12,read_deadline_ms:3000}});
const found=fields.objects.filter(x=>x.name?.value==='POC text');if(!fields.coverage?.complete||fields.coverage?.dirty||found.length!==1)throw Error('new field unresolved');state.newField=found[0];
let status;try{const r=await dtw.act({steps:[{id:'new-noop',op:'set_value',target:{ref:state.newField.ref},set_value:{text:''}}]});status={outcome:r.outcome,delivery:r.steps?.[0]?.delivery,fault:r.steps?.[0]?.fault?.code??r.fault?.code}}catch(e){status={error:e.code??e.message}};
print(JSON.stringify({old:state.oldWindow.ref,new:state.newWindow.ref,status}));`)
	t.Logf("same-title new window: %v", newWindow["print"])
	if !strings.Contains(fmt.Sprint(newWindow["print"]), "permission_denied") {
		t.Fatalf("same-title new window not denied: %+v", newWindow)
	}
	status := call("replacement-grant-status", `const title=`+string(mustJSON(title))+`;const g=await dtw.grants();const own=g.grants?.filter(x=>x.window_title===title)??[];if(own.length!==1)throw Error('original declaration missing');print(JSON.stringify({state:own[0].state,reason:own[0].reason}));`)
	t.Logf("replacement grant status: %v", status["print"])
	if !strings.Contains(fmt.Sprint(status["print"]), `"state":"unresolved"`) || !strings.Contains(fmt.Sprint(status["print"]), "window_identity_unavailable_or_changed") {
		t.Fatalf("stale exact grant still reported active: %+v", status)
	}
}
