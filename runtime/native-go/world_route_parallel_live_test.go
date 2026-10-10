package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Two independent MCP/JS/native-helper Sessions use two self-owned AppKit
// bundles. The deliberate coordinator hold proves background overlap and
// physical-seat serialization instead of inferring either from fast timings.
func TestCoreObjectAutomaticRoutesAcrossTwoOwnedApps(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" || os.Getenv("DTW_POC_AUTO_ROUTE") != "1" {
		t.Skip("requires explicit two-App live route probe")
	}
	if os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires isolated core helper")
	}
	base := filepath.Join("..", "..", "bin", "DTWFullFixture.app", "Contents", "MacOS", "DTWFullFixture")
	if _, err := os.Stat(base); err != nil {
		t.Fatal("build owned AppKit fixture first: ", err)
	}
	coord := filepath.Join(t.TempDir(), "locks")
	if err := os.Mkdir(coord, 0700); err != nil {
		t.Fatal(err)
	}
	trace := filepath.Join(t.TempDir(), "route.jsonl")
	ctx, cancel := context.WithTimeout(context.Background(), 65*time.Second)
	defer cancel()
	type routeTarget struct {
		label, app, title, log string
		process                *exec.Cmd
		session                *mcp.ClientSession
	}
	targets := make([]routeTarget, 2)
	for i, label := range []string{"a", "b"} {
		name := "DTWFullFixture" + strings.ToUpper(label)
		bundle := filepath.Join(t.TempDir(), name+".app")
		binary := filepath.Join(bundle, "Contents", "MacOS", name)
		if err := os.MkdirAll(filepath.Dir(binary), 0700); err != nil {
			t.Fatal(err)
		}
		from, err := os.Open(base)
		if err != nil {
			t.Fatal(err)
		}
		to, err := os.OpenFile(binary, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0700)
		if err != nil {
			from.Close()
			t.Fatal(err)
		}
		_, copyErr := io.Copy(to, from)
		closeErr := to.Close()
		from.Close()
		if copyErr != nil || closeErr != nil {
			t.Fatalf("owned bundle copy: %v %v", copyErr, closeErr)
		}
		plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>%s</string><key>CFBundleIdentifier</key><string>dev.caelis.desktop-world.route-poc-%s</string><key>CFBundleName</key><string>%s</string><key>CFBundlePackageType</key><string>APPL</string><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>`, name, label, name)
		if err := os.WriteFile(filepath.Join(bundle, "Contents", "Info.plist"), []byte(plist), 0600); err != nil {
			t.Fatal(err)
		}
		if out, err := exec.Command("/usr/bin/codesign", "--force", "--sign", "-", bundle).CombinedOutput(); err != nil {
			t.Fatalf("sign isolated fixture: %v %s", err, out)
		}
		targets[i] = routeTarget{label: label, app: name, title: fmt.Sprintf("DTW Route POC %s %d", label, os.Getpid()), log: filepath.Join(t.TempDir(), "events.jsonl")}
		p := exec.Command(binary, "--title", targets[i].title, "--log", targets[i].log, "--background", "1")
		if err := p.Start(); err != nil {
			t.Fatal(err)
		}
		targets[i].process = p
		t.Cleanup(func() { _ = p.Process.Kill(); _ = p.Wait() })
	}
	for i := range targets {
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) && len(ownedRows(t, targets[i].log)) == 0 {
			time.Sleep(20 * time.Millisecond)
		}
		if rows := ownedRows(t, targets[i].log); len(rows) == 0 || rows[0]["event"] != "ready" {
			t.Fatalf("%s fixture not ready: %+v", targets[i].label, rows)
		}
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_LEGACY_OUTPUT=0", "DTW_POC_COORD_DIR="+coord, "DTW_POC_COORD_TRACE="+trace, "DTW_POC_COORD_HOLD_MS=200", "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: "route-" + targets[i].label, Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		targets[i].session = session
		defer session.Close()
	}
	type answer struct {
		text string
		err  error
	}
	call := func(target routeTarget, id, code string) answer {
		result, err := target.session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "exec", "execution_id": id, "code": code}})
		if err != nil {
			return answer{err: err}
		}
		if result == nil || len(result.Content) != 1 || result.StructuredContent != nil {
			return answer{err: fmt.Errorf("unexpected MCP shape: %+v", result)}
		}
		text := result.Content[0].(*mcp.TextContent).Text
		if result.IsError {
			return answer{text: text, err: fmt.Errorf("tool error")}
		}
		return answer{text: text}
	}
	for _, target := range targets {
		code := `const app=await dtw.app(` + string(mustJSON(target.app)) + `);const win=await app.window(` + string(mustJSON(target.title)) + `);state.field=await win.one({name:'POC text'});state.submit=await win.one({name:'POC submit'});state.canvas=await win.one({name:'POC Canvas'});print(state.submit);print(state.canvas);`
		got := call(target, "route-setup-"+target.label, code)
		if got.err != nil || !strings.Contains(got.text, "POC submit") {
			t.Fatalf("%s setup: %v %s", target.label, got.err, got.text)
		}
	}
	parallel := func(kind string, code func(routeTarget) string) [2]answer {
		start := make(chan struct{})
		var out [2]answer
		done := make(chan int, 2)
		for i, target := range targets {
			go func(i int, target routeTarget) {
				<-start
				out[i] = call(target, "route-"+kind+"-"+target.label, code(target))
				done <- i
			}(i, target)
		}
		close(start)
		<-done
		<-done
		return out
	}
	background := parallel("background", func(target routeTarget) string {
		return `await state.field.setValue('` + target.label + `-中文🙂');`
	})
	for i, got := range background {
		t.Logf("%s background: %s", targets[i].label, got.text)
		if got.err != nil || !strings.Contains(got.text, "setValue: verified") || !waitOwnedEvent(t, targets[i].log, 0, "text", targets[i].label+"-中文🙂", 500*time.Millisecond) {
			t.Fatalf("%s background not confirmed: %v %s", targets[i].label, got.err, got.text)
		}
	}
	beforeInput := [2]int{len(ownedRows(t, targets[0].log)), len(ownedRows(t, targets[1].log))}
	backgroundInput := parallel("background-input", func(routeTarget) string {
		return `await state.submit.click();`
	})
	for i, got := range backgroundInput {
		t.Logf("%s background physical input: %s", targets[i].label, got.text)
		if got.err != nil || !strings.Contains(got.text, ".click: dispatched; effect unverified via background") ||
			!waitOwnedEvent(t, targets[i].log, beforeInput[i], "submit", targets[i].label+"-中文🙂", 500*time.Millisecond) {
			t.Fatalf("%s background click not confirmed: %v %s", targets[i].label, got.err, got.text)
		}
	}
	before := [2]int{len(ownedRows(t, targets[0].log)), len(ownedRows(t, targets[1].log))}
	frontBefore := frontmostPID()
	px, py, err := pointerLocation()
	if frontBefore == 0 || err != nil {
		t.Fatalf("initial seat metadata unavailable: pid=%d pointer=%v", frontBefore, err)
	}
	physicalMax := 0.0
	maxAt := time.Duration(0)
	maxX, maxY := px, py
	sampleStart := time.Now()
	stopSample := make(chan struct{})
	sampleDone := make(chan struct{})
	go func() {
		defer close(sampleDone)
		for {
			select {
			case <-stopSample:
				return
			default:
			}
			if x, y, err := pointerLocation(); err == nil {
				if distance := math.Hypot(x-px, y-py); distance > physicalMax {
					physicalMax, maxAt, maxX, maxY = distance, time.Since(sampleStart), x, y
				}
			}
			time.Sleep(time.Millisecond)
		}
	}()
	foreground := parallel("foreground", func(target routeTarget) string { return `await state.canvas.click();` })
	close(stopSample)
	<-sampleDone
	for i, got := range foreground {
		t.Logf("%s foreground: %s", targets[i].label, got.text)
		if got.err != nil || !strings.Contains(got.text, "foreground restoration: restored") || !waitOwnedEvent(t, targets[i].log, before[i], "click", "1", 500*time.Millisecond) {
			t.Fatalf("%s foreground not confirmed: %v %s", targets[i].label, got.err, got.text)
		}
	}
	if os.Getenv("DTW_POC_VIRTUAL") == "1" {
		t.Logf("parallel foreground routes, in-flight physical pointer displacement: %.1f px at %s; base=(%.1f,%.1f), peak=(%.1f,%.1f)", physicalMax, maxAt, px, py, maxX, maxY)
		if physicalMax > 2 {
			t.Fatalf("parallel virtual input moved physical pointer %.1f px", physicalMax)
		}
	}
	qx, qy, err := pointerLocation()
	if err != nil {
		t.Fatal(err)
	}
	if frontAfter := frontmostPID(); frontAfter != frontBefore || math.Hypot(qx-px, qy-py) > 2 {
		t.Fatalf("shared seat did not return: front PID %d -> %d, pointer displacement %.1f px", frontBefore, frontAfter, math.Hypot(qx-px, qy-py))
	}
	t.Logf("shared seat restored: front PID matched; pointer displacement %.1f px", math.Hypot(qx-px, qy-py))
	rows := routeTrace(t, trace)
	for _, kind := range []string{"background", "background-input", "foreground"} {
		a, b := routeSpan(rows, "route-"+kind+"-a-native-"), routeSpan(rows, "route-"+kind+"-b-native-")
		if a.acquired == 0 || a.released == 0 || b.acquired == 0 || b.released == 0 || a.app == "" || b.app == "" || a.app == b.app {
			t.Fatalf("%s distinct trace incomplete: a=%+v b=%+v", kind, a, b)
		}
		overlap := a.acquired < b.released && b.acquired < a.released
		if (kind == "background" || kind == "background-input") && (!overlap || a.foreground || b.foreground) {
			t.Fatalf("background actions did not overlap: a=%+v b=%+v", a, b)
		}
		t.Logf("%s overlap=%t a=%dms b=%dms", kind, overlap, (a.released-a.acquired)/1e6, (b.released-b.acquired)/1e6)
	}
	frontLeases := func() (waiting, acquired int, owners map[int]bool) {
		rows := routeTrace(t, trace)
		sort.Slice(rows, func(i, j int) bool { return rows[i].Time < rows[j].Time })
		active := map[int]bool{}
		owners = map[int]bool{}
		for _, row := range rows {
			switch row.Event {
			case "front_waiting":
				waiting++
			case "front_acquired":
				if len(active) != 0 {
					t.Fatalf("physical foreground leases overlapped: active=%v new=%d", active, row.PID)
				}
				active[row.PID] = true
				owners[row.PID] = true
				acquired++
			case "front_released":
				if !active[row.PID] {
					t.Fatalf("unmatched foreground release by %d", row.PID)
				}
				delete(active, row.PID)
			}
		}
		return
	}
	_, frontAcquired, owners := frontLeases()
	if frontAcquired != 2 || len(owners) != 2 {
		t.Fatalf("two distinct native helpers did not acquire foreground serially: acquisitions=%d owners=%v", frontAcquired, owners)
	}
	// Cancelling one Session while it waits for the shared foreground must not
	// post its click or alter the already running peer's drag.
	beforeA := len(ownedRows(t, targets[0].log))
	beforeB := len(ownedRows(t, targets[1].log))
	aDone := make(chan answer, 1)
	bDone := make(chan answer, 1)
	go func() {
		aDone <- call(targets[0], "route-cancel-a", `await state.canvas.dragTo(state.canvas,{from:{u:0.2,v:0.5},to:{u:0.7,v:0.5},durationMs:250});`)
	}()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		_, count, _ := frontLeases()
		if count >= frontAcquired+1 {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if _, count, _ := frontLeases(); count != frontAcquired+1 {
		t.Fatal("Session A did not acquire the native foreground lock")
	}
	go func() { bDone <- call(targets[1], "route-cancel-b", `await state.canvas.click();`) }()
	deadline = time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		waiting, _, _ := frontLeases()
		if waiting >= frontAcquired+2 {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if waiting, _, _ := frontLeases(); waiting < frontAcquired+2 {
		t.Fatal("Session B did not wait for native foreground lock")
	}
	if _, err := targets[1].session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{"operation": "cancel", "execution_id": "route-cancel-b"}}); err != nil {
		t.Fatal(err)
	}
	a, b := <-aDone, <-bDone
	t.Logf("cancel results: A=%+v B=%+v", a, b)
	dropped := false
	for _, row := range ownedRows(t, targets[0].log)[beforeA:] {
		if row["event"] == "drop" {
			var dx, dy float64
			if _, err := fmt.Sscanf(fmt.Sprint(row["value"]), "%f,%f", &dx, &dy); err == nil && dx > 100 && math.Abs(dy) < 10 {
				dropped = true
			}
		}
	}
	if a.err != nil || !strings.Contains(a.text, "W1/N1.dragTo: dispatched") || !dropped {
		t.Fatalf("Session A drag failed after B cancellation: %+v; rows=%+v", a, ownedRows(t, targets[0].log)[beforeA:])
	}
	if b.err == nil || !strings.Contains(b.text, "cancel") {
		t.Fatalf("Session B did not report cancellation: %+v", b)
	}
	if waitOwnedEvent(t, targets[1].log, beforeB, "click", "1", 150*time.Millisecond) {
		t.Fatalf("cancelled Session B dispatched an input action: rows=%+v", ownedRows(t, targets[1].log)[beforeB:])
	}
	if _, count, _ := frontLeases(); count != frontAcquired+1 {
		t.Fatalf("cancelled Session B acquired foreground: acquisitions=%d want=%d", count, frontAcquired+1)
	}
	t.Log("cancelled waiting Session B: no App callback or foreground acquisition; Session A drag completed")
	// Both Sessions now address App A. The same-app lock must serialize
	// conflicting background writes, while each Session keeps its own object.
	alias := `const app=await dtw.app(` + string(mustJSON(targets[0].app)) + `);const win=await app.window(` + string(mustJSON(targets[0].title)) + `);state.sameField=await win.one({name:'POC text'});print(state.sameField);`
	if got := call(targets[1], "route-same-setup-b", alias); got.err != nil || !strings.Contains(got.text, "POC text") {
		t.Fatalf("Session B same-app binding failed: %+v", got)
	}
	sameStart := make(chan struct{})
	sameDone := make(chan answer, 2)
	go func() {
		<-sameStart
		sameDone <- call(targets[0], "route-same-a", `await state.field.setValue('same-a');`)
	}()
	go func() {
		<-sameStart
		sameDone <- call(targets[1], "route-same-b", `await state.sameField.setValue('same-b');`)
	}()
	close(sameStart)
	for range 2 {
		got := <-sameDone
		if got.err != nil || !strings.Contains(got.text, "setValue: verified") {
			t.Fatalf("same-app background write failed: %+v", got)
		}
	}
	sameRows := routeTrace(t, trace)
	left, right := routeSpan(sameRows, "route-same-a-native-"), routeSpan(sameRows, "route-same-b-native-")
	if left.app == "" || right.app == "" || left.app != right.app || left.foreground || right.foreground ||
		left.acquired == 0 || right.acquired == 0 || left.released == 0 || right.released == 0 ||
		left.acquired < right.released && right.acquired < left.released {
		t.Fatalf("same-app background writes overlapped or lost identity: a=%+v b=%+v", left, right)
	}
	winner := "same-a"
	if right.released > left.released {
		winner = "same-b"
	}
	readback := call(targets[0], "route-same-read", `const value=await state.field.read('value');if(value?.status!=='known')throw Error('value unknown');print(value.value);`)
	if readback.err != nil || !strings.Contains(readback.text, "route-same-read · "+winner) {
		t.Fatalf("same-app final AX value did not match last serialized write %s: %+v", winner, readback)
	}
	t.Logf("same-app background overlap=false a=%dms b=%dms; final AX value matches last writer %s", (left.released-left.acquired)/1e6, (right.released-right.acquired)/1e6, winner)
}

type routeTraceRow struct {
	Event      string `json:"event"`
	ID         string `json:"native_id"`
	App        string `json:"app"`
	Foreground bool   `json:"foreground"`
	PID        int    `json:"pid"`
	Time       int64  `json:"time"`
}

func routeTrace(t *testing.T, path string) []routeTraceRow {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var rows []routeTraceRow
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var row routeTraceRow
		if err := json.Unmarshal([]byte(line), &row); err != nil {
			t.Fatal(err)
		}
		rows = append(rows, row)
	}
	return rows
}

type routeSpanData struct {
	app                string
	foreground         bool
	acquired, released int64
}

func routeSpan(rows []routeTraceRow, prefix string) routeSpanData {
	var span routeSpanData
	for _, row := range rows {
		if !strings.HasPrefix(row.ID, prefix) {
			continue
		}
		span.app, span.foreground = row.App, row.Foreground
		switch row.Event {
		case "acquired":
			span.acquired = row.Time
		case "released":
			span.released = row.Time
		}
	}
	return span
}
