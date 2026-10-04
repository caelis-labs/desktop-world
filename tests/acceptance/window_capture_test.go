package acceptance_test

import (
	"fmt"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"image/png"
	"os"
	"testing"
	"time"
)

func TestNativeWindowCapture(t *testing.T) {
	title, logPath := requiredFeatureFixture(t, "DTW_F9")
	frontTitle, frontLog := requiredFeatureFixture(t, "DTW_FOREGROUND")
	bg := featureStart(t, dw.InputNoShared)
	controls := bg.window(title)
	bg.grant(controls.App)
	invoke := func(name string) {
		o := bg.find(controls.Ref, name)
		r := bg.act(dw.Step{ID: "invoke", Op: "invoke", Target: dw.Target{Ref: o.Ref}})
		if r.Steps[0].Channel != "semantic" {
			t.Fatal(r)
		}
		time.Sleep(100 * time.Millisecond)
	}
	inventory := func() dw.Observation {
		var ob dw.Observation
		bg.call("observe", dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{controls.App}}, Projection: dw.ProjectionCaptureWindows, Fields: []string{"name", "role", "app"}, Budget: dw.Budget{MaxResults: 16, MaxOutputBytes: 4096}}, &ob)
		if !ob.Coverage.Complete || ob.Coverage.Dirty {
			t.Fatal("incomplete capture inventory", ob)
		}
		for _, o := range ob.Objects {
			if o.App != controls.App || o.Kind != dw.KindWindow || o.Bounds.Value != nil || len(o.Capabilities) != 0 || o.ValuePreview.Value != nil {
				t.Fatal("unrequested disclosure", o)
			}
		}
		return ob
	}
	findCapture := func(suffix string) dw.Ref {
		for _, o := range inventory().Objects {
			if o.Name.Value != nil && *o.Name.Value == title+suffix {
				return o.Ref
			}
		}
		t.Fatalf("capture window %q missing", suffix)
		return ""
	}
	canvas := findCapture(" Canvas")
	if canvas == controls.Ref {
		t.Fatal("AX and capture identities were guessed/joined")
	}
	refuse := func(ref dw.Ref, codes ...string) {
		bg.seq++
		r, err := bg.c.Call(bg.ctx, "feature", fmt.Sprintf("refuse-%d", bg.seq), "capture", dw.CaptureRequest{Kind: "window_content", Target: ref, MaxPixelWidth: 640, MaxPixelHeight: 640})
		bg.calls++
		bg.bytes += len(host.Content(r).Content[0].Text)
		if err != nil || r.Error == nil {
			t.Fatal("expected fresh refusal", r, err)
		}
		for _, code := range codes {
			if r.Error.Code == code {
				return
			}
		}
		t.Fatal("unexpected refusal", r.Error)
	}
	refuse(controls.Ref, "capability_unavailable")
	// The human simulator owns a separate dtw process and the real shared seat.
	fg := featureStart(t, dw.InputShared)
	front := fg.window(frontTitle)
	editor := fg.find(front.Ref, "内容")
	fg.grant(front.App)
	fg.act(dw.Step{ID: "window", Op: "focus", Target: dw.Target{Ref: front.Ref}}, dw.Step{ID: "editor", Op: "focus", Target: dw.Target{Ref: editor.Ref}})
	seatReq := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{front.Ref}}, Projection: dw.ProjectionDetail, Fields: []string{"role"}, Budget: dw.Budget{MaxOutputBytes: 2048}}
	var before, after dw.Observation
	fg.call("observe", seatReq, &before)
	if before.Seat.ForegroundWindow.Value == nil || *before.Seat.ForegroundWindow.Value != front.Ref {
		t.Fatal("foreground setup")
	}
	images, pngBytes, pixels := 0, 0, 0
	capture := func(ref dw.Ref, want string) dw.CaptureTile {
		var out struct {
			Capture dw.CaptureResult
			Files   []struct {
				Asset dw.AssetID
				Path  string
				Bytes int
			}
		}
		bg.call("capture", dw.CaptureRequest{Kind: "window_content", Target: ref, MaxPixelWidth: 640, MaxPixelHeight: 640}, &out)
		if len(out.Capture.Tiles) != 1 || len(out.Files) != 1 {
			t.Fatal(out)
		}
		tile := out.Capture.Tiles[0]
		if tile.Target != ref || tile.DesktopFrame != "" || tile.ImageToDesktop != (dw.Transform2D{}) || tile.ImageToTarget.A <= 0 || tile.ImageToTarget.D <= 0 || tile.PixelWidth > 640 || tile.PixelHeight > 640 {
			t.Fatal("invalid target-local mapping", tile)
		}
		f, err := os.Open(out.Files[0].Path)
		if err != nil {
			t.Fatal(err)
		}
		image, err := png.Decode(f)
		f.Close()
		if err != nil {
			t.Fatal(err)
		}
		r, g, b, _ := image.At(image.Bounds().Dx()/2, image.Bounds().Dy()/2).RGBA()
		if (want == "red" && !(r > 40000 && g < 12000 && b < 12000)) || (want == "green" && !(g > 40000 && g > r*8/5 && g > b*8/5)) || (want == "blue" && !(b > 40000 && r < 12000 && g < 12000)) {
			t.Fatalf("%s expected %s, got %d %d %d", out.Files[0].Path, want, r, g, b)
		}
		if want == "green" {
			// A blue child sheet/popup must not be composited into parent content.
			for y := image.Bounds().Dy() / 4; y < image.Bounds().Dy()*3/4; y += 8 {
				for x := image.Bounds().Dx() / 4; x < image.Bounds().Dx()*3/4; x += 8 {
					r, g, b, _ := image.At(x, y).RGBA()
					if b > 40000 && b > r*3/2 && b > g*3/2 {
						t.Fatal("child window was composited into parent", out.Files[0].Path)
					}
				}
			}
		}
		images++
		pngBytes += out.Files[0].Bytes
		pixels += tile.PixelWidth * tile.PixelHeight
		return tile
	}
	first := capture(canvas, "red")
	// Actual shared Unicode input interleaves with background content changes/capture.
	humanCalls, humanBytes := 0, 0
	done := make(chan error, 1)
	finished := make(chan struct{})
	t.Cleanup(func() {
		select {
		case <-finished:
		case <-time.After(3 * time.Second):
		}
	})
	go func() {
		defer close(finished)
		for i, ch := range "Human-🙂-capture" {
			r, err := fg.c.Call(fg.ctx, "feature", fmt.Sprintf("human-%d", i), "act", struct{ Steps []dw.Step }{[]dw.Step{{ID: "type", Op: "keyboard.type_text", Target: dw.Target{Ref: editor.Ref}, TypeText: &dw.TypeText{Text: string(ch)}}}})
			humanCalls++
			humanBytes += len(host.Content(r).Content[0].Text)
			if err == nil && r.Error != nil {
				err = r.Error
			}
			if err != nil {
				done <- err
				return
			}
			time.Sleep(80 * time.Millisecond)
		}
		done <- nil
	}()
	invoke("更新画布")
	capture(canvas, "green")
	invoke("移动缩放")
	moved := capture(canvas, "green")
	if moved.ImageToTarget.A*float64(moved.PixelWidth) == first.ImageToTarget.A*float64(first.PixelWidth) {
		t.Fatal("resize mapping not refreshed")
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	fg.call("observe", seatReq, &after)
	if after.Seat.ForegroundWindow.Value == nil || *after.Seat.ForegroundWindow.Value != front.Ref || before.Seat.Pointer.Value == nil || after.Seat.Pointer.Value == nil || before.Seat.Pointer.Value.X != after.Seat.Pointer.Value.X || before.Seat.Pointer.Value.Y != after.Seat.Pointer.Value.Y {
		t.Fatal("capture disturbed shared seat", before.Seat, after.Seat)
	}
	invoke("打开弹窗")
	popup := findCapture(" Popup")
	capture(popup, "blue")
	capture(canvas, "green")
	invoke("打开Sheet")
	// A sheet has its own native Ref and is excluded from the parent's pixels.
	sheet := findCapture(" Sheet")
	capture(sheet, "blue")
	capture(canvas, "green")
	invoke("关闭Sheet")
	invoke("最小化画布")
	refuse(canvas, "window_not_visible", "window_unavailable")
	invoke("恢复画布")
	capture(canvas, "green")
	invoke("隐藏画布")
	refuse(canvas, "window_not_visible", "window_unavailable")
	invoke("恢复画布")
	capture(canvas, "green")
	invoke("重建画布")
	refuse(canvas, "ref_gone")
	next := findCapture(" Canvas")
	if next == canvas {
		t.Fatal("recreated window reused Ref")
	}
	capture(next, "red")
	events := fixtureEvents(t, logPath)
	seen := map[string]bool{}
	for _, event := range events {
		seen[event.Event] = true
		if event.Event == "key_down" || event.Event == "pointer" {
			t.Fatal("background received shared input", event)
		}
	}
	for _, event := range []string{"canvas_updated", "canvas_moved_resized", "canvas_minimized", "canvas_hidden", "canvas_recreated", "popup_opened", "sheet_opened", "sheet_closed"} {
		if !seen[event] {
			t.Fatal("missing app-owned event", event)
		}
	}
	human := fixtureEvents(t, frontLog)
	typed := ""
	for _, e := range human {
		if e.Event == "key_down" {
			typed += e.Value
		}
	}
	if typed != "Human-🙂-capture" {
		t.Fatal("human text", typed)
	}
	// Capture enablement is host-owned; an ordinary helper cannot ask to enable it.
	path := os.Getenv("DTW_NATIVE_HELPER")
	disabled, err := host.Start(bg.ctx, host.Options{Executable: path, InputPolicy: dw.InputNoShared})
	if err != nil {
		t.Fatal(err)
	}
	defer disabled.Close()
	if err = disabled.BeginTurn(bg.ctx, "feature"); err != nil {
		t.Fatal(err)
	}
	denied, err := disabled.Call(bg.ctx, "feature", "no-capture", "capture", dw.CaptureRequest{Kind: "visible_region"})
	if err != nil || denied.Error == nil || denied.Error.Code != "permission_denied" {
		t.Fatal(denied, err)
	}
	if err := bg.c.EndTurn(bg.ctx, "feature"); err != nil {
		t.Fatal(err)
	}
	stale, err := bg.c.Call(bg.ctx, "feature", "ended-capture", "capture", dw.CaptureRequest{Kind: "window_content", Target: next})
	bg.calls++
	bg.bytes += len(host.Content(stale).Content[0].Text)
	if err != nil || stale.Error == nil {
		t.Fatal("ended turn retained capture authority", stale, err)
	}
	t.Logf("F9 human simulator: calls=%d projected_text_bytes=%d; separate disabled-capture host probe=1", fg.calls+humanCalls, fg.bytes+humanBytes)
	t.Logf("F9 independent real-desktop evidence: calls=%d projected_text_bytes=%d images=%d png_bytes=%d pixels=%d; foreground Unicode input/pointer stable; native identity, covered fresh pixels, resize, popup/sheet, minimize/hide/recreate, host capture authority passed", bg.calls, bg.bytes, images, pngBytes, pixels)
}
