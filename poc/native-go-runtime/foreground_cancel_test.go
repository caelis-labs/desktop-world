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

// Cancels one real Session while it waits behind another Session's owned
// foreground action. The waiting Session must never dispatch to AppKit.
func TestCancelWaitingOwnedForegroundSessionKeepsPeer(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title, logPath, tracePath := os.Getenv("DTW_POC_FOREGROUND_TITLE"), os.Getenv("DTW_POC_FOREGROUND_EVENT_LOG"), os.Getenv("DTW_POC_COORD_TRACE")
	if title == "" || logPath == "" || tracePath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set exact owned fixture, native helper, event log and coordination trace")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	var sessions []*mcp.ClientSession
	for i, hold := range []string{"900", "0"} {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title,
			"DTW_POC_COORD_HOLD_MS="+hold, "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: fmt.Sprintf("front-cancel-%d", i), Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		sessions = append(sessions, session)
	}
	setup := `const title=` + string(mustJSON(title)) + `;const grants=await dtw.grants();const own=(grants.grants??[]).filter(g=>g.window_title===title&&g.state==='active'&&g.application);if(own.length!==1)throw Error('active owned grant absent');const app=await dtw.observe({scope:{refs:[own[0].application]},projection:'detail',fields:['name','role'],budget:{max_results:4}});if(app.coverage?.complete!==true||app.coverage?.dirty||app.objects.length!==1)throw Error('app identity unproven');const ob=await dtw.observe({scope:{refs:[own[0].application]},projection:'outline',fields:['name','role','app'],match:{within:own[0].application,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12}});const wins=ob.objects.filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);if(ob.coverage?.complete!==true||ob.coverage?.dirty||wins.length!==1)throw Error('owned window unproven');state.win=wins[0];const buttons=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC submit'},budget:{max_results:8,max_visited_nodes:256,max_depth:12}});const hits=buttons.objects.filter(o=>o.name?.status==='known'&&o.name.value==='POC submit');if(buttons.coverage?.complete!==true||buttons.coverage?.dirty||hits.length!==1)throw Error('button unproven');state.submit=hits[0];print('ready');`
	for i, session := range sessions {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": fmt.Sprintf("front-cancel-setup-%d", i), "code": setup}})
		if err != nil || res.IsError {
			t.Fatalf("Session %d setup: %v %+v", i, err, res)
		}
	}
	set := `const ob=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:8,max_visited_nodes:256,max_depth:12}});const fields=ob.objects.filter(o=>o.name?.status==='known'&&o.name.value==='POC text');if(fields.length!==1)throw Error('field unproven');const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:fields[0].ref},set_value:{text:'POC-中文🙂'}}]});print(r.outcome);`
	if res, err := sessions[0].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "front-cancel-set", "code": set}}); err != nil || res.IsError {
		t.Fatalf("controlled value setup: %v %+v", err, res)
	}
	previous := ownedFixtureSubmitCount(t, logPath)
	click := `const r=await dtw.act({steps:[{id:'submit',op:'pointer.click',target:{ref:state.submit.ref},click:{button:'left',count:1}}]});print(JSON.stringify({outcome:r.outcome,delivery:r.steps?.[0]?.delivery,restoration:r.input?.restoration}));`
	type answer struct {
		res *mcp.CallToolResult
		err error
	}
	aDone, bDone := make(chan answer, 1), make(chan answer, 1)
	go func() {
		res, err := sessions[0].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "front-cancel-a", "code": click}})
		aDone <- answer{res, err}
	}()
	waitEvent := func(id, event string) bool {
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			data, _ := os.ReadFile(tracePath)
			for _, line := range strings.Split(string(data), "\n") {
				var row struct {
					Event string `json:"event"`
					ID    string `json:"native_id"`
				}
				if json.Unmarshal([]byte(line), &row) == nil && row.Event == event && strings.HasPrefix(row.ID, id+"-native-") {
					return true
				}
			}
			time.Sleep(10 * time.Millisecond)
		}
		return false
	}
	if !waitEvent("front-cancel-a", "acquired") {
		t.Fatal("Session A did not acquire the foreground action lock")
	}
	go func() {
		res, err := sessions[1].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "front-cancel-b", "code": click}})
		bDone <- answer{res, err}
	}()
	if !waitEvent("front-cancel-b", "waiting") {
		t.Fatal("Session B did not contend for the foreground action lock")
	}
	if _, err := sessions[1].CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "cancel", "execution_id": "front-cancel-b"}}); err != nil {
		t.Fatal(err)
	}
	b, a := <-bDone, <-aDone
	if a.err != nil || a.res.IsError {
		t.Fatalf("peer A failed: %v %+v", a.err, a.res)
	}
	if b.err != nil {
		t.Fatalf("cancelled peer B call: %v", b.err)
	}
	if !strings.Contains(fmt.Sprint(a.res.StructuredContent), "completed") {
		t.Fatalf("peer A original action not completed: %+v", a.res.StructuredContent)
	}
	data, _ := os.ReadFile(tracePath)
	if strings.Contains(string(data), `"event":"acquired","foreground":true,"native_id":"front-cancel-b-native-`) {
		t.Fatal("cancelled Session B acquired the foreground lock")
	}
	time.Sleep(150 * time.Millisecond)
	if now := ownedFixtureSubmitCount(t, logPath); now != previous+1 {
		t.Fatalf("cancelled Session B affected AppKit; callback delta=%d", now-previous)
	}
	t.Logf("Session B cancelled while waiting; Session A completed one owned callback; B state=%v", b.res.StructuredContent)
}
