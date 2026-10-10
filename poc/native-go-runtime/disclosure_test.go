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

func TestDiscloseOnlyChangedFieldsPerSession(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	connect := func() *mcp.ClientSession {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin", "DTW_POC_HELPER=")
		client := mcp.NewClient(&mcp.Implementation{Name: "poc-disclosure-test", Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		return session
	}
	call := func(session *mcp.ClientSession, id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
		if err != nil || res.IsError {
			t.Fatalf("disclosure call %s: %v, %+v", id, err, res)
		}
		out := res.StructuredContent.(map[string]any)
		if out["state"] != "completed" {
			t.Fatalf("disclosure script %s: %+v", id, out)
		}
		lines := out["print"].([]any)
		if len(lines) != 1 {
			t.Fatalf("disclosure script %s printed %d lines", id, len(lines))
		}
		var facts map[string]any
		if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
			t.Fatal(err)
		}
		return facts
	}
	a := connect()
	defer a.Close()
	b := connect()
	defer b.Close()
	base := `const ob={epoch:'one',objects:[{ref:'r1',kind:'ui',role:'button',name:{status:'known',value:'Test'},capabilities:[{name:'invoke',support:'supported'}]}],coverage:{complete:false,dirty:false,truncated:true,unavailable_sources:['ax_output_limit']}};`
	first := call(a, "disclosure-first", base+`print(JSON.stringify(dtw.disclose(ob)));`)
	if len(first["items"].([]any)) != 1 || first["coverage"].(map[string]any)["complete"] != false {
		t.Fatalf("first disclosure lost item or partial coverage: %+v", first)
	}
	second := call(a, "disclosure-repeat", base+`print(JSON.stringify(dtw.disclose(ob)));`)
	if len(second["items"].([]any)) != 0 || second["unchanged"] != float64(1) {
		t.Fatalf("repeated node was disclosed again: %+v", second)
	}
	refreshed := call(a, "disclosure-refresh", base+`print(JSON.stringify(dtw.disclose(ob,{refresh:true})));`)
	if len(refreshed["items"].([]any)) != 1 {
		t.Fatalf("explicit refresh did not restore prior facts: %+v", refreshed)
	}
	more := call(a, "disclosure-more", base+`print(JSON.stringify(dtw.disclose(ob,{fields:['kind','role','name','capabilities']})));`)
	items := more["items"].([]any)
	if len(items) != 1 {
		t.Fatalf("new field was not disclosed: %+v", more)
	}
	item := items[0].(map[string]any)
	if _, repeated := item["name"]; repeated {
		t.Fatalf("unchanged name repeated with new field: %+v", item)
	}
	if _, added := item["capabilities"]; !added {
		t.Fatalf("capabilities absent: %+v", item)
	}
	changed := call(a, "disclosure-changed", `const ob={epoch:'one',objects:[{ref:'r1',kind:'ui',role:'button',name:{status:'known',value:'Changed'}}],coverage:{complete:true,dirty:false,truncated:false}};print(JSON.stringify(dtw.disclose(ob)));`)
	if len(changed["items"].([]any)) != 1 || changed["items"].([]any)[0].(map[string]any)["name"].(map[string]any)["value"] != "Changed" {
		t.Fatalf("changed fact suppressed: %+v", changed)
	}
	peer := call(b, "disclosure-peer", base+`print(JSON.stringify(dtw.disclose(ob)));`)
	if len(peer["items"].([]any)) != 1 {
		t.Fatalf("disclosure cache leaked between sessions: %+v", peer)
	}
	layers := call(a, "disclosure-window-layers", `
const windowRect={value:{rect:{x:0,y:0,width:800,height:600}}};
dtw.index({epoch:'one',objects:[{ref:'w1',kind:'window',bounds:windowRect},{ref:'w2',kind:'window',bounds:windowRect}],coverage:{complete:true}});
const one=dtw.disclose({epoch:'one',objects:[{ref:'c1',kind:'ui',role:'container',window:'w1',name:{status:'known',value:'Left'},bounds:{value:{rect:{x:20,y:200,width:50,height:30}}}}],coverage:{scope:{refs:['w1']},complete:true}});
const two=dtw.disclose({epoch:'one',objects:[{ref:'c2',kind:'ui',role:'container',window:'w2',name:{status:'known',value:'Center'},bounds:{value:{rect:{x:400,y:200,width:50,height:30}}}}],coverage:{scope:{refs:['w2']},complete:true}});
print(JSON.stringify({one:one.items[0],two:two.items[0],resolved:dtw.ref(one.items[0].id)==='c1'&&dtw.ref(two.items[0].id)==='c2'}));`)
	one, two := layers["one"].(map[string]any), layers["two"].(map[string]any)
	if one["id"] != "W1/R1" || two["id"] != "W2/R1" || one["area_hint"] != "left" || two["area_hint"] != "main" || layers["resolved"] != true {
		t.Fatalf("window-scoped aliases or geometry hints failed: %+v", layers)
	}
	peerAlias := call(b, "disclosure-peer-alias", `let refused=false;try{dtw.ref('W1/R1')}catch{refused=true};print(JSON.stringify({refused}));`)
	if peerAlias["refused"] != true {
		t.Fatalf("display alias crossed Session boundary: %+v", peerAlias)
	}
	epoch := call(a, "disclosure-new-epoch", `const ob={epoch:'two',objects:[{ref:'w3',kind:'window',name:{status:'known',value:'New'}}],coverage:{complete:true}};const d=dtw.disclose(ob);let stale=false;try{dtw.ref('W1/R1')}catch{stale=true};print(JSON.stringify({id:d.items[0].id,stale}));`)
	if epoch["id"] != "W3" || epoch["stale"] != true {
		t.Fatalf("epoch reused an alias or retained stale display identity: %+v", epoch)
	}
}
