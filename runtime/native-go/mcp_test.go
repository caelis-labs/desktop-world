package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestStdioChild(t *testing.T) {
	if os.Getenv("DTW_POC_SCRIPT_CHILD") == "1" {
		if err := runScriptChild(); err != nil {
			t.Fatal(err)
		}
		return
	}
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
	if len(listed.Tools) != 1 || listed.Tools[0].Name != "exec" {
		t.Fatalf("tools: %+v", listed.Tools)
	}
	call := func(op, id, code string) (*mcp.CallToolResult, map[string]any) {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
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
		_, _ = session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "loop", "code": `await dtw.sleep(10); while(true){}`}})
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

func TestOfficialMCPBoundedQuickJSMemoryRetainsControl(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-memory-bound", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(operation, id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": operation, "execution_id": id, "code": code}})
		if err != nil {
			t.Fatal(err)
		}
		out, ok := res.StructuredContent.(map[string]any)
		if !ok {
			t.Fatalf("structured: %T", res.StructuredContent)
		}
		return out
	}
	result := call("exec", "memory-limit", `state.chunks=[];for(let i=0;i<256;i++)state.chunks.push(('x'.repeat(1024*1024))+i);print('unexpected-success')`)
	if result["state"] == "completed" {
		t.Fatalf("script escaped JS memory limit: %+v", result)
	}
	if result["state"] != "failed" && result["state"] != "state_lost" {
		t.Fatalf("unexpected memory failure state: %+v", result)
	}
	if result["state"] == "failed" && !strings.Contains(strings.ToLower(fmt.Sprint(result["error"])), "memory") {
		t.Fatalf("failure did not identify memory limit: %+v", result)
	}
	status := call("status", "memory-limit", "")
	if status["state"] != result["state"] {
		t.Fatalf("original state unavailable after memory limit: %+v", status)
	}
	t.Logf("bounded JS memory: original state=%v error=%v; MCP status remained responsive", status["state"], status["error"])
}

func TestBuiltNativeGoBinaryWithNoNodePath(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	binary := os.Getenv("DTW_POC_BUILT_BINARY")
	if binary == "" {
		t.Skip("set isolated built native Go POC binary")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	child := exec.Command(binary)
	child.Env = append(os.Environ(), "PATH=/usr/bin:/bin", "DTW_POC_HELPER=")
	client := mcp.NewClient(&mcp.Implementation{Name: "built-binary-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	listed, err := session.ListTools(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(listed.Tools) != 1 || listed.Tools[0].Name != "exec" {
		t.Fatalf("unexpected public tool surface: %+v", listed.Tools)
	}
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "built-batch", "code": `state.values=[1,2,3,4].filter(x=>x%2===0);await dtw.sleep(5);print(state.values.join(','));`}})
	if err != nil || res.IsError {
		t.Fatalf("built script: %v %+v", err, res)
	}
	if res.StructuredContent.(map[string]any)["print"].([]any)[0] != "2,4" {
		t.Fatalf("built script output: %+v", res.StructuredContent)
	}
	res, err = session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "built-state", "code": `print(state.values[1]);`}})
	if err != nil || res.IsError || res.StructuredContent.(map[string]any)["print"].([]any)[0] != "4" {
		t.Fatalf("built binary state not retained: %v %+v", err, res)
	}
}

