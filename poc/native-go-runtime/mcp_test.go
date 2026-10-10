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

func TestStdioChild(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") != "1" {
		return
	}
	if err := runServer(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func TestOfficialMCPStdioSingleToolControlAndState(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	listed, err := session.ListTools(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(listed.Tools) != 1 || listed.Tools[0].Name != "dtw_exec" {
		t.Fatalf("tools: %+v", listed.Tools)
	}
	call := func(op, id, code string) (*mcp.CallToolResult, map[string]any) {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		out, ok := res.StructuredContent.(map[string]any)
		if !ok {
			t.Fatalf("structured content: %T", res.StructuredContent)
		}
		if len(res.Content) != 1 {
			t.Fatalf("content count: %d", len(res.Content))
		}
		text := res.Content[0].(*mcp.TextContent).Text
		if !strings.Contains(text, id) {
			t.Fatalf("text omitted execution ID: %s", text)
		}
		return res, out
	}
	_, first := call("exec", "one", `state.answer = [1,2,3,4].filter(x=>x%2===0); await dtw.sleep(10); print(state.answer.join(','));`)
	if first["state"] != "completed" || first["print"].([]any)[0] != "2,4" {
		t.Fatalf("first: %+v", first)
	}
	nextResult, next := call("exec", "two", `print(state.answer[1]);`)
	if next["print"].([]any)[0] != "4" {
		t.Fatalf("state not preserved: %+v", next)
	}
	structuredBytes, _ := json.Marshal(nextResult.StructuredContent)
	transportBytes, _ := json.Marshal(nextResult)
	t.Logf("fixed state-read MCP bytes: text=%d structured=%d full-result=%d", len(nextResult.Content[0].(*mcp.TextContent).Text), len(structuredBytes), len(transportBytes))
	_, same := call("exec", "one", `state.answer = [1,2,3,4].filter(x=>x%2===0); await dtw.sleep(10); print(state.answer.join(','));`)
	if same["print"].([]any)[0] != "2,4" {
		t.Fatalf("dedupe: %+v", same)
	}
	_, conflict := call("exec", "one", `print('different')`)
	if conflict["error"].(map[string]any)["code"] != "execution_conflict" {
		t.Fatalf("conflict: %+v", conflict)
	}
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		_, _ = session.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": "exec", "execution_id": "loop", "code": `await dtw.sleep(10); while(true){}`}})
	}()
	var running bool
	for i := 0; i < 100; i++ {
		_, status := call("status", "loop", "")
		if status["state"] == "running" {
			running = true
			break
		}
		time.Sleep(time.Millisecond)
	}
	if !running {
		t.Fatal("loop did not report running")
	}
	_, stopped := call("cancel", "loop", "")
	if stopped["state"] != "cancelling" && stopped["state"] != "cancelled" {
		t.Fatalf("cancel: %+v", stopped)
	}
	select {
	case <-finished:
	case <-time.After(3 * time.Second):
		t.Fatal("cancel did not stop script")
	}
	_, final := call("result", "loop", "")
	if final["state"] != "cancelled" {
		t.Fatalf("final: %+v", final)
	}
	if strings.Contains(strings.Join(os.Environ(), " "), "DTW_NODE_PATH=") {
		t.Log("parent has Node environment; child PATH still excludes Node")
	}
}

// Run only with DTW_POC_HELPER set to a freshly built native dtw binary. It
// reports counts/coverage and never logs unrelated application titles.
func TestNativeObserveThroughGoMCP(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	if os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set DTW_POC_HELPER for actual native desktop observation")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-native-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": "exec", "execution_id": "native-observe", "code": `const ob = await dtw.observe({scope:{desktop:true}, projection:'summary', fields:['name','role'], budget:{max_results:10}}); state.ob=ob; print(ob.objects?.length);`}})
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("native observe MCP result: %s", res.Content[0].(*mcp.TextContent).Text)
	structured := res.StructuredContent.(map[string]any)
	coverage := structured["observations"].([]any)[0].(map[string]any)
	if coverage["complete"] != false || coverage["truncated"] != true || coverage["more"] != true {
		t.Fatalf("incomplete coverage lost: %+v", coverage)
	}
	full, qerr := session.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": "result", "execution_id": "native-observe"}})
	if qerr != nil {
		t.Fatal(qerr)
	}
	detail := full.StructuredContent.(map[string]any)
	if _, ok := detail["native_receipts"]; !ok {
		t.Fatal("original native receipt missing from result query")
	}
	t.Logf("original native receipt keys: %d", len(detail["native_receipts"].(map[string]any)))
	if res.IsError {
		t.Fatal("native observe failed")
	}
}

