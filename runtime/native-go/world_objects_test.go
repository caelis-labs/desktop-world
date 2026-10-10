package main

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/caelis-labs/desktop-world/host"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestWorldObjectScriptAndTextOnlyMCP(t *testing.T) {
	t.Setenv("DTW_POC_CHILD", "1")
	t.Setenv("DTW_POC_LEGACY_OUTPUT", "0")
	coordDir := t.TempDir()
	if err := os.Chmod(coordDir, 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("DTW_POC_COORD_DIR", coordDir)
	s := newSupervisor(nil, "")
	defer func() {
		if s.child != nil {
			_ = s.child.cmd.Process.Kill()
		}
	}()
	var actions []json.RawMessage
	s.testNative = func(_ context.Context, id, op string, body json.RawMessage) (host.Reply, error) {
		var result string
		switch op {
		case "observe":
			var q struct {
				Scope struct {
					Refs    []string `json:"refs"`
					Desktop bool     `json:"desktop"`
				} `json:"scope"`
				Projection string `json:"projection"`
			}
			if err := json.Unmarshal(body, &q); err != nil {
				t.Fatal(err)
			}
			if q.Scope.Desktop {
				result = `{"epoch":"one","objects":[{"ref":"app1","kind":"application","name":{"status":"known","value":"Fixture"}}],"coverage":{"complete":false,"truncated":true,"continuation":"next"}}`
				break
			}
			if len(q.Scope.Refs) != 1 {
				t.Fatalf("unexpected observation scope: %s", body)
			}
			switch q.Scope.Refs[0] {
			case "app1":
				if q.Projection == "detail" {
					result = `{"epoch":"one","objects":[{"ref":"app1","kind":"application","name":{"status":"known","value":"Fixture"}}],"coverage":{"complete":true}}`
				} else {
					result = `{"epoch":"one","objects":[{"ref":"w1","kind":"window","app":"app1","name":{"status":"known","value":"Fixture Window"}}],"coverage":{"complete":true}}`
				}
			case "w1":
				result = `{"epoch":"one","objects":[{"ref":"b1","kind":"ui","window":"w1","app":"app1","role":"button","name":{"status":"known","value":"Submit"},"capabilities":[{"name":"invoke","support":"supported","availability":"available"},{"name":"set_checked","support":"supported","availability":"available"},{"name":"set_selected","support":"supported","availability":"available"},{"name":"set_expanded","support":"supported","availability":"available"}]},{"ref":"c1","kind":"ui","window":"w1","app":"app1","role":"image","name":{"status":"known","value":"Canvas"}}],"coverage":{"complete":true}}`
			default:
				t.Fatalf("unexpected observation target: %s", body)
			}
		case "act":
			actions = append(actions, append(json.RawMessage(nil), body...))
			result = `{"run_id":"run1","outcome":"completed","seat_health":"ready","steps":[{"id":"W1/B1.invoke","channel":"semantic","delivery":"not_applicable","verification":"verified"}],"input":{"restoration":"not_borrowed"}}`
		default:
			t.Fatalf("unexpected native operation %s", op)
		}
		return host.Reply{ID: id, Result: json.RawMessage(result)}, nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	serverTransport, clientTransport := mcp.NewInMemoryTransports()
	serverSession, err := newServerWithSupervisor(s).Connect(ctx, serverTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer serverSession.Close()
	client := mcp.NewClient(&mcp.Implementation{Name: "world-object-test", Version: "1"}, nil)
	session, err := client.Connect(ctx, clientTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, op, code, detail string) *mcp.CallToolResult {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": op, "execution_id": id, "code": code, "detail": detail}})
		if err != nil {
			t.Fatal(err)
		}
		if res.StructuredContent != nil {
			t.Fatalf("text mode duplicated structuredContent: %+v", res.StructuredContent)
		}
		return res
	}
	auth := call("auth-surface", "exec", `if(dtw.grants!==undefined||dtw.revokeGrant!==undefined)throw Error('grant API exposed');let denied=false;try{await dtw.call('grants')}catch(e){denied=String(e).includes('unknown desktop operation')}if(!denied)throw Error('grant operation exposed');print('no DTW grant API');`, "")
	if auth.IsError || !strings.Contains(auth.Content[0].(*mcp.TextContent).Text, "no DTW grant API") {
		t.Fatalf("core POC still exposes DTW authorization: %+v", auth)
	}
	first := call("e1", "exec", `const app=await dtw.app('Fixture'); const win=await app.window('Fixture Window'); const button=await win.one({name:'Submit'}); state.button=button; print(button);`, "")
	if first.IsError {
		t.Fatalf("object discovery: %+v", first)
	}
	line := first.Content[0].(*mcp.TextContent).Text
	t.Logf("model result 1 (%d UTF-8 bytes): %s", len(line), line)
	if !strings.Contains(line, "e1 · W1/B1 Submit · button · invoke") || strings.Contains(line, "native_request") || strings.Contains(line, "observation incomplete") || strings.Contains(line, `\"`) {
		t.Fatalf("not concise or reusable: %q", line)
	}
	second := call("e2", "exec", `await dtw.at('W1/B1').invoke();`, "")
	if second.IsError {
		t.Fatalf("object action: %s", second.Content[0].(*mcp.TextContent).Text)
	}
	line = second.Content[0].(*mcp.TextContent).Text
	t.Logf("model result 2 (%d UTF-8 bytes): %s", len(line), line)
	if !strings.Contains(line, "e2 · W1/B1.invoke: verified") || len(actions) != 1 {
		t.Fatalf("action not correlated: %q actions=%d", line, len(actions))
	}
	if !strings.Contains(string(actions[0]), `"target":{"ref":"b1"}`) {
		t.Fatalf("display alias did not compile to native ref: %s", actions[0])
	}
	_ = call("e3", "exec", `await dtw.transaction(tx => { tx.setChecked(state.button,true); tx.setSelected(state.button,false); tx.setExpanded(state.button,true); });`, "")
	if len(actions) != 2 || !strings.Contains(string(actions[1]), `"set_checked":{"checked":true}`) ||
		!strings.Contains(string(actions[1]), `"set_selected":{"selected":false}`) ||
		!strings.Contains(string(actions[1]), `"set_expanded":{"expanded":true}`) {
		t.Fatalf("semantic transaction payload wrong: %s", actions[1])
	}
	resumed := call("e4", "exec", `await state.button.invoke();`, "")
	if resumed.IsError || len(actions) != 3 {
		t.Fatalf("persistent JS object did not resume: %s", resumed.Content[0].(*mcp.TextContent).Text)
	}
	positioned := call("e-position", "exec", `const actual=dtw.act;dtw.act=async request=>{print(JSON.stringify(request));return {}};await state.button.dragTo(state.button,{from:{u:0.2,v:0.5},to:{u:0.7,v:0.5},durationMs:200});dtw.act=actual;`, "")
	positionText := positioned.Content[0].(*mcp.TextContent).Text
	if positioned.IsError || len(actions) != 3 ||
		!strings.Contains(positionText, `"target":{"anchor":{"target":"b1","u":0.2,"v":0.5}}`) ||
		!strings.Contains(positionText, `"to":{"anchor":{"target":"b1","u":0.7,"v":0.5}}`) {
		t.Fatalf("positioned drag did not retain the observed target: %q actions=%d", positionText, len(actions))
	}
	badPosition := call("e-position-invalid", "exec", `let refused=false;try{await state.button.move({u:1.2,v:0.5})}catch(e){refused=e instanceof RangeError}print(refused);`, "")
	if badPosition.IsError || !strings.Contains(badPosition.Content[0].(*mcp.TextContent).Text, "true") || len(actions) != 3 {
		t.Fatalf("invalid position reached native: %+v", badPosition)
	}
	auto := call("e-auto", "exec", `state.canvas=await dtw.at('W1').one({name:'Canvas'});const real=dtw.act;dtw.act=async request=>{print(JSON.stringify(request));return {}};await state.button.activate();await state.canvas.activate({u:0.2,v:0.5});dtw.act=real;`, "")
	autoText := auto.Content[0].(*mcp.TextContent).Text
	if auto.IsError || len(actions) != 3 || !strings.Contains(autoText, `"id":"W1/B1.activate","op":"invoke"`) ||
		!strings.Contains(autoText, `"id":"W1/N1.activate","op":"pointer.click"`) ||
		!strings.Contains(autoText, `"target":{"anchor":{"target":"c1","u":0.2,"v":0.5}}`) {
		t.Fatalf("behavior route did not preserve JS action ID or target: %q", autoText)
	}
	stale := call("e5", "exec", `dtw.disclose({epoch:'two',objects:[],coverage:{complete:true}});let rejected=false;try{await state.button.invoke()}catch(e){rejected=String(e).includes('expired')}print(rejected);`, "")
	if stale.IsError || !strings.Contains(stale.Content[0].(*mcp.TextContent).Text, "true") || len(actions) != 3 {
		t.Fatalf("old world address reused after epoch: %s", stale.Content[0].(*mcp.TextContent).Text)
	}
	full := call("e2", "result", "", "full")
	if !strings.Contains(full.Content[0].(*mcp.TextContent).Text, `"native_receipts"`) {
		t.Fatalf("original receipt lost: %+v", full)
	}
	status := call("e2", "status", "", "")
	if status.Content[0].(*mcp.TextContent).Text != "e2 · completed" {
		t.Fatalf("status not concise: %+v", status)
	}
	peer := newSupervisor(nil, "")
	defer func() {
		if peer.child != nil {
			_ = peer.child.cmd.Process.Kill()
		}
	}()
	peerServerTransport, peerClientTransport := mcp.NewInMemoryTransports()
	peerServerSession, err := newServerWithSupervisor(peer).Connect(ctx, peerServerTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer peerServerSession.Close()
	peerClient := mcp.NewClient(&mcp.Implementation{Name: "world-object-peer", Version: "1"}, nil)
	peerSession, err := peerClient.Connect(ctx, peerClientTransport, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer peerSession.Close()
	foreign, err := peerSession.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "exec", "execution_id": "peer", "code": `let refused=false;try{dtw.at('W1/B1')}catch(e){refused=String(e).includes('unknown or expired')}print(refused);`,
	}})
	if err != nil || foreign.IsError || foreign.StructuredContent != nil ||
		!strings.Contains(foreign.Content[0].(*mcp.TextContent).Text, "true") {
		t.Fatalf("address escaped Session: %v %+v", err, foreign)
	}
}
