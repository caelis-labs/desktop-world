package host

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"strings"
	"testing"
	"time"
)

func TestPeerKeepsOriginalResultAndNeverReplays(t *testing.T) {
	repliesR, repliesW := io.Pipe()
	requestsR, requestsW := io.Pipe()
	t.Cleanup(func() { repliesR.Close(); repliesW.Close(); requestsR.Close(); requestsW.Close() })
	p := newPeer(repliesR, requestsW)
	go p.read(bufio.NewScanner(repliesR))
	request := map[string]string{"id": "one", "op": "act"}
	x, err := p.submit("one", request)
	if err != nil {
		t.Fatal(err)
	}
	scan := bufio.NewScanner(requestsR)
	if !scan.Scan() {
		t.Fatal("request missing")
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Millisecond)
	defer cancel()
	if _, err = wait(ctx, x); err == nil {
		t.Fatal("expected uncertain timeout")
	}
	y, err := p.submit("one", request)
	if err != nil || x != y {
		t.Fatal("resubmitted request")
	}
	if _, err = p.submit("one", map[string]string{"id": "one", "op": "changed"}); err == nil {
		t.Fatal("conflicting ID")
	}
	go io.WriteString(repliesW, "{\"id\":\"one\",\"result\":{\"run_id\":\"r\",\"outcome\":\"unknown\"}}\n")
	ctx2, cancel2 := context.WithTimeout(context.Background(), time.Second)
	defer cancel2()
	r, err := wait(ctx2, x)
	if err != nil || !strings.Contains(string(r.Result), "unknown") {
		t.Fatal(r, err)
	}
	// The Client's read-only recovery path uses the same original exchange.
	c := &Client{data: p, requests: map[string]string{"turn\x00id": "one"}}
	r, err = c.Reconcile(ctx2, "turn", "id")
	if err != nil || r.ID != "one" {
		t.Fatal(r, err)
	}
}

func TestContentBudgetCountsBothCopiesAndPreservesUnknownEffect(t *testing.T) {
	for _, text := range []string{strings.Repeat("<&>\u2028🌍", 500), strings.Repeat("<&>\u2028🌍", 10000)} {
		body, _ := json.Marshal(map[string]any{"outcome": "unknown", "run_id": "original-run", "seat_health": "fenced", "input": map[string]any{"mode": "cooperative", "foreground_ms": 240, "restoration": "failed", "restoration_reason": "previous_focus_not_acknowledged"}, "evidence": text})
		r := Content(Reply{ID: "original-request", Result: body})
		structured, _ := json.Marshal(r.StructuredContent)
		if len(r.Content[0].Text)+len(structured)+1024 > 32*1024 {
			t.Fatal("exceeded Runtime budget")
		}
		if !r.IsError || !strings.Contains(r.Content[0].Text, "original-run") || !strings.Contains(r.Content[0].Text, "unknown") || !strings.Contains(r.Content[0].Text, `"restoration":"failed"`) || !strings.Contains(r.Content[0].Text, `"restoration_reason":"previous_focus_not_acknowledged"`) {
			t.Fatal("lost uncertain effect")
		}
	}
}

func TestContentCompactsOnlyModelProjection(t *testing.T) {
	raw := json.RawMessage(`{"objects":[{"ref":"r","kind":"ui","sample_start":"time","name":{"status":"known","value":"","source":"ui_content"}}],"coverage":{"complete":false}}`)
	r := Content(Reply{ID: "read", Result: raw})
	if !strings.Contains(r.Content[0].Text, `"known":""`) || !strings.Contains(r.Content[0].Text, `"complete":false`) || strings.Contains(r.Content[0].Text, "sample_start") {
		t.Fatal("wrong compact model projection", r.Content[0].Text)
	}
	if !strings.Contains(string(raw), "sample_start") {
		t.Fatal("original reply changed")
	}
}

func TestEndTurnWithAlreadyCancelledContextClosesAuthorityPipe(t *testing.T) {
	dataR, dataW := io.Pipe()
	controlR, controlW := io.Pipe()
	dataReplyR, dataReplyW := io.Pipe()
	controlReplyR, controlReplyW := io.Pipe()
	t.Cleanup(func() { dataR.Close(); controlR.Close(); dataReplyW.Close(); controlReplyW.Close() })
	exited := make(chan struct{})
	close(exited)
	c := &Client{data: newPeer(dataReplyR, dataW), control: newPeer(controlReplyR, controlW), exited: exited}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := c.EndTurn(ctx, "turn1"); err == nil {
		t.Fatal("expected cancellation")
	}
	if _, err := controlR.Read(make([]byte, 1)); err != io.EOF {
		t.Fatal("authority pipe remained open", err)
	}
	c.Close()
}
