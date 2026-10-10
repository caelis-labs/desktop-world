package main

import (
	"bytes"
	"context"
	"encoding/json"
	"image/color"
	"image/png"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in native test. Both windows and every logged identity are owned by this
// fixture; only target-local PNG pixels are decoded and no image is retained.
func TestOwnedWindowContentSurvivesOwnOccluder(t *testing.T) {
	if os.Getenv("DTW_TEST_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_TEST_OCCLUSION_TITLE")
	control := os.Getenv("DTW_TEST_OCCLUSION_CONTROL")
	logPath := os.Getenv("DTW_TEST_OCCLUSION_LOG")
	if title == "" || control == "" || logPath == "" || os.Getenv("DTW_TEST_HELPER") == "" {
		t.Skip("set exact owned occlusion fixture, control/log paths and helper")
	}
	t.Cleanup(func() {
		if data, err := os.ReadFile(logPath); err == nil && strings.Contains(string(data), `"event":"closed"`) {
			return
		}
		_ = os.WriteFile(control, []byte("close\n"), 0600)
	})
	waitEvent := func(want string) map[string]any {
		deadline := time.Now().Add(4 * time.Second)
		for time.Now().Before(deadline) {
			data, err := os.ReadFile(logPath)
			if err == nil {
				for _, line := range bytes.Split(data, []byte("\n")) {
					var row map[string]any
					if json.Unmarshal(line, &row) == nil && row["event"] == want {
						return row
					}
				}
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatalf("owned fixture event %q absent", want)
		return nil
	}
	ready := waitEvent("ready")
	if ready["active"] != false || ready["target_key"] != false || ready["target_visible"] != true {
		t.Fatalf("owned fixture not in safe background state: %+v", ready)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_TEST_CHILD=1", "PATH=/usr/bin:/bin")
	child.Stderr = os.Stderr
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-owned-occlusion", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	capture := func(id, code string) (map[string]any, color.NRGBA) {
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
		if payload["tiles"] != float64(1) || payload["files"] != float64(1) || payload["target_local"] != true {
			t.Fatalf("%s invalid owned capture: %+v", id, payload)
		}
		imageResult, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "result", "execution_id": id, "include_image": true,
		}})
		if err != nil || imageResult.IsError || len(imageResult.Content) != 2 {
			t.Fatalf("%s owned PNG missing: %v", id, err)
		}
		pngContent, ok := imageResult.Content[1].(*mcp.ImageContent)
		if !ok || pngContent.MIMEType != "image/png" {
			t.Fatalf("%s invalid image content", id)
		}
		img, err := png.Decode(bytes.NewReader(pngContent.Data))
		if err != nil {
			t.Fatal(err)
		}
		bounds := img.Bounds()
		if bounds.Dx() < 2 || bounds.Dy() < 2 {
			t.Fatalf("%s PNG too small: %v", id, bounds)
		}
		center := color.NRGBAModel.Convert(img.At(bounds.Min.X+bounds.Dx()/2,
			bounds.Min.Y+bounds.Dy()/2)).(color.NRGBA)
		return payload, center
	}
	first, before := capture("occlusion-before", `const title=`+string(mustJSON(title))+`;
const ob=await dtw.observe({scope:{desktop:true},projection:'capture_windows',fields:['name','role','app'],budget:{max_results:1024,max_visited_nodes:10000,read_deadline_ms:4000}});
const own=ob.objects.filter(o=>o.kind==='window'&&o.name?.status==='known'&&o.name.value===title);
if(own.length!==1)throw Error('owned target count='+own.length);
state.occlusionRef=own[0].ref;
const r=await dtw.capture({kind:'window_content',target:state.occlusionRef,max_pixel_width:360,max_pixel_height:160});
print(JSON.stringify({found:own.length,tiles:r.capture?.tiles?.length,files:r.files?.length,
target_local:r.capture?.tiles?.[0]?.target===state.occlusionRef,
width:r.capture?.tiles?.[0]?.pixel_width,height:r.capture?.tiles?.[0]?.pixel_height}));`)
	if first["found"] != float64(1) || int(before.R) <= int(before.G)+40 || int(before.R) <= int(before.B)+40 {
		t.Fatalf("owned target was not red before cover: pixel=%+v metadata=%+v", before, first)
	}
	if err := os.WriteFile(control, []byte("cover\n"), 0600); err != nil {
		t.Fatal(err)
	}
	covered := waitEvent("covered")
	if covered["active"] != false || covered["target_key"] != false ||
		covered["cover_key"] != false || covered["cover_above_target"] != true ||
		covered["target_visible"] != true || covered["cover_visible"] != true {
		t.Fatalf("owned cover not safely above target: %+v", covered)
	}
	time.Sleep(200 * time.Millisecond)
	second, during := capture("occlusion-during", `const r=await dtw.capture({kind:'window_content',target:state.occlusionRef,max_pixel_width:360,max_pixel_height:160});
print(JSON.stringify({tiles:r.capture?.tiles?.length,files:r.files?.length,
target_local:r.capture?.tiles?.[0]?.target===state.occlusionRef,
width:r.capture?.tiles?.[0]?.pixel_width,height:r.capture?.tiles?.[0]?.pixel_height}));`)
	if first["width"] != second["width"] || first["height"] != second["height"] ||
		int(during.R) <= int(during.G)+40 || int(during.R) <= int(during.B)+40 {
		t.Fatalf("owned target content changed under own cover: before=%+v during=%+v", before, during)
	}
	t.Logf("owned target content remained red under blue own cover; pixels=%vx%v, RGB before=%d/%d/%d during=%d/%d/%d",
		first["width"], first["height"], before.R, before.G, before.B,
		during.R, during.G, during.B)
	if err := os.WriteFile(control, []byte("close\n"), 0600); err != nil {
		t.Fatal(err)
	}
	closed := waitEvent("closed")
	if closed["target_visible"] != false || closed["cover_visible"] != false {
		t.Fatalf("owned windows did not close: %+v", closed)
	}
	// The fixture log contains only owned AppKit metadata; no keyboard or user
	// content is recorded. No rendered image is written to disk by this test.
	if data, err := os.ReadFile(logPath); err != nil || strings.Contains(string(data), `"event":"unknown_control"`) {
		t.Fatal("unexpected fixture control outcome")
	}
}
