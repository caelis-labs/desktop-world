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

// Two separate owned AppKit bundles. The physical action is short and only
// enabled by explicit fixture titles/logs; the coordinator refuses active
// user input before dispatch. This tests seat serialization, not title-grant
// exact-window authority (the current helper still grants the whole App).
func TestTwoSessionsDistinctOwnedAppsShareForegroundSeat(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	titles := []string{os.Getenv("DTW_POC_DISTINCT_TITLE_A"), os.Getenv("DTW_POC_DISTINCT_TITLE_B")}
	logs := []string{os.Getenv("DTW_POC_DISTINCT_LOG_A"), os.Getenv("DTW_POC_DISTINCT_LOG_B")}
	trace := os.Getenv("DTW_POC_COORD_TRACE")
	if titles[0] == "" || titles[1] == "" || titles[0] == titles[1] || logs[0] == "" || logs[1] == "" || trace == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires two distinct owned fixture titles, logs, trace and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	type peer struct {
		label   string
		session *mcp.ClientSession
	}
	peers := make([]peer, 2)
	for i, label := range []string{"a", "b"} {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+titles[i], "DTW_POC_COORD_HOLD_MS=350", "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: "distinct-owned-" + label, Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		peers[i] = peer{label, session}
	}
	call := func(p peer, id, code string) map[string]any {
		res, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil || res.IsError {
			t.Fatalf("%s: %v %+v", id, err, res)
		}
		return res.StructuredContent.(map[string]any)
	}
	for i, p := range peers {
		setup := `const title=` + string(mustJSON(titles[i])) + `;
const g=await dtw.grants();
const owned=(g.grants??[]).filter(x=>x.window_title===title&&x.state==='active'&&x.application);
if(owned.length!==1)throw Error('owned grant unresolved');
const app=await dtw.observe({scope:{refs:[owned[0].application]},projection:'detail',fields:['name','role','app'],budget:{max_results:1,read_deadline_ms:3000}});
if(!app.coverage?.complete||app.coverage?.dirty||app.objects.length!==1||app.objects[0].name?.status!=='known')throw Error('owned App identity unresolved');
const find=async (scope,within,name,projection)=>{
  for(let attempt=0;attempt<3;attempt++){
    const o=await dtw.observe({scope,projection,fields:['name','role','app'],
      match:{within,name_equals:name},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});
    const hits=o.objects.filter(x=>x.name?.status==='known'&&x.name.value===name);
    if(o.coverage?.complete&&!o.coverage?.dirty&&!o.coverage?.truncated&&hits.length===1)return hits[0];
    if(attempt<2)await dtw.sleep(100);
  }
  throw Error('owned object unresolved: '+name);
};
state.win=await find({refs:[owned[0].application]},owned[0].application,title,'summary');
if(state.win.kind!=='window')throw Error('owned window kind unproven');
state.field=await find({refs:[state.win.ref]},state.win.ref,'POC text','outline');
state.submit=await find({refs:[state.win.ref]},state.win.ref,'POC submit','outline');
const receipt=await dtw.act({steps:[{id:'set',op:'set_value',target:{ref:state.field.ref},set_value:{text:'POC-中文🙂'}}]});
if(receipt.outcome!=='completed'||receipt.steps?.[0]?.verification!=='verified')throw Error('owned set unverified');
print('ready');`
		if got := call(p, "distinct-setup-"+p.label, setup); fmt.Sprint(got["print"]) != "[ready]" {
			t.Fatalf("setup: %+v", got)
		}
	}
	previous := []int{ownedFixtureSubmitCount(t, logs[0]), ownedFixtureSubmitCount(t, logs[1])}
	click := `const r=await dtw.act({steps:[{id:'submit',op:'pointer.click',target:{ref:state.submit.ref},click:{button:'left',count:1}}]});print(JSON.stringify({outcome:r.outcome,delivery:r.steps?.[0]?.delivery,channel:r.steps?.[0]?.channel,restoration:r.input?.restoration}));`
	type answer struct {
		label string
		out   map[string]any
		err   error
	}
	started := make(chan struct{})
	done := make(chan answer, 2)
	for _, p := range peers {
		go func(p peer) {
			<-started
			res, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": "distinct-click-" + p.label, "code": click}})
			if err != nil {
				done <- answer{p.label, nil, err}
				return
			}
			done <- answer{p.label, res.StructuredContent.(map[string]any), nil}
		}(p)
	}
	close(started)
	failed := false
	for range 2 {
		got := <-done
		if got.err != nil {
			t.Errorf("%s transport: %v", got.label, got.err)
			failed = true
			continue
		}
		printed := fmt.Sprint(got.out["print"])
		if !strings.Contains(printed, `"outcome":"completed"`) || !strings.Contains(printed, `"delivery":"complete"`) || !strings.Contains(printed, `"channel":"foreground_transaction"`) || !strings.Contains(printed, `"restoration":"restored"`) {
			t.Errorf("%s current output: %+v", got.label, got.out)
			failed = true
			for _, p := range peers {
				if p.label != got.label {
					continue
				}
				original, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "distinct-click-" + got.label}})
				if err == nil {
					t.Logf("%s original receipt: %+v", got.label, original.StructuredContent)
				}
			}
			continue
		}
		t.Logf("%s: %s", got.label, printed)
		for _, p := range peers {
			if p.label != got.label {
				continue
			}
			original, err := p.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": "distinct-click-" + got.label}})
			if err != nil || original.IsError {
				t.Errorf("%s original result: %v %+v", got.label, err, original)
				failed = true
				break
			}
			full := original.StructuredContent.(map[string]any)
			acts, ok := full["actions"].([]any)
			if !ok || len(acts) != 1 || acts[0].(map[string]any)["run_id"] == "" {
				t.Errorf("%s original run missing: %+v", got.label, full)
				failed = true
				break
			}
			if receipts, ok := full["native_receipts"].(map[string]any); !ok || len(receipts) != 1 {
				t.Errorf("%s original native receipt missing: %+v", got.label, full)
				failed = true
			}
		}
	}
	if failed {
		return
	}
	time.Sleep(150 * time.Millisecond)
	for i := range logs {
		if now := ownedFixtureSubmitCount(t, logs[i]); now != previous[i]+1 {
			t.Fatalf("owned App %d callback delta=%d", i, now-previous[i])
		}
	}
	data, err := os.ReadFile(trace)
	if err != nil {
		t.Fatal(err)
	}
	type span struct {
		app                      string
		wait, acquired, released int64
	}
	spans := map[string]*span{"a": {}, "b": {}}
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var row struct {
			Event      string `json:"event"`
			ID         string `json:"native_id"`
			App        string `json:"app"`
			Foreground bool   `json:"foreground"`
			Time       int64  `json:"time"`
		}
		if json.Unmarshal([]byte(line), &row) != nil || !row.Foreground {
			continue
		}
		for label, s := range spans {
			if !strings.HasPrefix(row.ID, "distinct-click-"+label+"-native-") {
				continue
			}
			s.app = row.App
			switch row.Event {
			case "waiting":
				s.wait = row.Time
			case "acquired":
				s.acquired = row.Time
			case "released":
				s.released = row.Time
			}
		}
	}
	a, b := spans["a"], spans["b"]
	if a.app == "" || b.app == "" || a.app == b.app || a.wait == 0 || b.wait == 0 || a.acquired == 0 || b.acquired == 0 || a.released == 0 || b.released == 0 {
		t.Fatalf("distinct App trace incomplete: a=%+v b=%+v", a, b)
	}
	if !(a.released <= b.acquired || b.released <= a.acquired) {
		t.Fatalf("physical seat overlap: a=%+v b=%+v", a, b)
	}
	if !(a.wait < b.released && b.wait < a.released) {
		t.Fatalf("no actual contention: a=%+v b=%+v", a, b)
	}
	t.Logf("distinct App foreground serialized: a=%d ms b=%d ms", (a.released-a.acquired)/1e6, (b.released-b.acquired)/1e6)
}
