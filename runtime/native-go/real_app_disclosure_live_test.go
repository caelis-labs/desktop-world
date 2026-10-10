package main

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in read-only proof against the user's selected disposable Obsidian vault.
// The native tree stays internal; only one exact control and its field deltas
// reach MCP text/structured content across three executions in one Session.
func TestSelectedRealAppProgressiveDisclosure(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	appName := os.Getenv("DTW_POC_REAL_APP_NAME")
	if appName != "Obsidian" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set the explicitly selected Obsidian app and native helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-real-disclosure", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) (map[string]any, string, int) {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
		if err != nil || res.IsError {
			t.Fatalf("real disclosure %s: %v, %+v", id, err, res)
		}
		structured := res.StructuredContent.(map[string]any)
		if structured["state"] != "completed" {
			t.Fatalf("real disclosure %s: %+v", id, structured)
		}
		lines := structured["print"].([]any)
		if len(lines) != 1 {
			t.Fatalf("real disclosure %s printed %d lines", id, len(lines))
		}
		var facts map[string]any
		if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
			t.Fatal(err)
		}
		var textContent string
		for _, block := range res.Content {
			if block, ok := block.(*mcp.TextContent); ok {
				textContent += block.Text
			}
		}
		encoded, _ := json.Marshal(structured)
		return facts, textContent, len(encoded)
	}
	firstCode := `const selected='Obsidian';
const all=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:1024,max_visited_nodes:10000,read_deadline_ms:4000}});
const apps=all.objects.filter(o=>o.kind==='application'&&o.name?.status==='known'&&o.name.value===selected);
if(apps.length!==1)throw Error('selected app unresolved');
const app=apps[0].ref;
const own=await dtw.observe({scope:{refs:[app]},projection:'summary',fields:['name','role','app','bounds'],budget:{max_results:64,max_visited_nodes:512,read_deadline_ms:4000}});
const windows=own.objects.filter(o=>o.kind==='window'&&o.app===app&&o.name?.status==='known'&&o.name.value.includes('DevNote')&&!o.name.value.startsWith('设置'));
if(own.coverage?.complete!==true||own.coverage?.dirty||windows.length!==1)throw Error('selected test window unresolved');
state.disclosureWindow=windows[0].ref;
dtw.index(own);
const ob=await dtw.observe({scope:{refs:[state.disclosureWindow]},projection:'outline',fields:['name','role','capabilities','bounds'],
match:{within:state.disclosureWindow,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
if(ob.coverage?.complete!==true||ob.coverage?.dirty||ob.objects.filter(o=>o.name?.value==='新建笔记').length!==1)throw Error('exact control unresolved');
const disclosed=dtw.disclose(ob);if(dtw.ref(disclosed.items[0].id)!==ob.objects[0].ref)throw Error('display alias mismatch');
print(JSON.stringify(disclosed));`
	first, firstText, firstStructuredBytes := call("real-disclosure-first", firstCode)
	if len(first["items"].([]any)) != 1 || first["coverage"].(map[string]any)["complete"] != true {
		t.Fatalf("real exact initial disclosure: %+v", first)
	}
	firstItem := first["items"].([]any)[0].(map[string]any)
	if id, ok := firstItem["id"].(string); !ok || !strings.HasPrefix(id, "W") || strings.Contains(id, "r-") {
		t.Fatalf("expected short window-scoped display alias: %+v", firstItem)
	}
	if _, ok := firstItem["area_hint"].(string); !ok {
		t.Fatalf("selected real control lacks bounded geometry area hint: %+v", firstItem)
	}
	t.Logf("selected control display alias=%v area_hint=%v", firstItem["id"], firstItem["area_hint"])
	if strings.Contains(firstText, "新建笔记") {
		t.Fatalf("first model text repeated a structured control: %q", firstText)
	}
	repeatCode := `const ob=await dtw.observe({scope:{refs:[state.disclosureWindow]},projection:'outline',fields:['name','role','capabilities','bounds'],
match:{within:state.disclosureWindow,name_equals:'新建笔记'},budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
if(ob.coverage?.complete!==true||ob.coverage?.dirty||ob.objects.filter(o=>o.name?.value==='新建笔记').length!==1)throw Error('exact control unresolved');
print(JSON.stringify(dtw.disclose(ob)));`
	second, secondText, secondStructuredBytes := call("real-disclosure-repeat", repeatCode)
	if len(second["items"].([]any)) != 0 || second["unchanged"] != float64(1) || strings.Contains(secondText, "新建笔记") {
		t.Fatalf("unchanged real AX node repeated to model: %+v text=%q", second, secondText)
	}
	moreCode := strings.Replace(repeatCode, "dtw.disclose(ob)", "dtw.disclose(ob,{fields:['kind','role','name','capabilities']})", 1)
	third, thirdText, thirdStructuredBytes := call("real-disclosure-capabilities", moreCode)
	items := third["items"].([]any)
	if len(items) != 1 {
		t.Fatalf("real incremental capabilities absent: %+v", third)
	}
	item := items[0].(map[string]any)
	if _, repeated := item["name"]; repeated || strings.Contains(thirdText, "新建笔记") {
		t.Fatalf("unchanged real AX name repeated in richer view: %+v", third)
	}
	if _, added := item["capabilities"]; !added {
		t.Fatalf("real incremental capabilities absent: %+v", third)
	}
	t.Logf("selected real AX progressive disclosure: first=%d repeated=%d richer=%d items=1/0/1 complete=true, text_bytes=%d/%d/%d structured_bytes=%d/%d/%d", len(firstText), len(secondText), len(thirdText), len(firstText), len(secondText), len(thirdText), firstStructuredBytes, secondStructuredBytes, thirdStructuredBytes)
}
