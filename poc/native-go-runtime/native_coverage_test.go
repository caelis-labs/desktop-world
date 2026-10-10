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

// Read-only, bounded live desktop coverage. It prints only page counts and
// the one owned title's match count; no unrelated object names leave JS state.
func TestOwnedNativeCoverageAndGrantThroughGoMCP(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_COVERAGE_TITLE")
	if title == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set exact owned title and native helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_WRITE_WINDOW="+title, "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-coverage-test", Version: "1"}, nil)
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
	grants := call("coverage-grants", `const title=`+string(mustJSON(title))+`;const attempts=[];for(let i=0;i<3;i++){const g=await dtw.grants();const own=g.grants?.filter(x=>x.window_title===title)??[];attempts.push(own.map(x=>({state:x.state,reason:x.reason,application_ref_present:!!x.application})));if(own.length===1&&own[0].state==='active'&&own[0].application){state.ownedApp=own[0].application;break;}if(i<2)await dtw.sleep(100);}print(JSON.stringify(attempts));`)
	t.Logf("owned exact-window grant status: %v", grants["print"])
	if s, _ := grants["print"].([]any); len(s) > 0 && !strings.Contains(s[0].(string), `"state":"active"`) {
		t.Skip("owned grant unresolved; scoped native read is not authorized")
	}
	narrow := call("coverage-narrow", `if(!state.ownedApp)throw Error('owned grant unresolved');const title=`+string(mustJSON(title))+`;const ob=await dtw.observe({scope:{refs:[state.ownedApp]},projection:'outline',fields:['name','role','app'],match:{within:state.ownedApp,name_equals:title},budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});const wins=ob.objects.filter(x=>x.kind==='window'&&x.name?.status==='known'&&x.name.value===title);print(JSON.stringify({matches:wins.length,complete:ob.coverage?.complete,dirty:ob.coverage?.dirty,truncated:ob.coverage?.truncated,more:!!ob.coverage?.continuation}));`)
	t.Logf("owned app-scoped exact-window observation: %v", narrow["print"])
	narrowSummary := call("coverage-narrow-summary", `if(!state.ownedApp)throw Error('owned grant unresolved');const title=`+string(mustJSON(title))+`;const ob=await dtw.observe({scope:{refs:[state.ownedApp]},projection:'summary',fields:['name','role','app'],match:{within:state.ownedApp,name_equals:title},budget:{max_results:16,max_visited_nodes:128,max_depth:3,read_deadline_ms:3000}});const wins=ob.objects.filter(x=>x.kind==='window'&&x.name?.status==='known'&&x.name.value===title);print(JSON.stringify({matches:wins.length,complete:ob.coverage?.complete,dirty:ob.coverage?.dirty,truncated:ob.coverage?.truncated,more:!!ob.coverage?.continuation}));`)
	t.Logf("owned app-scoped summary observation: %v", narrowSummary["print"])
	scan := call("coverage-pages", `const title=`+string(mustJSON(title))+`;let ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000}});const pages=[];let matches=0;for(let i=0;i<8;i++){matches+=ob.objects.filter(x=>x.kind==='window'&&x.name?.status==='known'&&x.name.value===title).length;pages.push({count:ob.objects.length,complete:ob.coverage?.complete,dirty:ob.coverage?.dirty,truncated:ob.coverage?.truncated,more:!!ob.coverage?.continuation,unavailable:ob.coverage?.unavailable_sources});if(!ob.coverage?.continuation)break;ob=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:64,max_visited_nodes:512,max_depth:12,read_deadline_ms:3000},continuation:ob.coverage.continuation});}print(JSON.stringify({matches,pages}));`)
	var summary struct {
		Matches int `json:"matches"`
		Pages   []struct {
			Count       int      `json:"count"`
			Complete    bool     `json:"complete"`
			Dirty       bool     `json:"dirty"`
			Truncated   bool     `json:"truncated"`
			More        bool     `json:"more"`
			Unavailable []string `json:"unavailable"`
		} `json:"pages"`
	}
	if err := json.Unmarshal([]byte(scan["print"].([]any)[0].(string)), &summary); err != nil {
		t.Fatal(err)
	}
	if len(summary.Pages) == 0 || len(summary.Pages) > 8 {
		t.Fatalf("invalid bounded pages: %+v", summary)
	}
	t.Logf("owned exact-title matches=%d pages=%+v", summary.Matches, summary.Pages)
}
