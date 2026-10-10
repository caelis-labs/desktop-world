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

// Runs only against an explicitly named, owned AppKit fixture. Two real MCP
// Sessions share the physical seat; no other application's UI is inspected.
func TestTwoSessionsOwnedForegroundContention(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_FOREGROUND_TITLE")
	logPath := os.Getenv("DTW_POC_FOREGROUND_EVENT_LOG")
	tracePath := os.Getenv("DTW_POC_COORD_TRACE")
	if title == "" || logPath == "" || tracePath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set exact owned fixture, native helper, event log and coordination trace")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	type participant struct {
		label   string
		session *mcp.ClientSession
	}
	var peers []participant
	for _, label := range []string{"a", "b"} {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title,
			"DTW_POC_COORD_HOLD_MS=350", "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: "poc-foreground-" + label, Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		peers = append(peers, participant{label, session})
	}
	call := func(p participant, id, code string) map[string]any {
		res, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || res.IsError {
			t.Fatalf("%s: %v %+v", id, err, res)
		}
		return res.StructuredContent.(map[string]any)
	}
	setup := `const title=` + string(mustJSON(title)) + `;const grants=await dtw.grants();const own=(grants.grants??[]).filter(g=>g.window_title===title&&g.state==='active'&&g.application);if(own.length!==1)throw Error('active exact owned grant absent');const app=await dtw.observe({scope:{refs:[own[0].application]},projection:'detail',fields:['name','role','app'],budget:{max_results:4,read_deadline_ms:3000}});if(app.coverage?.complete!==true||app.coverage?.dirty||app.objects.length!==1)throw Error('owned app identity unproven');const ob=await dtw.observe({scope:{refs:[own[0].application]},projection:'outline',fields:['name','role','app'],match:{within:own[0].application,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});const windows=ob.objects.filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);if(ob.coverage?.complete!==true||ob.coverage?.dirty||windows.length!==1)throw Error('exact window unproven');state.win=windows[0];const buttons=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC submit'},budget:{max_results:8,max_visited_nodes:256,max_depth:12,read_deadline_ms:3000}});const hits=buttons.objects.filter(o=>o.name?.status==='known'&&o.name.value==='POC submit');if(buttons.coverage?.complete!==true||buttons.coverage?.dirty||hits.length!==1)throw Error('submit unproven');state.submit=hits[0];print('ready');`
	for _, peer := range peers {
		if got := call(peer, "front-setup-"+peer.label, setup); fmt.Sprint(got["print"]) != "[ready]" {
			t.Fatalf("%s setup: %+v", peer.label, got)
		}
	}
	// The semantic write is background and gives the App a controlled value
	// that its callback can check without logging text.
	set := `const ob=await dtw.observe({scope:{refs:[state.win.ref]},projection:'outline',fields:['name','role'],match:{within:state.win.ref,name_equals:'POC text'},budget:{max_results:8,max_visited_nodes:256,max_depth:12,read_deadline_ms:3000}});const fields=ob.objects.filter(o=>o.name?.status==='known'&&o.name.value==='POC text');if(fields.length!==1)throw Error('field unproven');const r=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:fields[0].ref},set_value:{text:'POC-中文🙂'}}]});print(r.outcome);`
	if got := call(peers[0], "front-set", set); !strings.Contains(fmt.Sprint(got["print"]), "completed") {
		t.Fatalf("controlled semantic write: %+v", got)
	}
	previous := ownedFixtureSubmitCount(t, logPath)
	click := `const r=await dtw.act({steps:[{id:'submit',op:'pointer.click',target:{ref:state.submit.ref},click:{button:'left',count:1}}]});print(JSON.stringify({outcome:r.outcome,delivery:r.steps?.[0]?.delivery,channel:r.steps?.[0]?.channel,restoration:r.input?.restoration,foreground_ms:r.input?.foreground_ms}));`
	type answer struct {
		label string
		res   *mcp.CallToolResult
		err   error
	}
	start := make(chan struct{})
	done := make(chan answer, 2)
	for _, peer := range peers {
		go func(p participant) {
			<-start
			res, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
				"operation": "exec", "execution_id": "front-" + p.label, "code": click}})
			done <- answer{p.label, res, err}
		}(peer)
	}
	close(start)
	runIDs := make(map[string]string)
	for range 2 {
		got := <-done
		if got.err != nil || got.res.IsError {
			t.Fatalf("Session %s click: %v %+v", got.label, got.err, got.res)
		}
		out := got.res.StructuredContent.(map[string]any)
		printed := fmt.Sprint(out["print"])
		if !strings.Contains(printed, `"outcome":"completed"`) || !strings.Contains(printed, `"delivery":"complete"`) || !strings.Contains(printed, `"channel":"foreground_transaction"`) || !strings.Contains(printed, `"restoration":"restored"`) {
			t.Fatalf("Session %s foreground receipt: %+v", got.label, out)
		}
		peer := peers[0]
		if got.label == "b" {
			peer = peers[1]
		}
		full, err := peer.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "front-" + got.label}})
		if err != nil || full.IsError {
			t.Fatalf("Session %s original result: %v %+v", got.label, err, full)
		}
		facts := full.StructuredContent.(map[string]any)
		actions, ok := facts["actions"].([]any)
		if !ok || len(actions) != 1 {
			t.Fatalf("Session %s original action missing: %+v", got.label, facts)
		}
		id, _ := actions[0].(map[string]any)["run_id"].(string)
		if id == "" {
			t.Fatalf("Session %s original run ID missing: %+v", got.label, facts)
		}
		runIDs[got.label] = id
		t.Logf("Session %s original run=%s %s", got.label, id, printed)
	}
	if runIDs["a"] == runIDs["b"] {
		t.Fatal("two foreground Sessions shared a native run ID")
	}
	time.Sleep(150 * time.Millisecond)
	if now := ownedFixtureSubmitCount(t, logPath); now != previous+2 {
		t.Fatalf("two delivered receipts but owned callback delta=%d", now-previous)
	}
	data, err := os.ReadFile(tracePath)
	if err != nil {
		t.Fatal(err)
	}
	type interval struct{ waiting, acquired, released int64 }
	intervals := map[string]*interval{"a": {}, "b": {}}
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var row struct {
			Event      string `json:"event"`
			ID         string `json:"native_id"`
			Foreground bool   `json:"foreground"`
			Time       int64  `json:"time"`
		}
		if json.Unmarshal([]byte(line), &row) != nil || !row.Foreground {
			continue
		}
		for label, slot := range intervals {
			if !strings.HasPrefix(row.ID, "front-"+label+"-native-") {
				continue
			}
			switch row.Event {
			case "waiting":
				slot.waiting = row.Time
			case "acquired":
				slot.acquired = row.Time
			case "released":
				slot.released = row.Time
			}
		}
	}
	a, b := intervals["a"], intervals["b"]
	if a.waiting == 0 || b.waiting == 0 || a.acquired == 0 || b.acquired == 0 || a.released == 0 || b.released == 0 {
		t.Fatalf("missing physical seat lock events: a=%+v b=%+v", a, b)
	}
	if !(a.released <= b.acquired || b.released <= a.acquired) {
		t.Fatalf("physical seat intervals overlapped: a=%+v b=%+v", a, b)
	}
	if !(a.waiting < b.released && b.waiting < a.released) {
		t.Fatalf("two Sessions did not actually contend: a=%+v b=%+v", a, b)
	}
	t.Logf("physical foreground lock serialized two real Session callbacks; intervals a=%d ms b=%d ms", (a.released-a.acquired)/1e6, (b.released-b.acquired)/1e6)
}