func TestWedgedScriptSubprocessLeavesExecControlAlive(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_TEST_BLOCK=1", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "wedge-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(op, id, code string) (*mcp.CallToolResult, error) {
		return session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
	}
	returned := make(chan error, 1)
	go func() {
		_, err := call("exec", "hard-wedge", `dtw._testBlock(); print('unreachable')`)
		returned <- err
	}()
	time.Sleep(100 * time.Millisecond)
	status, err := call("status", "hard-wedge", "")
	if err != nil {
		t.Fatal(err)
	}
	if status.StructuredContent.(map[string]any)["state"] != "running" {
		t.Fatalf("control not live during child wedge: %+v", status.StructuredContent)
	}
	if _, err := call("cancel", "hard-wedge", ""); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-returned:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("exec call blocked after script child kill")
	}
	result, err := call("result", "hard-wedge", "")
	if err != nil {
		t.Fatal(err)
	}
	if result.StructuredContent.(map[string]any)["state"] != "state_lost" {
		t.Fatalf("child wedge cancellation did not report state loss: %+v", result.StructuredContent)
	}
	refused, err := call("exec", "after-loss", `print('unsafe-new-context')`)
	if err != nil {
		t.Fatal(err)
	}
	if refused.StructuredContent.(map[string]any)["error"].(map[string]any)["code"] != "state_lost" {
		t.Fatalf("silently recreated lost state: %+v", refused.StructuredContent)
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
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "native-observe", "code": `const ob = await dtw.observe({scope:{desktop:true}, projection:'summary', fields:['name','role'], budget:{max_results:10}}); state.ob=ob; print(ob.objects?.length);`}})
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("native observe MCP result: %s", res.Content[0].(*mcp.TextContent).Text)
	structured := res.StructuredContent.(map[string]any)
	coverage := structured["observations"].([]any)[0].(map[string]any)
	if coverage["complete"] != false || coverage["truncated"] != true || coverage["more"] != true {
		t.Fatalf("incomplete coverage lost: %+v", coverage)
	}
	full, qerr := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "native-observe"}})
	if qerr != nil {
		t.Fatal(qerr)
	}
	detail := full.StructuredContent.(map[string]any)
	if _, ok := detail["native_receipts"]; !ok {
		t.Fatal("original native receipt missing from result query")
	}
	t.Logf("original native receipt keys: %d", len(detail["native_receipts"].(map[string]any)))
	defaultText := res.Content[0].(*mcp.TextContent).Text
	defaultStructured, _ := json.Marshal(res.StructuredContent)
	defaultEnvelope, _ := json.Marshal(res)
	fullStructured, _ := json.Marshal(full.StructuredContent)
	fullEnvelope, _ := json.Marshal(full)
	printed := structured["print"].([]any)[0].(string)
	if strings.Contains(defaultText, printed) || !strings.Contains(string(defaultStructured), printed) {
		t.Fatal("printed fact must appear once in structuredContent, not in both model-facing channels")
	}
	t.Logf("fixed native observe payload: default text=%d structured=%d envelope=%d bytes; result structured=%d envelope=%d bytes; printed fact occurs only in structuredContent", len(defaultText), len(defaultStructured), len(defaultEnvelope), len(fullStructured), len(fullEnvelope))
	if res.IsError {
		t.Fatal("native observe failed")
	}
}

func TestLateNativeWriteReceiptAfterScriptCancellation(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, eventLog := os.Getenv("DTW_POC_LATE_FIXTURE_TITLE"), os.Getenv("DTW_POC_LATE_EVENT_LOG")
	if title == "" || eventLog == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set owned late fixture, event log and native helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "DTW_POC_NATIVE_DELAY_OP=act", "DTW_POC_NATIVE_REPLY_DELAY_MS=1000", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "late-native-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(op, id, code string) (*mcp.CallToolResult, error) {
		return session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
	}
	setup := `const title=` + string(mustJSON(title)) + `;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let windows=ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title);for(let i=0;i<8&&windows.length===0&&ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});windows.push(...ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title));}if(windows.length!==1)throw Error('window not unique');state.win=windows[0];let field=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:16,max_visited_nodes:512,max_depth:12}});let matches=field.objects.filter(o=>o.name?.value==='POC text');if(matches.length!==1)throw Error('field not unique');state.field=matches[0];print('ready');`
	ready, err := call("exec", "late-setup", setup)
	if err != nil || ready.IsError {
		t.Fatalf("setup: %v %+v", err, ready)
	}
	finished := make(chan error, 1)
	go func() {
		_, e := call("exec", "late-write", `const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'LATE-中文🙂'}}]});print(r.outcome);`)
		finished <- e
	}()
	countValue := func() int {
		data, e := os.ReadFile(eventLog)
		if e != nil {
			return 0
		}
		count := 0
		for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
			var row struct {
				Event      string `json:"event"`
				FieldValue string `json:"field_value"`
			}
			if json.Unmarshal([]byte(line), &row) == nil && row.Event == "text_change" && row.FieldValue == "LATE-中文🙂" {
				count++
			}
		}
		return count
	}
	deadline := time.Now().Add(5 * time.Second)
	for countValue() == 0 && time.Now().Before(deadline) {
		time.Sleep(20 * time.Millisecond)
	}
	if countValue() != 1 {
		t.Fatal("owned app did not receive the one native semantic write")
	}
	if _, err := call("cancel", "late-write", ""); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-finished:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("cancel did not release exec call")
	}
	var result map[string]any
	for time.Now().Before(deadline.Add(2 * time.Second)) {
		res, e := call("result", "late-write", "")
		if e != nil {
			t.Fatal(e)
		}
		result = res.StructuredContent.(map[string]any)
		if receipts, ok := result["native_receipts"].(map[string]any); ok && len(receipts) == 1 {
			break
		}
		time.Sleep(30 * time.Millisecond)
	}
	receipts, ok := result["native_receipts"].(map[string]any)
	if !ok || len(receipts) != 1 {
		t.Fatalf("original late native receipt absent: %+v", result)
	}
	for _, raw := range receipts {
		reply := raw.(map[string]any)
		body := reply["result"].(map[string]any)
		if body["outcome"] != "completed" {
			t.Fatalf("late native write was not completed: %+v", body)
		}
	}
	if countValue() != 1 {
		t.Fatal("late reconciliation repeated the business write")
	}
	if path := os.Getenv("DTW_POC_LATE_RECEIPT_PATH"); path != "" {
		data, marshalErr := json.MarshalIndent(result, "", "  ")
		if marshalErr != nil {
			t.Fatal(marshalErr)
		}
		if writeErr := os.WriteFile(path, data, 0600); writeErr != nil {
			t.Fatal(writeErr)
		}
	}
	t.Logf("late original receipt retained after script state %v; owned app write count=%d", result["state"], countValue())
}

