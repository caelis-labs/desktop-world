package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in native test: only one owned fixture title and its private control/log
// paths. It never returns a desktop image or unrelated window names to output.
func TestOwnedWindowCaptureRefExpiresWithoutAX(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_CAPTURE_TITLE")
	control := os.Getenv("DTW_POC_CAPTURE_CONTROL")
	logPath := os.Getenv("DTW_POC_CAPTURE_LOG")
	if title == "" || control == "" || logPath == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("set exact owned capture fixture, control/log paths and helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-owned-capture", Version: "1"}, nil)
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
		out := res.StructuredContent.(map[string]any)
		lines := out["print"].([]any)
		if len(lines) != 1 {
			t.Fatalf("%s printed %d lines", id, len(lines))
		}
		var payload map[string]any
		if err := json.Unmarshal([]byte(lines[0].(string)), &payload); err != nil {
			t.Fatal(err)
		}
		return payload
	}
	first := call("owned-capture", `const title=`+string(mustJSON(title))+`;
const ob=await dtw.observe({scope:{desktop:true},projection:'capture_windows',fields:['name','role','app'],budget:{max_results:1024,max_visited_nodes:10000,read_deadline_ms:4000}});
const own=ob.objects.filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(own.length!==1)throw Error('owned capture target count='+own.length);
state.ownedCaptureRef=own[0].ref;
const result=await dtw.capture({kind:'window_content',target:state.ownedCaptureRef,max_pixel_width:320,max_pixel_height:160});
const tile=result.capture?.tiles?.[0];
print(JSON.stringify({found:own.length,tiles:result.capture?.tiles?.length,files:result.files?.length,
pixel_width:tile?.pixel_width,pixel_height:tile?.pixel_height,
target_local:tile?.target===state.ownedCaptureRef,desktop_frame:tile?.desktop_frame,
local_a:tile?.image_to_target?.a,local_d:tile?.image_to_target?.d}));`)
	if first["found"] != float64(1) || first["tiles"] != float64(1) || first["files"] != float64(1) {
		t.Fatalf("owned window capture incomplete: %+v", first)
	}
	width, widthOK := first["pixel_width"].(float64)
	height, heightOK := first["pixel_height"].(float64)
	localA, aOK := first["local_a"].(float64)
	localD, dOK := first["local_d"].(float64)
	desktopFrame, desktopFramePresent := first["desktop_frame"]
	if !widthOK || !heightOK || width < 1 || width > 320 || height < 1 || height > 160 ||
		first["target_local"] != true || (desktopFramePresent && desktopFrame != "") ||
		!aOK || !dOK || localA <= 0 || localD <= 0 {
		t.Fatalf("owned capture geometry invalid or desktop mapping leaked: %+v", first)
	}
	imageResult, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
		"operation": "result", "execution_id": "owned-capture", "include_image": true,
	}})
	if err != nil || imageResult.IsError || len(imageResult.Content) != 2 {
		t.Fatalf("owned PNG missing: %v", err)
	}
	png, ok := imageResult.Content[1].(*mcp.ImageContent)
	if !ok || png.MIMEType != "image/png" || len(png.Data) < 8 || string(png.Data[:8]) != "\x89PNG\r\n\x1a\n" {
		t.Fatal("invalid owned PNG")
	}
	if len(png.Data) < 24 || binary.BigEndian.Uint32(png.Data[16:20]) != uint32(width) ||
		binary.BigEndian.Uint32(png.Data[20:24]) != uint32(height) {
		t.Fatal("PNG IHDR dimensions differ from capture tile")
	}
	t.Logf("owned PNG bytes=%d, pixels=%dx%d, target-local transform=%g,%g", len(png.Data),
		int(width), int(height), localA, localD)
	if err := os.WriteFile(control, []byte("close\n"), 0600); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		data, err := os.ReadFile(logPath)
		if err == nil && strings.Contains(string(data), `"event":"closed"`) {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	data, err := os.ReadFile(logPath)
	if err != nil || !strings.Contains(string(data), `"event":"closed"`) {
		t.Fatal("owned fixture did not confirm close")
	}
	stale := call("owned-capture-stale", `try{await dtw.capture({kind:'window_content',target:state.ownedCaptureRef,max_pixel_width:320,max_pixel_height:160});print(JSON.stringify({unexpected:true}));}catch(e){print(JSON.stringify({code:e.code||'unknown'}));}`)
	if stale["unexpected"] == true || stale["code"] == nil {
		t.Fatalf("stale owned Ref was accepted or lacked an error: %+v", stale)
	}
	t.Logf("stale capture Ref refused: code=%s", stale["code"])
}
