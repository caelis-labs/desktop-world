package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Read-only, opt-in real-App proof that separate no-grant POC Sessions keep
// their own JS/native state and one spinning script cannot block the peer.
func TestTwoRealAppObjectSessionsRemainIndependent(t *testing.T) {
	if os.Getenv("DTW_POC_CHILD") == "1" {
		return
	}
	title := os.Getenv("DTW_POC_REAL_WINDOW_TITLE")
	if title == "" || os.Getenv("DTW_POC_HELPER") == "" {
		t.Skip("requires selected real window and isolated core helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	open := func(label string) *mcp.ClientSession {
		child := exec.Command(os.Args[0], "-test.run=^TestStdioChild$")
		child.Env = append(os.Environ(), "DTW_POC_CHILD=1", "DTW_POC_LEGACY_OUTPUT=0", "PATH=/usr/bin:/bin")
		client := mcp.NewClient(&mcp.Implementation{Name: "real-object-" + label, Version: "1"}, nil)
		session, err := client.Connect(ctx, &mcp.CommandTransport{Command: child}, nil)
		if err != nil {
			t.Fatal(err)
		}
		return session
	}
	a, b := open("a"), open("b")
	defer a.Close()
	defer b.Close()
	call := func(session *mcp.ClientSession, operation, id, code string) (*mcp.CallToolResult, error) {
		return session.CallTool(ctx, &mcp.CallToolParams{Name: "exec", Arguments: map[string]any{
			"operation": operation, "execution_id": id, "code": code,
		}})
	}
	for _, peer := range []struct {
		label   string
		session *mcp.ClientSession
	}{{"A", a}, {"B", b}} {
		started := time.Now()
		code := `state.owner=` + string(mustJSON(peer.label)) + `;const app=await dtw.app('Obsidian');state.win=await app.window(` + string(mustJSON(title)) + `);state.control=await state.win.one({name:'新建笔记'});const name=await state.control.read('name');if(name?.value!=='新建笔记')throw Error('wrong selected control');print(state.owner+' '+state.control.id);`
		result, err := call(peer.session, "exec", "real-parallel-setup", code)
		if err != nil || result.IsError || len(result.Content) != 1 {
			t.Fatalf("Session %s setup: %v %+v", peer.label, err, result)
		}
		if got := result.Content[0].(*mcp.TextContent).Text; !strings.Contains(got, peer.label+" W") {
			t.Fatalf("Session %s object address missing: %q", peer.label, got)
		}
		t.Logf("Session %s setup=%s", peer.label, time.Since(started).Round(time.Millisecond))
	}
	type readback struct {
		label string
		text  string
		err   error
		spent time.Duration
	}
	readStart := make(chan struct{})
	readDone := make(chan readback, 2)
	for _, peer := range []struct {
		label   string
		session *mcp.ClientSession
	}{{"A", a}, {"B", b}} {
		go func(label string, session *mcp.ClientSession) {
			<-readStart
			start := time.Now()
			result, err := call(session, "exec", "real-parallel-read", `const n=await state.control.read('name');if(n?.value!=='新建笔记')throw Error('name changed');print(state.owner+' read');`)
			if err != nil {
				readDone <- readback{label: label, err: err, spent: time.Since(start)}
				return
			}
			if len(result.Content) != 1 {
				readDone <- readback{label: label, err: fmt.Errorf("native read returned %d blocks", len(result.Content)), spent: time.Since(start)}
				return
			}
			message := result.Content[0].(*mcp.TextContent).Text
			var callErr error
			if result.IsError {
				callErr = fmt.Errorf("native read failed: %s", message)
			}
			readDone <- readback{label: label, text: message, err: callErr, spent: time.Since(start)}
		}(peer.label, peer.session)
	}
	close(readStart)
	for range 2 {
		got := <-readDone
		if got.err != nil || !strings.Contains(got.text, " · "+got.label+" read") {
			t.Fatalf("Session %s parallel native read: %v %q", got.label, got.err, got.text)
		}
		t.Logf("Session %s parallel native read=%s", got.label, got.spent.Round(time.Millisecond))
	}
	if sequence := os.Getenv("DTW_POC_SEQUENTIAL_CAPTURE"); sequence != "" {
		peers := []struct {
			label   string
			session *mcp.ClientSession
		}{{"A1", a}, {"A2", a}}
		if sequence == "cross" {
			peers = []struct {
				label   string
				session *mcp.ClientSession
			}{{"A1", a}, {"B", b}, {"A2", a}}
		}
		for _, peer := range peers {
			started := time.Now()
			result, err := call(peer.session, "exec", "real-sequence-capture-"+peer.label, `await state.win.capture();print(state.owner);`)
			message := ""
			if result != nil && len(result.Content) > 0 {
				message = result.Content[0].(*mcp.TextContent).Text
			}
			t.Logf("Session %s sequential capture duration=%s result=%q transport=%v", peer.label,
				time.Since(started).Round(time.Millisecond), message, err)
			if err != nil || result == nil || result.IsError || !strings.Contains(message, "capture: 1 image(s) ready") {
				t.Errorf("Session %s sequential capture failed: %v %q", peer.label, err, message)
			}
		}
	}
	if os.Getenv("DTW_POC_PARALLEL_CAPTURE") == "1" {
		type capture struct {
			label string
			text  string
			err   error
			spent time.Duration
		}
		started := make(chan struct{})
		done := make(chan capture, 2)
		for _, peer := range []struct {
			label   string
			session *mcp.ClientSession
		}{{"A", a}, {"B", b}} {
			go func(label string, session *mcp.ClientSession) {
				<-started
				start := time.Now()
				result, err := call(session, "exec", "real-parallel-capture", `await state.win.capture();print(state.owner);`)
				if err != nil {
					done <- capture{label: label, err: err, spent: time.Since(start)}
					return
				}
				if result.IsError || len(result.Content) != 1 {
					message := "missing model result"
					if len(result.Content) > 0 {
						if text, ok := result.Content[0].(*mcp.TextContent); ok {
							message = text.Text
						}
					}
					done <- capture{label: label, err: fmt.Errorf("capture failed: %s", message), spent: time.Since(start)}
					return
				}
				done <- capture{label: label, text: result.Content[0].(*mcp.TextContent).Text, spent: time.Since(start)}
			}(peer.label, peer.session)
		}
		close(started)
		var captures []capture
		for range 2 {
			captures = append(captures, <-done)
		}
		for _, got := range captures {
			t.Logf("Session %s parallel capture duration=%s result=%q error=%v", got.label, got.spent.Round(time.Millisecond), got.text, got.err)
			if got.err != nil || !strings.Contains(got.text, "capture: 1 image(s) ready") ||
				!strings.Contains(got.text, " · "+got.label) {
				t.Fatalf("Session %s parallel capture: %v %q", got.label, got.err, got.text)
			}
		}
	}
	loopDone := make(chan error, 1)
	go func() {
		result, err := call(a, "exec", "real-parallel-loop", `while(true){}`)
		if err != nil {
			loopDone <- err
			return
		}
		if !strings.Contains(result.Content[0].(*mcp.TextContent).Text, "cancelled") {
			loopDone <- fmt.Errorf("loop ended without cancellation: %+v", result)
			return
		}
		loopDone <- nil
	}()
	running := false
	for i := 0; i < 100; i++ {
		status, err := call(a, "status", "real-parallel-loop", "")
		if err == nil && strings.Contains(status.Content[0].(*mcp.TextContent).Text, "running") {
			running = true
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if !running {
		t.Fatal("Session A loop never reached observable running state")
	}
	peer, err := call(b, "exec", "real-parallel-peer", `const name=await state.control.read('name');if(name?.value!=='新建笔记')throw Error('peer read failed');print(state.owner+' readable');`)
	if err != nil || peer.IsError || !strings.Contains(peer.Content[0].(*mcp.TextContent).Text, "B readable") {
		t.Fatalf("peer blocked by Session A loop: %v %+v", err, peer)
	}
	status, err := call(a, "status", "real-parallel-loop", "")
	if err != nil || !strings.Contains(status.Content[0].(*mcp.TextContent).Text, "running") {
		t.Fatalf("Session A stopped before peer read completed: %v %+v", err, status)
	}
	stopped, err := call(a, "cancel", "real-parallel-loop", "")
	if err != nil || stopped.IsError {
		t.Fatalf("cancel Session A: %v %+v", err, stopped)
	}
	select {
	case err := <-loopDone:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("Session A loop did not stop")
	}
	verify, err := call(b, "exec", "real-parallel-after", `print(state.owner);`)
	if err != nil || verify.IsError || !strings.Contains(verify.Content[0].(*mcp.TextContent).Text, " · B") {
		t.Fatalf("Session B state affected by A cancel: %v %+v", err, verify)
	}
	afterA, err := call(a, "exec", "real-parallel-a-after", `print(state.owner);`)
	if err != nil || afterA.IsError || !strings.Contains(afterA.Content[0].(*mcp.TextContent).Text, " · A") {
		t.Fatalf("Session A state lost after its cancellation: %v %+v", err, afterA)
	}
	t.Log("two real App Sessions kept independent state and peer read during loop cancellation")
}