func TestTwoNativeSessionsSameAppBackgroundSerialization(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, logPath, tracePath := os.Getenv("DTW_POC_CONCURRENT_TITLE"), os.Getenv("DTW_POC_CONCURRENT_EVENT_LOG"), os.Getenv("DTW_POC_COORD_TRACE")
	titleB, logPathB := os.Getenv("DTW_POC_CONCURRENT_TITLE_B"), os.Getenv("DTW_POC_CONCURRENT_EVENT_LOG_B")
	independent := titleB != ""
	if title == "" || logPath == "" || tracePath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set owned concurrent fixture, helper and trace")
	}
	if independent && logPathB == "" {
		t.Fatal("second owned app log is required")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	type participant struct {
		session *mcp.ClientSession
		label   string
		title   string
	}
	var participants []participant
	fieldRefs := make(map[string]string)
	for _, label := range []string{"a", "b"} {
		participantTitle := title
		if label == "b" && independent {
			participantTitle = titleB
		}
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+participantTitle, "DTW_POC_COORD_HOLD_MS=400", "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: "concurrent-" + label, Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		participants = append(participants, participant{session, label, participantTitle})
	}
	for _, p := range participants {
		setup := `const title=` + string(mustJSON(p.title)) + `;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let windows=ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title);for(let i=0;i<8&&windows.length===0&&ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});windows.push(...ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title));}if(windows.length!==1)throw Error('window not unique');state.win=windows[0];await dtw.observe({scope:{refs:[state.win.app]},projection:'detail',fields:['name','role','app'],budget:{max_results:4}});let field=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:16,max_visited_nodes:512,max_depth:12}});let matches=field.objects.filter(o=>o.name?.value==='POC text');if(matches.length!==1)throw Error('field not unique');state.field=matches[0];print(JSON.stringify({app:state.win.app,field:state.field.ref}));`
		res, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "setup-" + p.label, "code": setup}})
		if err != nil || res.IsError {
			t.Fatalf("%s setup: %v %+v", p.label, err, res)
		}
		var refs struct {
			Field string `json:"field"`
		}
		printed := res.StructuredContent.(map[string]any)["print"].([]any)[0].(string)
		if json.Unmarshal([]byte(printed), &refs) != nil || refs.Field == "" {
			t.Fatalf("missing owned field ref in %s setup", p.label)
		}
		fieldRefs[p.label] = refs.Field
	}
	if fieldRefs["a"] == fieldRefs["b"] {
		t.Fatal("two native Sessions shared an object Ref")
	}
	start := make(chan struct{})
	done := make(chan error, 2)
	for _, p := range participants {
		p := p
		go func() {
			<-start
			code := `const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'SESSION-` + p.label + `'}}]});print(r.outcome);`
			res, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "same-" + p.label, "code": code}})
			if err == nil && res.IsError {
				err = fmt.Errorf("%s native result: %+v", p.label, res.StructuredContent)
			}
			done <- err
		}()
	}
	close(start)
	for range participants {
		if err := <-done; err != nil {
			t.Fatal(err)
		}
	}
	data, err := os.ReadFile(tracePath)
	if err != nil {
		t.Fatal(err)
	}
	type interval struct {
		begin, end int64
		app        string
		pid        int
	}
	intervals := map[string]*interval{}
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var e struct {
			Event string `json:"event"`
			ID    string `json:"native_id"`
			App   string `json:"app"`
			PID   int    `json:"pid"`
			Time  int64  `json:"time"`
		}
		if json.Unmarshal([]byte(line), &e) != nil {
			continue
		}
		if e.ID != "same-a-native-1" && e.ID != "same-b-native-1" {
			continue
		}
		v := intervals[e.ID]
		if v == nil {
			v = &interval{}
			intervals[e.ID] = v
		}
		if e.Event == "acquired" {
			v.begin = e.Time
			v.app = e.App
			v.pid = e.PID
		}
		if e.Event == "released" {
			v.end = e.Time
		}
	}
	a, b := intervals["same-a-native-1"], intervals["same-b-native-1"]
	if a == nil || b == nil || a.app == "" || b.app == "" || a.pid == b.pid || a.begin == 0 || b.begin == 0 || a.end == 0 || b.end == 0 {
		t.Fatalf("missing two-process coordination evidence: a=%+v b=%+v", a, b)
	}
	overlap := a.begin < b.end && b.begin < a.end
	if independent && (a.app == b.app || !overlap) || !independent && (a.app != b.app || overlap) {
		t.Fatalf("unexpected same-app/independent-app coordination: independent=%v a=%+v b=%+v", independent, a, b)
	}
	log, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(log), `"field_value":"SESSION-a"`) {
		t.Fatal("first owned app did not receive Session A's write")
	}
	if independent {
		secondLog, err := os.ReadFile(logPathB)
		if err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(string(secondLog), `"field_value":"SESSION-b"`) {
			t.Fatal("second owned app did not receive Session B's write")
		}
	} else if !strings.Contains(string(log), `"field_value":"SESSION-b"`) {
		t.Fatal("same owned app did not receive Session B's write")
	}
	foreignCode := `await dtw.act({steps:[{id:'foreign',op:'set_value',target:{ref:` + string(mustJSON(fieldRefs["a"])) + `},set_value:{text:'FOREIGN-REF'}}]});`
	foreign, err := participants[1].session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "foreign-ref", "code": foreignCode}})
	if err != nil {
		t.Fatal(err)
	}
	foreignFacts := foreign.StructuredContent.(map[string]any)
	foreignFault, ok := foreignFacts["native_error"].(map[string]any)
	if !foreign.IsError || !ok || foreignFault["code"] != "permission_denied" && foreignFault["code"] != "ref_gone" && foreignFault["code"] != "ref_expired" {
		t.Fatalf("Session B accepted Session A's Ref: %+v", foreignFacts)
	}
	for _, path := range []string{logPath, logPathB} {
		if path == "" {
			continue
		}
		data, _ := os.ReadFile(path)
		if strings.Contains(string(data), "FOREIGN-REF") {
			t.Fatal("foreign Session Ref changed an owned app")
		}
	}
	t.Logf("two MCP Sessions, distinct supervisor PIDs %d/%d, apps %q/%q; overlap=%v; lock intervals %d/%d ms", a.pid, b.pid, a.app, b.app, overlap, (a.end-a.begin)/1e6, (b.end-b.begin)/1e6)
}