func TestTwoStdioSessionsKeepJSStateAndCancellationSeparate(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	open := func() *mcp.ClientSession {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_HELPER=", "PATH=/usr/bin:/bin")
		child.Stderr = os.Stderr
		client := mcp.NewClient(&mcp.Implementation{Name: "poc-multi-test", Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		return session
	}
	a, b := open(), open()
	defer a.Close()
	defer b.Close()
	invoke := func(s *mcp.ClientSession, op, id, code string) map[string]any {
		res, err := s.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		return res.StructuredContent.(map[string]any)
	}
	if got := invoke(a, "exec", "a-init", `state.owner='A'; print(state.owner)`)["print"].([]any)[0]; got != "A" {
		t.Fatalf("A state: %v", got)
	}
	if got := invoke(b, "exec", "b-init", `state.owner='B'; print(state.owner)`)["print"].([]any)[0]; got != "B" {
		t.Fatalf("B state: %v", got)
	}
	aDone := make(chan map[string]any, 1)
	bDone := make(chan map[string]any, 1)
	go func() { aDone <- invoke(a, "exec", "a-loop", `while(true){}`) }()
	go func() { bDone <- invoke(b, "exec", "b-wait", `await dtw.sleep(100); print(state.owner)`) }()
	for i := 0; i < 100; i++ {
		if invoke(a, "status", "a-loop", "")["state"] == "running" {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if got := invoke(a, "cancel", "a-loop", "")["state"]; got != "cancelling" && got != "cancelled" {
		t.Fatalf("cancel A: %v", got)
	}
	select {
	case outcome := <-aDone:
		if outcome["state"] != "cancelled" {
			t.Fatalf("A: %+v", outcome)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("A did not stop")
	}
	select {
	case outcome := <-bDone:
		if outcome["state"] != "completed" || outcome["print"].([]any)[0] != "B" {
			t.Fatalf("B affected: %+v", outcome)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("B did not complete")
	}
	if got := invoke(a, "exec", "a-read", `print(state.owner)`)["print"].([]any)[0]; got != "A" {
		t.Fatalf("A state lost: %v", got)
	}
}

func TestOwnedAppKitSemanticThroughGoMCP(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_FIXTURE_TITLE")
	if title == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set exact owned fixture title and native helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-appkit-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		t.Logf("%s: %s", id, res.Content[0].(*mcp.TextContent).Text)
		if res.IsError {
			t.Fatalf("%s failed", id)
		}
		return res.StructuredContent.(map[string]any)
	}
	find := `const title = ` + string(mustJSON(title)) + `;
let ob = await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
let scanned=[...ob.objects];
let windows = ob.objects.filter(o=>o.kind==='window' && o.name?.status==='known' && o.name?.value===title);
for(let i=0;i<8 && windows.length===0 && ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});scanned.push(...ob.objects);windows.push(...ob.objects.filter(o=>o.kind==='window' && o.name?.status==='known' && o.name?.value===title));}
if(windows.length!==1)throw Error('fixture window not unique: '+JSON.stringify({found:windows.length,scanned:scanned.length,kinds:scanned.reduce((a,o)=>(a[o.kind]=(a[o.kind]||0)+1,a),{}),nameKeys:Object.keys(scanned[0]?.name??{}),coverage:ob.coverage}));
state.win=windows[0];print(JSON.stringify({window_found:true,complete:ob.coverage?.complete,truncated:ob.coverage?.truncated}));`
	call("fixture-find", find)
	field := `let ob = await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role','value_preview'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
let fields=ob.objects.filter(o=>o.name?.status==='known' && o.name?.value==='POC text');if(fields.length!==1)throw Error('field not unique: '+fields.length);state.field=fields[0];print(JSON.stringify({field_found:true,coverage:ob.coverage?.complete}));`
	call("fixture-field", field)
	act := `const receipt=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'POC-中文🙂'}}]});print(JSON.stringify({outcome:receipt.outcome,delivery:receipt.steps?.[0]?.delivery,verification:receipt.steps?.[0]?.verification,channel:receipt.steps?.[0]?.channel}));`
	out := call("fixture-set", act)
	if !strings.Contains(out["print"].([]any)[0].(string), "completed") {
		t.Fatalf("semantic act not completed: %+v", out)
	}
	call("fixture-read", `const ob=await dtw.observe({scope:{refs:[state.field.ref]},projection:'detail',fields:['name','role','value_preview'],budget:{max_results:4}});print(JSON.stringify({value:ob.objects?.[0]?.value_preview?.value,complete:ob.coverage?.complete}));`)
	call("fixture-capture", `const ob=await dtw.observe({scope:{refs:[state.win.app]},projection:'capture_windows',fields:['name','role','app'],budget:{max_results:16,max_visited_nodes:256,max_depth:4,read_deadline_ms:3000}});const windows=ob.objects.filter(o=>o.name?.status==='known' && o.name?.value===state.win.name.value);if(windows.length!==1)throw Error('capture window not unique: '+windows.length);const captured=await dtw.capture({kind:'window_content',target:windows[0].ref,max_pixel_width:400,max_pixel_height:400});print(JSON.stringify({files:captured.files?.length,tiles:captured.capture?.tiles?.length}));`)
	imageResult, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "dtw_exec", Arguments: map[string]any{"operation": "result", "execution_id": "fixture-capture", "include_image": true}})
	if err != nil {
		t.Fatal(err)
	}
	if imageResult.IsError || len(imageResult.Content) != 2 {
		t.Fatalf("MCP PNG missing: %+v", imageResult.StructuredContent)
	}
	png, ok := imageResult.Content[1].(*mcp.ImageContent)
	if !ok || png.MIMEType != "image/png" || len(png.Data) < 8 || string(png.Data[:8]) != "\x89PNG\r\n\x1a\n" {
		t.Fatal("invalid MCP image content")
	}
	t.Logf("owned window PNG via MCP ImageContent: %d bytes", len(png.Data))
	if os.Getenv("DTW_POC_SKIP_FOREGROUND") == "1" {
		return
	}
	call("fixture-submit-find", `const ob=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC submit'},budget:{max_results:8,max_visited_nodes:256,max_depth:12,read_deadline_ms:3000}});const buttons=ob.objects.filter(o=>o.name?.status==='known' && o.name?.value==='POC submit');if(buttons.length!==1)throw Error('submit not unique: '+buttons.length);state.submit=buttons[0];print('submit_found');`)
	click := call("fixture-submit-click", `const receipt=await dtw.act({steps:[{id:'submit',op:'pointer.click',target:{ref:state.submit.ref},click:{button:'left',count:1}}]});print(JSON.stringify({outcome:receipt.outcome,delivery:receipt.steps?.[0]?.delivery,channel:receipt.steps?.[0]?.channel,restoration:receipt.input?.restoration}));`)
	if !strings.Contains(click["print"].([]any)[0].(string), `"restoration":"restored"`) {
		t.Fatalf("foreground restoration unverified: %+v", click)
	}
}

func mustJSON(v any) []byte {
	b, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return b
}
