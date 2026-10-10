package main

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Uses a separate Chrome profile and a loopback-only page. DTW performs all
// UI actions; the page's own callback is the independent effect readback.
func TestSelectedChromeAutomaticInputOnLocalPage(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	if os.Getenv("DTW_POC_CHROME_LIVE") != "1" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires opt-in isolated Chrome and native helper")
	}
	chrome := "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
	if _, err := os.Stat(chrome); err != nil {
		t.Fatal(err)
	}
	title := fmt.Sprintf("DTW Chrome CUA POC %d", os.Getpid())
	var mu sync.Mutex
	var received []string
	loaded := make(chan struct{})
	var loadOnce sync.Once
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/event" && r.Method == http.MethodPost {
			data, _ := io.ReadAll(io.LimitReader(r.Body, 256))
			mu.Lock()
			received = append(received, string(data))
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		fmt.Fprintf(w, `<!doctype html><html><head><title>%s</title></head><body>
<button aria-label="DTW test button" id="button">DTW test button</button>
<input aria-label="DTW test input" id="input" autocomplete="off">
<div role="status" id="status">ready</div><div style="height:1800px"></div>
<script>
const post=(kind,value)=>fetch('/event',{method:'POST',body:JSON.stringify({kind,value})});
document.getElementById('button').addEventListener('click',()=>{document.getElementById('status').textContent='clicked';post('click','button')});
document.getElementById('button').addEventListener('mousemove',()=>post('move','button'));
document.getElementById('input').addEventListener('input',e=>{document.getElementById('status').textContent='typed';post('input',e.target.value)});
document.getElementById('input').addEventListener('keydown',e=>{if(e.key==='Enter')post('key','Enter')});
document.addEventListener('wheel',e=>post('wheel',String(Math.sign(e.deltaY))));
post('ready','page');
</script></body></html>`, title)
		loadOnce.Do(func() { close(loaded) })
	}))
	defer server.Close()
	profile := filepath.Join(t.TempDir(), "profile")
	app := exec.Command(chrome, "--user-data-dir="+profile, "--no-first-run", "--no-default-browser-check", "--disable-sync", "--new-window", server.URL+"/")
	if err := app.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = app.Process.Kill(); _ = app.Wait() })
	select {
	case <-loaded:
	case <-time.After(12 * time.Second):
		t.Fatal("isolated Chrome did not load the loopback test page")
	}
	readyDeadline := time.Now().Add(5 * time.Second)
	pageReady := false
	for time.Now().Before(readyDeadline) {
		mu.Lock()
		for _, event := range received {
			if event == `{"kind":"ready","value":"page"}` {
				pageReady = true
				break
			}
		}
		mu.Unlock()
		if pageReady {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !pageReady {
		t.Fatal("isolated Chrome page listeners did not signal readiness")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
	child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
	client := mcp.NewClient(&mcp.Implementation{Name: "poc-chrome-local-page", Version: "1"}, nil)
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
			t.Fatalf("%s result: %v %+v", id, err, res)
		}
		line := res.Content[0].(*mcp.TextContent).Text
		t.Logf("%s: %s", id, line)
		if res.IsError {
			t.Fatalf("%s failed; inspect original execution ID before any new input", id)
		}
		return line
	}
	assertOriginalRoute := func(id, route string) {
		t.Helper()
		res, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": "result", "execution_id": id, "detail": "full",
		}})
		if err != nil || res == nil || res.IsError || len(res.Content) != 1 {
			t.Fatalf("%s original receipt unavailable: %v %+v", id, err, res)
		}
		full := res.Content[0].(*mcp.TextContent).Text
		if !strings.Contains(full, `"channel": "`+route+`"`) {
			t.Fatalf("%s original receipt did not record %s: %s", id, route, full)
		}
	}
	windowTitle := title + " - Google Chrome"
	setup := `const app=await dtw.app('Chrome',{window:` + string(mustJSON(windowTitle)) + `});
const win=await app.window(` + string(mustJSON(windowTitle)) + `);state.win=win;
state.button=await win.one({name:'DTW test button'});
state.input=await win.one({name:'DTW test input'});
print(state.button);print(state.input);`
	line := call("chrome-local-setup", setup)
	if !strings.Contains(line, "DTW test button") || !strings.Contains(line, "DTW test input") {
		t.Fatal("Chrome test controls not located in exact window")
	}
	line = call("chrome-local-button-click", `await state.button.click();`)
	if !strings.Contains(line, ".click:") {
		t.Fatalf("click result cannot be tied to script target: %s", line)
	}
	assertOriginalRoute("chrome-local-button-click", "targeted_background")
	waitEvent := func(expected string) {
		t.Helper()
		deadline := time.Now().Add(1500 * time.Millisecond)
		for time.Now().Before(deadline) {
			mu.Lock()
			seen := append([]string(nil), received...)
			mu.Unlock()
			for _, event := range seen {
				if event == expected {
					return
				}
			}
			time.Sleep(30 * time.Millisecond)
		}
		t.Fatalf("Chrome event missing after dispatch; no replay. expected=%s", expected)
	}
	waitEvent(`{"kind":"click","value":"button"}`)
	t.Log("independent Chrome callback confirms the test button click")
	moveEvent := `{"kind":"move","value":"button"}`
	countMove := func() int {
		mu.Lock()
		defer mu.Unlock()
		n := 0
		for _, event := range received {
			if event == moveEvent {
				n++
			}
		}
		return n
	}
	beforeMove := countMove()
	line = call("chrome-local-pointer-move", `await state.button.move({u:0.65,v:0.5});`)
	if !strings.Contains(line, ".move:") {
		t.Fatalf("pointer move result cannot be tied to script target: %s", line)
	}
	assertOriginalRoute("chrome-local-pointer-move", "targeted_background")
	deadline := time.Now().Add(1500 * time.Millisecond)
	for time.Now().Before(deadline) && countMove() <= beforeMove {
		time.Sleep(30 * time.Millisecond)
	}
	if got := countMove(); got != beforeMove+1 {
		t.Fatalf("background pointer move callback count changed %d -> %d; want exactly one new event", beforeMove, got)
	}
	t.Log("independent Chrome callback confirms background pointer move")
	line = call("chrome-local-input-focus", `await state.input.click();`)
	if !strings.Contains(line, ".click:") {
		t.Fatalf("input click result cannot be tied to script target: %s", line)
	}
	line = call("chrome-local-type", `await state.input.typeText('DTW-Chrome-中文🙂');`)
	if !strings.Contains(line, ".typeText:") {
		t.Fatalf("keyboard result cannot be tied to script target: %s", line)
	}
	assertOriginalRoute("chrome-local-type", "targeted_background")
	waitEvent(`{"kind":"input","value":"DTW-Chrome-中文🙂"}`)
	t.Log("independent Chrome callback confirms exact Unicode text")
	line = call("chrome-local-enter", `await state.input.press('Enter');`)
	if !strings.Contains(line, ".press:") {
		t.Fatalf("Enter result cannot be tied to script target: %s", line)
	}
	assertOriginalRoute("chrome-local-enter", "targeted_background")
	waitEvent(`{"kind":"key","value":"Enter"}`)
	t.Log("independent Chrome callback confirms background Enter")
	line = call("chrome-local-scroll", `await state.win.scroll({dy:1,u:0.8,v:0.7});`)
	if !strings.Contains(line, ".scroll:") {
		t.Fatalf("scroll result cannot be tied to script target: %s", line)
	}
	assertOriginalRoute("chrome-local-scroll", "targeted_background")
	waitEvent(`{"kind":"wheel","value":"1"}`)
	t.Log("independent Chrome callback confirms background wheel")
	mu.Lock()
	defer mu.Unlock()
	for _, event := range []string{`{"kind":"click","value":"button"}`, `{"kind":"input","value":"DTW-Chrome-中文🙂"}`, `{"kind":"key","value":"Enter"}`, `{"kind":"wheel","value":"1"}`} {
		count := 0
		for _, got := range received {
			if got == event {
				count++
			}
		}
		if count != 1 {
			t.Fatalf("Chrome callback count for %s = %d, want 1", event, count)
		}
	}
}