func TestCancelWaitingNativeSessionDoesNotAffectPeer(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, logPath, tracePath := os.Getenv("DTW_POC_CANCEL_TITLE"), os.Getenv("DTW_POC_CANCEL_EVENT_LOG"), os.Getenv("DTW_POC_COORD_TRACE")
	if title == "" || logPath == "" || tracePath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set owned cancel fixture, helper and trace")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	var sessions []*mcp.ClientSession
	for i, hold := range []string{"900", "0"} {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "DTW_POC_COORD_HOLD_MS="+hold, "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: fmt.Sprintf("cancel-peer-%d", i), Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		sessions = append(sessions, session)
	}
	code := func(value string) string {
		return `const title=` + string(mustJSON(title)) + `;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let wins=ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title);for(let i=0;i<8&&wins.length===0&&ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});wins.push(...ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title));}if(wins.length!==1)throw Error('window not unique');await dtw.observe({scope:{refs:[wins[0].app]},projection:'detail',fields:['name','role','app'],budget:{max_results:4}});let field=await dtw.observe({scope:{refs:[wins[0].ref]},projection:'outline',fields:['name','role'],match:{within:wins[0].ref,name_equals:'POC text'},budget:{max_results:16,max_visited_nodes:512,max_depth:12}});let matches=field.objects.filter(o=>o.name?.value==='POC text');if(matches.length!==1)throw Error('field not unique');const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:matches[0].ref},set_value:{text:` + string(mustJSON(value)) + `}}]});print(r.outcome);`
	}
	type answer struct {
		result *mcp.CallToolResult
		err    error
	}
	aDone, bDone := make(chan answer, 1), make(chan answer, 1)
	go func() {
		r, e := sessions[0].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "cancel-peer-a", "code": code("CANCEL-PEER-A")}})
		aDone <- answer{r, e}
	}()
	waitTrace := func(prefix, event string) bool {
		deadline := time.Now().Add(6 * time.Second)
		for time.Now().Before(deadline) {
			data, _ := os.ReadFile(tracePath)
			for _, line := range strings.Split(string(data), "\n") {
				var row struct {
					Event string `json:"event"`
					ID    string `json:"native_id"`
				}
				if json.Unmarshal([]byte(line), &row) == nil && row.Event == event && strings.HasPrefix(row.ID, prefix) {
					return true
				}
			}
			time.Sleep(15 * time.Millisecond)
		}
		return false
	}
	if !waitTrace("cancel-peer-a-native-", "acquired") {
		t.Fatal("first native Session never acquired same-app lock")
	}
	go func() {
		r, e := sessions[1].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "cancel-peer-b", "code": code("CANCEL-PEER-B")}})
		bDone <- answer{r, e}
	}()
	if !waitTrace("cancel-peer-b-native-", "waiting") {
		t.Fatal("second native Session did not contend for same-app lock")
	}
	if _, err := sessions[1].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "cancel", "execution_id": "cancel-peer-b"}}); err != nil {
		t.Fatal(err)
	}
	b := <-bDone
	if b.err != nil {
		t.Fatal(b.err)
	}
	a := <-aDone
	if a.err != nil || a.result.IsError {
		t.Fatalf("peer A failed after B cancellation: %v %+v", a.err, a.result)
	}
	data, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), `"field_value":"CANCEL-PEER-A"`) || strings.Contains(string(data), "CANCEL-PEER-B") {
		t.Fatal("cancelled Session B wrote the app or interrupted Session A")
	}
	trace, _ := os.ReadFile(tracePath)
	for _, line := range strings.Split(string(trace), "\n") {
		var row struct {
			Event string `json:"event"`
			ID    string `json:"native_id"`
		}
		if json.Unmarshal([]byte(line), &row) == nil && row.Event == "acquired" && strings.HasPrefix(row.ID, "cancel-peer-b-native-") {
			t.Fatal("cancelled Session B acquired the write lock after cancellation")
		}
	}
	t.Log("Session B cancelled while waiting on same-app lock; Session A completed one real background write; no B app effect")
}

