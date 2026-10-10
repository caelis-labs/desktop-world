package main

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in, read-only capture of the explicitly selected disposable real app.
// Images stay in the caller's private temporary path for visual inspection and
// are never included in committed evidence.
func TestSelectedRealAppWindowContentCapture(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	appName := os.Getenv("DTW_POC_REAL_APP_NAME")
	prefix := os.Getenv("DTW_POC_REAL_CAPTURE_PREFIX")
	if appName == "" || prefix == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set explicitly selected real app, private capture prefix and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-selected-real-capture", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) map[string]any {
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
		if err != nil || res.IsError {
			t.Fatalf("%s failed: %v, %+v", id, err, res)
		}
		lines := res.StructuredContent.(map[string]any)["print"].([]any)
		if len(lines) != 1 {
			t.Fatalf("%s printed %d lines", id, len(lines))
		}
		var facts map[string]any
		if err := json.Unmarshal([]byte(lines[0].(string)), &facts); err != nil {
			t.Fatal(err)
		}
		return facts
	}
	setup := call("real-capture-setup", `const selected=`+string(mustJSON(appName))+`;
const all=await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],budget:{max_results:1024,max_visited_nodes:10000,read_deadline_ms:4000}});
const apps=all.objects.filter(o=>o.kind==='application'&&o.name?.status==='known'&&o.name.value===selected);
if(apps.length!==1)throw Error('selected app not unique');
const own=await dtw.observe({scope:{refs:[apps[0].ref]},projection:'capture_windows',fields:['name','role','app'],budget:{max_results:32,max_visited_nodes:512,read_deadline_ms:4000}});
if(own.coverage?.complete!==true||own.coverage?.dirty||own.coverage?.truncated)throw Error('owned capture inventory incomplete');
state.realCaptureRefs=own.objects.filter(o=>o.kind==='window'&&o.app===apps[0].ref).map(o=>o.ref);
print(JSON.stringify({app_matches:apps.length,capture_windows:state.realCaptureRefs.length,complete:own.coverage?.complete}));`)
	count, ok := setup["capture_windows"].(float64)
	if !ok || count < 1 || count > 4 {
		t.Fatalf("selected real app capture inventory unexpected: %+v", setup)
	}
	for i := 0; i < int(count); i++ {
		id := fmt.Sprintf("real-capture-%d", i)
		facts := call(id, fmt.Sprintf(`const r=await dtw.capture({kind:'window_content',target:state.realCaptureRefs[%d],max_pixel_width:800,max_pixel_height:600});
const tile=r.capture?.tiles?.[0];print(JSON.stringify({tiles:r.capture?.tiles?.length,files:r.files?.length,
target_local:tile?.target===state.realCaptureRefs[%d],width:tile?.pixel_width,height:tile?.pixel_height,
desktop_frame:tile?.desktop_frame}));`, i, i))
		if facts["tiles"] != float64(1) || facts["files"] != float64(1) || facts["target_local"] != true {
			t.Fatalf("%s invalid target-local capture: %+v", id, facts)
		}
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "result", "execution_id": id, "include_image": true,
		}})
		if err != nil || res.IsError || len(res.Content) != 2 {
			t.Fatalf("%s PNG unavailable: %v", id, err)
		}
		img, ok := res.Content[1].(*mcp.ImageContent)
		if !ok || img.MIMEType != "image/png" || len(img.Data) < 24 ||
			string(img.Data[:8]) != "\x89PNG\r\n\x1a\n" {
			t.Fatalf("%s invalid PNG", id)
		}
		width := binary.BigEndian.Uint32(img.Data[16:20])
		height := binary.BigEndian.Uint32(img.Data[20:24])
		if float64(width) != facts["width"] || float64(height) != facts["height"] ||
			width > 800 || height > 600 {
			t.Fatalf("%s PNG/tile dimensions disagree: %dx%d %+v", id, width, height, facts)
		}
		path := fmt.Sprintf("%s-%d.png", prefix, i)
		if err := os.WriteFile(path, img.Data, 0600); err != nil {
			t.Fatal(err)
		}
		hash := sha256.Sum256(img.Data)
		t.Logf("selected real app window %d target-local PNG=%dx%d bytes=%d sha256=%s private_path=%s",
			i, width, height, len(img.Data), hex.EncodeToString(hash[:]), path)
	}
}
