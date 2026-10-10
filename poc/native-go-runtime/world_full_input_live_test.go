package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Opt-in current-chain probe. The target is a disposable AppKit process
// launched without activation; every action is addressed to its observed AX
// window/control, and the process is closed with the test.
func TestCoreObjectFullInputOnOwnedAppKit(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	if os.Getenv("DTW_POC_FULL_INPUT") != "1" {
		t.Skip("requires explicit owned-App input probe")
	}
	if os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires isolated core helper")
	}
	fixture := filepath.Join("..", "..", "bin", "DTWFullFixture.app", "Contents", "MacOS", "DTWFullFixture")
	if _, err := os.Stat(fixture); err != nil {
		t.Fatal("build the owned AppKit fixture first: ", err)
	}
	title := fmt.Sprintf("DTW Core Input POC %d", os.Getpid())
	logPath := filepath.Join(t.TempDir(), "owned.jsonl")
	app := exec.Command(fixture, "--title", title, "--log", logPath, "--background", "1")
	if err := app.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = app.Process.Kill(); _ = app.Wait() })
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if rows := ownedRows(t, logPath); len(rows) > 0 && rows[0]["event"] == "ready" {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}
	if rows := ownedRows(t, logPath); len(rows) == 0 || rows[0]["event"] != "ready" {
		t.Fatal("owned AppKit fixture did not become ready")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "owned-input", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	call := func(id, code string) string {
		t.Helper()
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "exec", "execution_id": id, "code": code,
		}})
		if err != nil || res == nil || len(res.Content) != 1 || res.StructuredContent != nil {
			t.Fatalf("%s MCP result: %v %+v", id, err, res)
		}
		line := res.Content[0].(*mcp.TextContent).Text
		t.Logf("exec %s: %s", id, line)
		if res.IsError {
			original, queryErr := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
				"operation": "result", "execution_id": id, "detail": "full",
			}})
			if queryErr == nil && original != nil && len(original.Content) == 1 {
				t.Logf("original %s: %s", id, original.Content[0].(*mcp.TextContent).Text)
			}
		}
		if res.IsError || !strings.HasPrefix(line, id+" · ") {
			t.Fatalf("%s failed or detached output: %q", id, line)
		}
		return line
	}
	setup := `const app=await dtw.app('DTWFullFixture');const win=await app.window(` + string(mustJSON(title)) + `);state.app=app;state.win=win;state.field=await win.one({name:'POC text'});state.multi=await win.one({name:'POC multiline'});state.canvas=await win.one({name:'POC Canvas'});state.submit=await win.one({name:'POC submit'});state.dialog=await win.one({name:'POC dialog'});print(state.field);print(state.multi);print(state.canvas);print(state.submit);print(state.dialog);`
	line := call("owned-setup", setup)
	if !strings.Contains(line, "POC text") || !strings.Contains(line, "POC Canvas") || !strings.Contains(line, "POC submit") {
		t.Fatalf("owned targets unavailable: %q", line)
	}
	line = call("owned-semantic", `await state.field.setValue('DTW-CORE-中文🙂');print('setValue requested');`)
	if !strings.Contains(line, "via semantic") || !strings.Contains(line, "verified") {
		t.Fatalf("semantic background route unverified: %q", line)
	}
	if !ownedEventValue(ownedRows(t, logPath), "text", "DTW-CORE-中文🙂") {
		t.Fatal("owned AppKit text callback did not confirm semantic write")
	}
	if os.Getenv("DTW_POC_INPUT_CASE") == "limits" {
		before := len(ownedRows(t, logPath))
		px, py, err := pointerLocation()
		if err != nil {
			t.Fatal(err)
		}
		front := frontmostPID()
		for _, trial := range []struct{ id, code string }{
			{"owned-long-text", `await state.field.typeText('x'.repeat(257));`},
			{"owned-long-drag", `await state.canvas.dragTo(state.canvas,{durationMs:501});`},
		} {
			res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": trial.id, "code": trial.code}})
			if err != nil || res == nil || !res.IsError || len(res.Content) != 1 {
				t.Fatalf("%s should reject before input: %v %+v", trial.id, err, res)
			}
			line := res.Content[0].(*mcp.TextContent).Text
			t.Logf("exec %s: %s", trial.id, line)
			if !strings.Contains(line, "input_burst_limit") {
				t.Fatalf("%s wrong rejection: %q", trial.id, line)
			}
			original, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "result", "execution_id": trial.id, "detail": "full"}})
			if err != nil || original == nil || len(original.Content) != 1 {
				t.Fatalf("%s original receipt missing: %v %+v", trial.id, err, original)
			}
			full := original.Content[0].(*mcp.TextContent).Text
			if !strings.Contains(full, `"delivery": "none"`) || !strings.Contains(full, `"restoration": "not_borrowed"`) {
				t.Fatalf("%s did not prove pre-borrow refusal: %s", trial.id, full)
			}
		}
		qx, qy, err := pointerLocation()
		if err != nil || math.Hypot(qx-px, qy-py) > 2 || frontmostPID() != front || len(ownedRows(t, logPath)) != before {
			t.Fatalf("rejected input changed target or seat: pointer=%v/%v front=%d/%d rows=%d/%d err=%v", px, py, front, frontmostPID(), before, len(ownedRows(t, logPath)), err)
		}
		t.Log("overlong keyboard and drag rejected before borrowing; App callbacks and seat unchanged")
		return
	}
	frontBefore := frontmostPID()
	if frontBefore == 0 {
		t.Fatal("frontmost PID unavailable before behavior routing")
	}
	beforeAuto := len(ownedRows(t, logPath))
	line = call("owned-auto-background", `await state.submit.activate();`)
	if !strings.Contains(line, "W1/B1.activate: dispatched; effect unverified via semantic") || strings.Contains(line, "foreground restoration") ||
		!waitOwnedEvent(t, logPath, beforeAuto, "submit", "DTW-CORE-中文🙂", 500*time.Millisecond) {
		t.Fatalf("automatic semantic activation not confirmed: %q", line)
	}
	if got := frontmostPID(); got != frontBefore {
		t.Fatalf("semantic activation changed frontmost PID: before=%d after=%d", frontBefore, got)
	}
	beforeAuto = len(ownedRows(t, logPath))
	px, py, err := pointerLocation()
	if err != nil {
		t.Fatal(err)
	}
	line = call("owned-auto-foreground", `await state.canvas.activate({u:0.2,v:0.5});`)
	if !strings.Contains(line, "W1/N1.activate: dispatched; effect unverified via foreground_transaction") || !strings.Contains(line, "foreground restoration: restored") ||
		!waitOwnedEvent(t, logPath, beforeAuto, "click", "1", 500*time.Millisecond) {
		t.Fatalf("automatic foreground activation not confirmed: %q", line)
	}
	qx, qy, err := pointerLocation()
	if err != nil {
		t.Fatal(err)
	}
	if distance := math.Hypot(qx-px, qy-py); distance > 2 {
		t.Fatalf("pointer did not return after foreground activation: displacement=%.1f px", distance)
	} else {
		t.Logf("independent pointer restoration: displacement=%.1f px", distance)
	}
	if got := frontmostPID(); got != frontBefore {
		t.Fatalf("foreground activation did not return frontmost PID: before=%d after=%d", frontBefore, got)
	}
	t.Log("independent frontmost PID restoration: matched")
	if os.Getenv("DTW_POC_INPUT_CASE") == "auto" {
		return
	}
	checkMenu := func() {
		before := len(ownedRows(t, logPath))
		line := call("owned-context-menu", `const appRef=dtw.ref(state.app.id);await dtw.act({steps:[{id:state.canvas.id+'.right',op:'pointer.click',target:{id:state.canvas.id},click:{button:'right',count:1}},{id:'menu.bind',op:'bind',bind:{name:'menu',require_unique:true,locator:{within:appRef,name_equals:'POC menu commit',role:'menu_item',max_depth:12}}},{id:'menu.invoke',op:'invoke',target:{bound:'menu'}}]});`)
		if !strings.Contains(line, "W1/N1.right") || !strings.Contains(line, "menu.invoke") || !strings.Contains(line, "foreground restoration: restored") {
			t.Fatalf("context-menu route unverified: %q", line)
		}
		if !waitOwnedEvent(t, logPath, before, "menu_commit", "", 500*time.Millisecond) {
			t.Fatalf("owned AppKit menu callback did not confirm the right-click path: %+v", ownedRows(t, logPath)[before:])
		}
	}
	if os.Getenv("DTW_POC_INPUT_CASE") == "menu" {
		checkMenu()
		return
	}
	checkDialog := func() {
		before := len(ownedRows(t, logPath))
		line := call("owned-dialog", `const windowRef=dtw.ref(state.win.id);await dtw.act({steps:[{id:state.dialog.id+'.click',op:'pointer.click',target:{id:state.dialog.id},click:{button:'left',count:1}},{id:'dialog.bindText',op:'bind',bind:{name:'dialogtext',require_unique:true,locator:{within:windowRef,name_equals:'POC dialog text',role:'text_field',max_depth:12}}},{id:'dialog.focusText',op:'pointer.click',target:{bound:'dialogtext'},click:{button:'left',count:1}},{id:'dialog.typeText',op:'keyboard.type_text',target:{bound:'dialogtext'},type_text:{text:'CONFIRMED-中文'}},{id:'dialog.bindConfirm',op:'bind',bind:{name:'confirm',require_unique:true,locator:{within:windowRef,name_equals:'POC confirm',role:'button',max_depth:12}}},{id:'dialog.confirm',op:'invoke',target:{bound:'confirm'}}]});`)
		if !strings.Contains(line, "W1/B2.click") || !strings.Contains(line, "dialog.typeText") || !strings.Contains(line, "dialog.confirm") || !strings.Contains(line, "foreground restoration: restored") {
			t.Fatalf("dialog route unverified: %q", line)
		}
		newRows := ownedRows(t, logPath)[before:]
		if !waitOwnedEvent(t, logPath, before, "dialog_text", "CONFIRMED-中文", 500*time.Millisecond) || !waitOwnedEvent(t, logPath, before, "dialog_confirm", "", 500*time.Millisecond) {
			t.Fatalf("owned AppKit dialog callback did not confirm text and action: %+v", newRows)
		}
	}
	if os.Getenv("DTW_POC_INPUT_CASE") == "dialog" {
		checkDialog()
		return
	}
	line = call("owned-move", `await state.canvas.move({u:0.2,v:0.5});`)
	if !strings.Contains(line, "via foreground_transaction") || !strings.Contains(line, "foreground restoration: restored") {
		t.Fatalf("short foreground pointer route unverified: %q", line)
	}
	if !ownedEventValue(ownedRows(t, logPath), "move", "") {
		t.Fatal("owned AppKit canvas did not receive the pointer move")
	}
	before := len(ownedRows(t, logPath))
	line = call("owned-pointer-batch", `await dtw.transaction(tx=>{tx.click(state.canvas);tx.click(state.canvas,{count:2});tx.click(state.canvas,{button:'middle'});tx.scroll(state.canvas,{dy:3});tx.scroll(state.canvas,{dx:3,dy:0});});`)
	for _, marker := range []string{"W1/N1.click#1", "W1/N1.click#2", "W1/N1.click#3", "W1/N1.scroll#4", "W1/N1.scroll#5", "foreground_transaction", "foreground restoration: restored"} {
		if !strings.Contains(line, marker) {
			t.Fatalf("pointer batch result omits %q: %q", marker, line)
		}
	}
	newRows := ownedRows(t, logPath)[before:]
	for _, event := range []struct{ name, value string }{{"click", "1"}, {"double", ""}, {"middle", "2"}} {
		if !ownedEventValue(newRows, event.name, event.value) {
			t.Fatalf("owned AppKit missing %s/%s callback: %+v", event.name, event.value, newRows)
		}
	}
	var vertical, horizontal bool
	for _, row := range newRows {
		if row["event"] != "scroll" {
			continue
		}
		var dx, dy float64
		if _, err := fmt.Sscanf(fmt.Sprint(row["value"]), "%f,%f", &dx, &dy); err != nil {
			t.Fatal(err)
		}
		vertical = vertical || dx == 0 && dy != 0
		horizontal = horizontal || dx != 0 && dy == 0
	}
	if !vertical || !horizontal {
		t.Fatalf("owned AppKit missing wheel axes: vertical=%t horizontal=%t", vertical, horizontal)
	}
	before = len(ownedRows(t, logPath))
	line = call("owned-drag", `await state.canvas.dragTo(state.canvas,{from:{u:0.2,v:0.5},to:{u:0.7,v:0.5},durationMs:250});`)
	if !strings.Contains(line, "W1/N1.dragTo: dispatched") || !strings.Contains(line, "foreground restoration: restored") {
		t.Fatalf("positioned foreground drag unverified: %q", line)
	}
	var displaced bool
	for _, row := range ownedRows(t, logPath)[before:] {
		if row["event"] != "drop" {
			continue
		}
		var dx, dy float64
		if _, err := fmt.Sscanf(fmt.Sprint(row["value"]), "%f,%f", &dx, &dy); err != nil {
			t.Fatal(err)
		}
		displaced = displaced || dx > 100 && dy > -10 && dy < 10
	}
	if !displaced {
		t.Fatal("owned AppKit canvas did not confirm the positioned drag/drop")
	}
	before = len(ownedRows(t, logPath))
	line = call("owned-keyboard-submit", `await dtw.transaction(tx=>{tx.click(state.field);tx.press(state.field,'A',['primary']);tx.typeText(state.field,'Typed-中文🙂');tx.click(state.submit);});`)
	for _, marker := range []string{"W1/T1.click#1", "W1/T1.press#2", "W1/T1.typeText#3", "W1/B1.click#4", "foreground_transaction", "foreground restoration: restored"} {
		if !strings.Contains(line, marker) {
			t.Fatalf("keyboard batch result omits %q: %q", marker, line)
		}
	}
	newRows = ownedRows(t, logPath)[before:]
	if !ownedEventValue(newRows, "text", "Typed-中文🙂") || !ownedEventValue(newRows, "submit", "Typed-中文🙂") {
		t.Fatalf("owned AppKit did not confirm keyboard text and submit: %+v", newRows)
	}
	before = len(ownedRows(t, logPath))
	line = call("owned-shortcuts", `await dtw.transaction(tx=>{tx.click(state.field);tx.press(state.field,'A',['primary']);tx.typeText(state.field,'replaced');tx.press(state.field,'Left',['shift']);tx.press(state.field,'Backspace');});`)
	for _, marker := range []string{"W1/T1.press#2", "W1/T1.press#4", "W1/T1.press#5", "foreground restoration: restored"} {
		if !strings.Contains(line, marker) {
			t.Fatalf("shortcut result omits %q: %q", marker, line)
		}
	}
	if !ownedEventValue(ownedRows(t, logPath)[before:], "text", "replace") {
		t.Fatal("owned AppKit field did not confirm Shift+Left/Backspace result")
	}
	before = len(ownedRows(t, logPath))
	line = call("owned-multiline", `await dtw.transaction(tx=>{tx.click(state.multi);tx.typeText(state.multi,'Line1\n中文🙂\nLine3');});`)
	if !strings.Contains(line, "W1/T2.typeText#2") || !strings.Contains(line, "foreground restoration: restored") {
		t.Fatalf("multiline text route unverified: %q", line)
	}
	if !ownedEventValue(ownedRows(t, logPath)[before:], "multiline", "Line1\n中文🙂\nLine3") {
		t.Fatal("owned AppKit text view did not confirm exact multiline input")
	}
	before = len(ownedRows(t, logPath))
	line = call("owned-window-key", `await state.win.press('Tab');`)
	if !strings.Contains(line, "W1.press: dispatched") || !strings.Contains(line, "foreground restoration: restored") {
		t.Fatalf("window-targeted key route unverified: %q", line)
	}
	if !ownedEventValue(ownedRows(t, logPath)[before:], "multiline", "Line1\n中文🙂\nLine3\t") {
		t.Fatal("owned AppKit did not receive window-targeted Tab")
	}
	checkMenu()
	checkDialog()
}

func ownedRows(t *testing.T, path string) []map[string]any {
	t.Helper()
	f, err := os.Open(path)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	var rows []map[string]any
	scan := bufio.NewScanner(f)
	for scan.Scan() {
		var row map[string]any
		if err := json.Unmarshal(scan.Bytes(), &row); err != nil {
			t.Fatal(err)
		}
		rows = append(rows, row)
	}
	if err := scan.Err(); err != nil {
		t.Fatal(err)
	}
	return rows
}

func ownedEventValue(rows []map[string]any, event, value string) bool {
	for _, row := range rows {
		if row["event"] == event && row["value"] == value {
			return true
		}
	}
	return false
}

func waitOwnedEvent(t *testing.T, path string, from int, event, value string, timeout time.Duration) bool {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for {
		rows := ownedRows(t, path)
		if len(rows) >= from && ownedEventValue(rows[from:], event, value) {
			return true
		}
		if time.Now().After(deadline) {
			return false
		}
		time.Sleep(20 * time.Millisecond)
	}
}