func TestOwnedSemanticControlsThroughGoMCP(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, kind, logPath := os.Getenv("DTW_POC_SEMANTIC_TITLE"), os.Getenv("DTW_POC_SEMANTIC_CASE"), os.Getenv("DTW_POC_SEMANTIC_EVENT_LOG")
	if title == "" || kind == "" || logPath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set owned semantic fixture title/case/log and helper")
	}
	var target, op, arm, property, eventName string
	var events []string
	switch kind {
	case "check":
		target, op, arm, property, eventName = "Approve order", "set_checked", "set_checked", "checked", "checkbox"
		events = []string{"1", "0", "1"}
	case "selection":
		target, op, arm, property, eventName = "Order A", "set_selected", "set_selected", "selected", "selected"
		events = []string{"Order A:true", "Order A:false", "Order A:true"}
	case "expand":
		target, op, arm, property, eventName = "Shipping details", "set_expanded", "set_expanded", "expanded", "expanded"
		events = []string{"true", "false", "true"}
	default:
		t.Fatal("unknown semantic case")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "semantic-" + kind, Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	setup := `const title=` + string(mustJSON(title)) + `;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let wins=ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title);for(let i=0;i<8&&wins.length===0&&ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});wins.push(...ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title));}if(wins.length!==1)throw Error('window not unique');state.win=wins[0];let target=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:` + string(mustJSON(target)) + `},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let hits=target.objects.filter(o=>o.name?.value===` + string(mustJSON(target)) + `);if(hits.length!==1)throw Error('target not unique: '+hits.length);state.target=hits[0];print('ready');`
	res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "semantic-setup", "code": setup}})
	if err != nil || res.IsError {
		t.Fatalf("semantic setup: %v %+v", err, res)
	}
	code := `for(const desired of [true,true,false,true]){const receipt=await dtw.act({steps:[{id:'semantic',op:` + string(mustJSON(op)) + `,target:{ref:state.target.ref},` + arm + `:{` + property + `:desired}}]});print(JSON.stringify({outcome:receipt.outcome,delivery:receipt.steps?.[0]?.delivery,verification:receipt.steps?.[0]?.verification,channel:receipt.steps?.[0]?.channel,restoration:receipt.input?.restoration}));}`
	res, err = session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "semantic-actions", "code": code}})
	if err != nil || res.IsError {
		t.Fatalf("semantic action: %v %+v", err, res)
	}
	facts := res.StructuredContent.(map[string]any)
	for _, raw := range facts["actions"].([]any) {
		action := raw.(map[string]any)
		steps := action["steps"].([]any)
		step := steps[0].(map[string]any)
		if action["outcome"] != "completed" || action["restoration"] != "not_borrowed" || step["channel"] != "semantic" || step["delivery"] != "complete" && step["delivery"] != "not_applicable" || step["verification"] != "verified" {
			t.Fatalf("semantic background action not verified: %+v", action)
		}
	}
	data, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	var actual []string
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var row struct {
			Event string `json:"event"`
			Value string `json:"value"`
		}
		if json.Unmarshal([]byte(line), &row) == nil && row.Event == eventName {
			actual = append(actual, row.Value)
		}
	}
	if len(actual) != len(events) {
		t.Fatalf("unexpected app callbacks for %s: %q", kind, actual)
	}
	for i, want := range events {
		if actual[i] != want {
			t.Fatalf("%s callback %d: got %q want %q", kind, i, actual[i], want)
		}
	}
	if kind == "check" || kind == "selection" {
		invokeCode := `const ob=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'提交'},budget:{max_results:8,max_visited_nodes:512,max_depth:12}});const buttons=ob.objects.filter(o=>o.name?.value==='提交');if(buttons.length!==1)throw Error('submit button not unique');const r=await dtw.act({steps:[{id:'commit',op:'invoke',target:{ref:buttons[0].ref}}]});print(JSON.stringify({outcome:r.outcome,channel:r.steps?.[0]?.channel,delivery:r.steps?.[0]?.delivery,restoration:r.input?.restoration}));`
		invoked, invokeErr := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "semantic-invoke", "code": invokeCode}})
		if invokeErr != nil || invoked.IsError {
			t.Fatalf("semantic invoke: %v %+v", invokeErr, invoked)
		}
		data, readErr := os.ReadFile(logPath)
		if readErr != nil {
			t.Fatal(readErr)
		}
		wantEvent, wantValue := "approved", "order-42"
		if kind == "selection" {
			wantEvent, wantValue = "ordered", "Order A,Order B"
		}
		found := false
		for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
			var row struct {
				Event string `json:"event"`
				Value string `json:"value"`
			}
			if json.Unmarshal([]byte(line), &row) == nil && row.Event == wantEvent && row.Value == wantValue {
				found = true
			}
		}
		if !found {
			t.Fatalf("semantic invoke did not commit owned app business result %s=%s", wantEvent, wantValue)
		}
		t.Logf("%s: semantic invoke produced owned app %s=%s", kind, wantEvent, wantValue)
	}
	if path := os.Getenv("DTW_POC_SEMANTIC_RECEIPT_PATH"); path != "" {
		bytes, _ := json.MarshalIndent(facts, "", "  ")
		if err := os.WriteFile(path, bytes, 0600); err != nil {
			t.Fatal(err)
		}
	}
	t.Logf("%s: four MCP semantic actions, three independent app transitions %v; all semantic/verified/not_borrowed", kind, actual)
}

