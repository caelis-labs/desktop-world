package protocol_test

import (
	"context"
	"encoding/json"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"strings"
	"testing"
	"time"
)

func TestWirePrecisionAndFacts(t *testing.T) {
	v := struct {
		Revision dw.Revision
		Timeout  time.Duration
		Known    dw.Fact[bool]
		Unknown  dw.Fact[string]
	}{dw.Revision(1<<63 + 17), 2 * time.Second, dw.Known(false), dw.Unknown[string]()}
	b, e := protocol.Marshal(v)
	if e != nil {
		t.Fatal(e)
	}
	s := string(b)
	for _, want := range []string{`"revision":"9223372036854775825"`, `"timeout_ms":2000`, `"value":false`, `"status":"unknown"`} {
		if !strings.Contains(s, want) {
			t.Fatalf("missing %s in %s", want, s)
		}
	}
}
func TestStrictWire(t *testing.T) {
	for _, s := range []string{`{"timeout_ms":1,"timeout_ms":2}`, `{"unknown":true}`, `{"timeout_ms":1.1}`, `{"timeout_ms":-1}`, `{"epoch":null}`, `{} {}`, `{"steps":[{"target":{"ref":"a","extra":true}}]}`} {
		var p dw.Plan
		if e := protocol.Decode([]byte(s), &p); e == nil {
			t.Fatalf("accepted %s", s)
		}
	}
	var v struct{ Revision dw.Revision }
	if e := protocol.Decode([]byte(`{"revision":9007199254740993}`), &v); e == nil {
		t.Fatal("accepted numeric revision")
	}
}
func TestDesignPlanDecodesAndValidates(t *testing.T) {
	b, e := os.ReadFile("../examples/protocol/world-act-01.json")
	if e != nil {
		t.Fatal(e)
	}
	var r protocol.Request
	if e = protocol.Decode(b, &r); e != nil {
		t.Fatal(e)
	}
	var p dw.Plan
	if e = protocol.Decode(r.Args, &p); e != nil {
		t.Fatal(e)
	}
	if e = p.Validate(); e != nil {
		t.Fatal(e)
	}
}
func TestHandlerBoundToActor(t *testing.T) {
	ctx := context.Background()
	w, f, e := dwtest.New(ctx)
	if e != nil {
		t.Fatal(e)
	}
	defer w.Close(ctx)
	f.Form()
	a, _ := w.NewActor(ctx, dw.ActorConfig{ID: "a", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	env, _ := w.Environment(ctx)
	h := protocol.Handler{Actor: a, Epoch: env.Epoch}
	b, _ := json.Marshal(map[string]any{"protocol": protocol.Version, "world": env.Epoch, "op": "world.observe", "args": map[string]any{"scope": map[string]bool{"desktop": true}, "projection": "summary"}})
	reply, e := h.Handle(ctx, b)
	if e != nil || !strings.Contains(string(reply), `"objects"`) {
		t.Fatalf("%s %v", reply, e)
	}
	bad := strings.Replace(string(b), "world.observe", "native.eval", 1)
	if _, e = h.Handle(ctx, []byte(bad)); e == nil {
		t.Fatal("accepted arbitrary op")
	}
}

func TestHandlerPreservesDeadlineCause(t *testing.T) {
	w, f, err := dwtest.New(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close(context.Background())
	f.Form()
	a, _ := w.NewActor(context.Background(), dw.ActorConfig{ID: "deadline", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	env, _ := w.Environment(context.Background())
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	b, _ := protocol.Marshal(protocol.Request{Protocol: protocol.Version, World: env.Epoch, Op: "world.observe", Args: json.RawMessage(`{"scope":{"desktop":true}}`)})
	out, err := (protocol.Handler{Actor: a, Epoch: env.Epoch}).Handle(ctx, b)
	if err != nil || !strings.Contains(string(out), `"code":"cancelled"`) {
		t.Fatalf("%s %v", out, err)
	}
	ctx, cancel = context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
	defer cancel()
	out, err = (protocol.Handler{Actor: a, Epoch: env.Epoch}).Handle(ctx, b)
	if err != nil || !strings.Contains(string(out), `"code":"deadline_exceeded"`) || !strings.Contains(string(out), "read_deadline_ms") {
		t.Fatalf("%s %v", out, err)
	}
}
func FuzzDecodePlan(f *testing.F) {
	f.Add([]byte(`{"epoch":"e","request_id":"e:r","steps":[]}`))
	f.Add([]byte(`{"steps":[{"target":{"ref":"x","bound":"y"}}]}`))
	f.Fuzz(func(t *testing.T, b []byte) {
		var p dw.Plan
		if protocol.Decode(b, &p) == nil {
			_ = p.Validate()
		}
	})
}