func TestOwnedGrantRevocationThroughGoMCP(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, logPath := os.Getenv("DTW_POC_REVOKE_TITLE"), os.Getenv("DTW_POC_REVOKE_EVENT_LOG")
	if title == "" || logPath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set owned fixture title/log and native helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-grant-revoke", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) map[string]any {
		result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || result.IsError {
			t.Fatalf("%s: %v %+v", id, err, result)
		}
		return result.StructuredContent.(map[string]any)
	}
	setup := `const title=` + string(mustJSON(title)) + `;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let wins=ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title);for(let i=0;i<8&&wins.length===0&&ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});wins.push(...ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title));}if(wins.length!==1)throw Error('window not unique');state.win=wins[0];let items=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let fields=items.objects.filter(o=>o.name?.value==='POC text');if(fields.length!==1)throw Error('field not unique');state.field=fields[0];print('ready');`
	call("revoke-setup", setup)
	before := call("revoke-before", `const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'POC-中文🙂'}}]});if(r.outcome!=='completed'||r.steps?.[0]?.verification!=='verified')throw Error(JSON.stringify(r));print('verified-write');`)
	if got := before["print"].([]any)[0]; got != "verified-write" {
		t.Fatalf("pre-revoke write: %v", got)
	}
	childB := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	childB.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	clientB := mcp.NewClient(&mcp.Implementation{Name: "poc-grant-peer", Version: "1"}, nil)
	sessionB, err := clientB.Connect(ctx, &mcp.CommandTransport{Command: childB}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer sessionB.Close()
	callB := func(id, code string) map[string]any {
		result, err := sessionB.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || result.IsError {
			t.Fatalf("peer %s: %v %+v", id, err, result)
		}
		return result.StructuredContent.(map[string]any)
	}
	callB("revoke-peer-setup", setup)
	peerGrant := callB("revoke-peer-grant", `const grants=await dtw.grants();const title=`+string(mustJSON(title))+`;const grant=grants.grants?.find(g=>g.window_title===title&&g.state==='active');if(!grant)throw Error('peer grant missing');print(grant.id);`)
	t.Logf("peer independent grant: %+v", peerGrant["print"])
	time.Sleep(100 * time.Millisecond)
	beforeLog, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	changeCount := strings.Count(string(beforeLog), `"event":"text_change"`)
	control := call("revoke-control", `const grants=await dtw.grants();const title=`+string(mustJSON(title))+`;const grant=grants.grants?.find(g=>g.window_title===title&&g.state==='active');if(!grant)throw Error('active grant missing: '+JSON.stringify(grants));await dtw.revokeGrant({grant_id:grant.id});const after=await dtw.grants();const current=after.grants?.find(g=>g.id===grant.id);if(current?.state!=='revoked')throw Error('grant still active: '+JSON.stringify(after));print(JSON.stringify({id:grant.id,state:current.state}));`)
	t.Logf("revoke: %+v", control["print"])
	blocked := call("revoke-blocked", `let result;try{const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'UNAUTHORIZED'}}]});result={outcome:r.outcome,delivery:r.steps?.[0]?.delivery,fault:r.steps?.[0]?.fault?.code};}catch(e){result={error:e.code??e.message};}print(JSON.stringify(result));`)
	t.Logf("post-revoke: %+v", blocked["print"])
	if strings.Contains(fmt.Sprint(blocked["print"]), `"outcome":"completed"`) {
		t.Fatalf("write completed after revoke: %+v", blocked)
	}
	readback := call("revoke-readback", `const ob=await dtw.observe({scope:{refs:[state.field.ref]},projection:'detail',fields:['name','role','value_preview'],budget:{max_results:4}});print(JSON.stringify({value:ob.objects?.[0]?.value_preview?.value,complete:ob.coverage?.complete}));`)
	if !strings.Contains(fmt.Sprint(readback["print"]), "POC-中文🙂") {
		t.Fatalf("revoke changed controlled value: %+v", readback)
	}
	data, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Count(string(data), `"event":"text_change"`) != changeCount {
		t.Fatal("owned App received a new text change after grant revocation")
	}
	peerWrite := callB("revoke-peer-write", `const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'SESSION-B-AUTHORIZED'}}]});if(r.outcome!=='completed'||r.steps?.[0]?.verification!=='verified')throw Error(JSON.stringify(r));print('peer-verified');`)
	if got := peerWrite["print"].([]any)[0]; got != "peer-verified" {
		t.Fatalf("peer write after other Session revoke: %+v", peerWrite)
	}
	peerRead := callB("revoke-peer-read", `const ob=await dtw.observe({scope:{refs:[state.field.ref]},projection:'detail',fields:['value_preview'],budget:{max_results:4}});print(ob.objects?.[0]?.value_preview?.value);`)
	if got := peerRead["print"].([]any)[0]; got != "SESSION-B-AUTHORIZED" {
		t.Fatalf("peer write not observed: %+v", peerRead)
	}
	t.Log("Session A grant revoked and its write denied; Session B retained its independent grant and completed one real background write")
}

func TestOwnedGrantExpiresAfterAppExitThroughGoMCP(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, logPath, pidText := os.Getenv("DTW_POC_EXPIRY_TITLE"), os.Getenv("DTW_POC_EXPIRY_EVENT_LOG"), os.Getenv("DTW_POC_EXPIRY_PID")
	if title == "" || logPath == "" || pidText == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set exact owned fixture title, log, PID and native helper")
	}
	pid, err := strconv.Atoi(pidText)
	if err != nil || pid <= 1 {
		t.Fatal("invalid owned fixture PID")
	}
	logData, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	var ready struct {
		Event string `json:"event"`
		Title string `json:"title"`
		PID   int    `json:"pid"`
	}
	if json.Unmarshal([]byte(strings.SplitN(string(logData), "\n", 2)[0]), &ready) != nil || ready.Event != "ready" || ready.Title != title || ready.PID != pid {
		t.Fatal("PID/title do not match the fresh owned fixture log")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-grant-expiry", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) map[string]any {
		result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || result.IsError {
			t.Fatalf("%s: %v %+v", id, err, result)
		}
		return result.StructuredContent.(map[string]any)
	}
	setup := `const title=` + string(mustJSON(title)) + `;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let wins=ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title);for(let i=0;i<8&&wins.length===0&&ob.coverage?.continuation;i++){ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});wins.push(...ob.objects.filter(o=>o.kind==='window'&&o.name?.value===title));}if(wins.length!==1)throw Error('window not unique');state.win=wins[0];let items=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});let fields=items.objects.filter(o=>o.name?.value==='POC text');if(fields.length!==1)throw Error('field not unique');state.field=fields[0];const grants=await dtw.grants();const grant=grants.grants?.find(g=>g.window_title===title&&g.state==='active');if(!grant)throw Error('active grant missing');state.grantID=grant.id;print(grant.id);`
	active := call("expiry-setup", setup)
	t.Logf("active owned grant: %+v", active["print"])
	if err := syscall.Kill(pid, syscall.SIGTERM); err != nil {
		t.Fatalf("terminate only owned fixture PID %d: %v", pid, err)
	}
	for i := 0; i < 100; i++ {
		if err := syscall.Kill(pid, 0); err == syscall.ESRCH {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if syscall.Kill(pid, 0) != syscall.ESRCH {
		t.Fatal("owned fixture did not exit after SIGTERM")
	}
	expired := call("expiry-refresh", `let stateNow='';for(let i=0;i<10;i++){try{await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role'],budget:{max_results:1,max_visited_nodes:32,read_deadline_ms:1000}})}catch(e){}const grants=await dtw.grants();stateNow=grants.grants?.find(g=>g.id===state.grantID)?.state??'missing';if(stateNow==='expired')break;await dtw.sleep(100)}print(stateNow);`)
	if got := expired["print"].([]any)[0]; got != "expired" {
		t.Fatalf("terminated App grant not expired: %+v", expired)
	}
	blocked := call("expiry-stale-write", `try{const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'STALE-WRITE'}}]});print(JSON.stringify({outcome:r.outcome,delivery:r.steps?.[0]?.delivery}));}catch(e){print(JSON.stringify({error:e.code??e.message}));}`)
	if strings.Contains(fmt.Sprint(blocked["print"]), `"outcome":"completed"`) {
		t.Fatalf("stale write completed after App exit: %+v", blocked)
	}
	t.Logf("grant expired after owned App exited; old Ref refused: %+v", blocked["print"])
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
		res, err := s.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code}})
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
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
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
	if os.Getenv("DTW_POC_GRANT_FIRST") == "1" {
		find = `const title = ` + string(mustJSON(title)) + `;
const grants=await dtw.grants();const own=(grants.grants??[]).filter(g=>g.window_title===title&&g.state==='active'&&!!g.application);
if(own.length!==1)throw Error('exact owned grant not unique');
let ob;for(let i=0;i<2;i++){ob=await dtw.observe({scope:{refs:[own[0].application]},projection:'summary',fields:['name','role','app'],match:{within:own[0].application,name_equals:title},budget:{max_results:16,max_visited_nodes:128,max_depth:3,read_deadline_ms:3000}});if(ob.coverage?.complete===true&&!ob.coverage?.dirty&&!ob.coverage?.truncated)break;}
const windows=ob.objects.filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(windows.length!==1||ob.coverage?.complete!==true||ob.coverage?.dirty||ob.coverage?.truncated)throw Error('owned window coverage unproven');
state.win=windows[0];print(JSON.stringify({window_found:true,grant_first:true,complete:ob.coverage.complete,dirty:ob.coverage.dirty}));`
	}
	call("fixture-find", find)
	if os.Getenv("DTW_POC_APP_IDENTITY") == "1" {
		call("fixture-app", `const ob=await dtw.observe({scope:{refs:[state.win.app]},projection:'detail',fields:['name','role','app'],budget:{max_results:4}});print(JSON.stringify(ob.objects.map(o=>({ref:o.ref,kind:o.kind,name:o.name,app:o.app}))));`)
	}
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
	imageResult, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "fixture-capture", "include_image": true}})
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
	previousSubmits := 0
	if path := os.Getenv("DTW_POC_EVENT_LOG"); path != "" {
		previousSubmits = ownedFixtureSubmitCount(t, path)
	}
	click := call("fixture-submit-click", `const receipt=await dtw.act({steps:[{id:'submit',op:'pointer.click',target:{ref:state.submit.ref},click:{button:'left',count:1}}]});print(JSON.stringify({outcome:receipt.outcome,delivery:receipt.steps?.[0]?.delivery,channel:receipt.steps?.[0]?.channel,restoration:receipt.input?.restoration}));`)
	fullClick, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "fixture-submit-click"}})
	if err != nil {
		t.Fatal(err)
	}
	if path := os.Getenv("DTW_POC_RECEIPT_PATH"); path != "" {
		data, marshalErr := json.MarshalIndent(fullClick.StructuredContent, "", "  ")
		if marshalErr != nil {
			t.Fatal(marshalErr)
		}
		if writeErr := os.WriteFile(path, data, 0600); writeErr != nil {
			t.Fatal(writeErr)
		}
	}
	if !strings.Contains(click["print"].([]any)[0].(string), `"restoration":"restored"`) {
		t.Fatalf("foreground restoration unverified: %+v", click)
	}
	if path := os.Getenv("DTW_POC_EVENT_LOG"); path != "" {
		time.Sleep(150 * time.Millisecond)
		if got := ownedFixtureSubmitCount(t, path); got != previousSubmits+1 {
			t.Fatalf("native click receipt claimed complete, but owned app submit callback delta = %d; original receipt: %+v", got-previousSubmits, fullClick.StructuredContent)
		}
	}
}

// The owned fixture deliberately records no text. The callback's
// matches_expected flag is computed in-process from the controlled test value.
func ownedFixtureSubmitCount(t *testing.T, path string) int {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var event struct {
			Event           string `json:"event"`
			MatchesExpected bool   `json:"matches_expected"`
		}
		if json.Unmarshal([]byte(line), &event) == nil && event.Event == "submit" {
			count++
			if !event.MatchesExpected {
				t.Fatalf("owned submit callback did not retain the controlled test value")
			}
		}
	}
	return count
}

func mustJSON(v any) []byte {
	b, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return b
}
