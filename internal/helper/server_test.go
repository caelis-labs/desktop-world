package helper

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/dwtest"
	"github.com/caelis-labs/desktop-world/protocol"
	"io"
	"strings"
	"testing"
	"time"
)

func setup(t *testing.T, c Config) (*Server, *dwtest.Fixture) {
	t.Helper()
	w, f, err := dwtest.New(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	f.Form()
	s, err := New(context.Background(), w, c)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, c := context.WithTimeout(context.Background(), time.Second)
		defer c()
		if err := w.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	return s, f
}
func call(s *Server, id, op string, args any) Response {
	b, _ := json.Marshal(args)
	return s.Handle(context.Background(), Request{ID: id, Op: op, Args: b})
}
func refs(t *testing.T, s *Server) map[string]dw.Ref {
	t.Helper()
	r := call(s, "inventory", "observe", map[string]any{"scope": map[string]bool{"desktop": true}, "projection": "outline", "budget": map[string]int{"max_output_bytes": 65536}})
	if r.Error != nil {
		t.Fatal(r.Error)
	}
	var ob dw.Observation
	if err := protocol.Decode(r.Result.(json.RawMessage), &ob); err != nil {
		t.Fatal(err)
	}
	m := map[string]dw.Ref{}
	for _, o := range ob.Objects {
		if o.Name.Value != nil {
			m[*o.Name.Value] = o.Ref
		}
	}
	return m
}
func invoke(ref dw.Ref) any {
	return map[string]any{"steps": []any{map[string]any{"id": "press", "op": "invoke", "target": map[string]any{"ref": ref}}}}
}

func TestHelperStableIDAndPolicy(t *testing.T) {
	s, f := setup(t, Config{WriteApps: []string{"Fixture"}})
	r := refs(t, s)
	args := invoke(r["提交"])
	one := call(s, "submit-once", "act", args)
	if one.Error != nil {
		t.Fatal(one.Error)
	}
	again := call(s, "submit-once", "act", args)
	if again.Error != nil {
		t.Fatal(again.Error)
	}
	var a, b dw.Receipt
	_ = protocol.Decode(one.Result.(json.RawMessage), &a)
	_ = protocol.Decode(again.Result.(json.RawMessage), &b)
	if a.RunID != b.RunID || len(f.Events()) != 1 {
		t.Fatal("replayed helper ID")
	}
	conflict := call(s, "submit-once", "act", invoke(r["内容"]))
	if conflict.Error == nil || conflict.Error.Code != "request_conflict" {
		t.Fatalf("%+v", conflict)
	}
	bad := call(s, "bad", "act", map[string]any{"epoch": "foreign"})
	if bad.Error == nil {
		t.Fatal("accepted model epoch")
	}
	f.Add(dwtest.Node{ID: "other", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Other")}})
	f.Add(dwtest.Node{ID: "other-button", App: "other", Parent: "other", Object: dw.Object{Kind: dw.KindUI, Role: "button", Name: dw.Known("Other button")}})
	r = refs(t, s)
	denied := call(s, "other", "act", invoke(r["Other button"]))
	if denied.Error == nil || len(f.Events()) != 1 {
		t.Fatal("escaped host app scope")
	}
}

func TestHelperDefaultReadOnly(t *testing.T) {
	s, f := setup(t, Config{})
	r := refs(t, s)
	out := call(s, "write", "act", invoke(r["提交"]))
	if out.Error == nil || len(f.Events()) != 0 {
		t.Fatal("default permitted write")
	}
}

func TestHelperFullEnvelopeBudget(t *testing.T) {
	s, _ := setup(t, Config{})
	out := call(s, strings.Repeat("a", 128), "observe", map[string]any{"scope": map[string]bool{"desktop": true}, "projection": "outline", "budget": map[string]int{"max_output_bytes": 2500}})
	if out.Error != nil {
		t.Fatal(out.Error)
	}
	b, _ := protocol.Marshal(out)
	if len(b) > 2500 {
		t.Fatalf("envelope %d > 2500", len(b))
	}
}

func TestStdioMetadataAndEOF(t *testing.T) {
	var audit bytes.Buffer
	s, _ := setup(t, Config{Audit: &audit})
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- s.Serve(ctx, inR, outW); outW.Close() }()
	scan := bufio.NewScanner(outR)
	if !scan.Scan() || !strings.Contains(scan.Text(), `"type":"hello"`) {
		t.Fatal("missing hello")
	}
	secretName := "private-ui-value"
	request := `{"id":"observe","op":"observe","args":{"scope":{"desktop":true},"projection":"summary","fields":["name"]}}` + "\n"
	go func() { _, _ = io.WriteString(inW, request) }()
	if !scan.Scan() {
		t.Fatal("no reply")
	}
	inW.Close()
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if strings.Contains(audit.String(), "Fixture") || strings.Contains(audit.String(), secretName) || !strings.Contains(audit.String(), "response_bytes") {
		t.Fatalf("audit policy: %s", audit.String())
	}
	var row map[string]any
	if json.Unmarshal(bytes.Split(bytes.TrimSpace(audit.Bytes()), []byte("\n"))[1], &row) != nil || row["op"] != "observe" {
		t.Fatal("invalid audit")
	}
	if row["full_response_bytes"].(float64) <= row["response_bytes"].(float64) {
		t.Fatal("compact audit did not measure the actual reduction")
	}
}

func TestPipeAcceptsUnicodeBeyondPTYLineLimit(t *testing.T) {
	s, _ := setup(t, Config{WriteApps: []string{"Fixture"}})
	r := refs(t, s)
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- s.Serve(ctx, inR, outW); outW.Close() }()
	scan := bufio.NewScanner(outR)
	scan.Buffer(make([]byte, 4096), 1<<20)
	if !scan.Scan() {
		t.Fatal("hello")
	}
	args, _ := json.Marshal(map[string]any{"steps": []any{map[string]any{"id": "write", "op": "set_value", "target": map[string]any{"ref": r["内容"]}, "set_value": map[string]any{"text": strings.Repeat("Unicode 🌍", 700)}}}})
	request, _ := protocol.Marshal(Request{ID: "long-line", Op: "act", Args: args})
	if len(request) <= 4096 {
		t.Fatal("test did not exceed PTY line limit")
	}
	go func() { _, _ = inW.Write(append(request, '\n')) }()
	if !scan.Scan() || !strings.Contains(scan.Text(), `"outcome":"completed"`) {
		t.Fatalf("truncated/unverified input: %s", scan.Text())
	}
	inW.Close()
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}

func TestAppWindowDisambiguatesSameApplicationNames(t *testing.T) {
	w, f, err := dwtest.New(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close(context.Background())
	f.Form()
	f.Add(dwtest.Node{ID: "other-app", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Fixture")}})
	f.Add(dwtest.Node{ID: "other-window", App: "other-app", Parent: "other-app", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("Other window")}})
	ambiguous, err := New(context.Background(), w, Config{WriteApps: []string{"Fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	status, err := ambiguous.grantStatus("session")
	if err != nil || len(status.Grants) != 1 || status.Grants[0].State != "ambiguous" {
		t.Fatal("ambiguity not reported", status, err)
	}
	ambiguous.actor.Close()
	ambiguous.discovery.Close()
	// A new helper process opens a new World; Actor IDs are not reusable after Close.
	w2, f2, err := dwtest.New(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer w2.Close(context.Background())
	f2.Form()
	f2.Add(dwtest.Node{ID: "other-app", Object: dw.Object{Kind: dw.KindApplication, Name: dw.Known("Fixture")}})
	f2.Add(dwtest.Node{ID: "other-window", App: "other-app", Parent: "other-app", Object: dw.Object{Kind: dw.KindWindow, Name: dw.Known("Other window")}})
	s, err := New(context.Background(), w2, Config{WriteAppWindows: []string{"Desktop World Fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	r := refs(t, s)
	if got := call(s, "invoke", "act", invoke(r["提交"])); got.Error != nil {
		t.Fatal(got.Error)
	}
	if got := call(s, "other", "act", map[string]any{"steps": []any{map[string]any{"id": "focus", "op": "focus", "target": map[string]any{"ref": r["Other window"]}}}}); got.Error == nil {
		t.Fatal("granted both same-name apps")
	}
}

func TestControlCapacityDuringNativeWrite(t *testing.T) {
	s, f := setup(t, Config{DesktopWrite: true})
	r := refs(t, s)
	entered := make(chan struct{})
	release := make(chan struct{})
	defer close(release)
	f.Enqueue("invoke", dwtest.Behavior{Before: func(*dwtest.Fixture) { close(entered) }, Block: release, Apply: true})
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- s.Serve(ctx, inR, outW); outW.Close() }()
	scan := bufio.NewScanner(outR)
	if !scan.Scan() {
		t.Fatal("hello")
	}
	args, _ := json.Marshal(invoke(r["提交"]))
	a, _ := protocol.Marshal(Request{ID: "blocked", Op: "act", Args: args})
	go func() { _, _ = inW.Write(append(a, '\n')) }()
	select {
	case <-entered:
	case <-ctx.Done():
		t.Fatal("write did not start")
	}
	go func() {
		_, _ = io.WriteString(inW, `{"id":"read","op":"observe","args":{"scope":{"desktop":true}}}`+"\n"+`{"id":"control","op":"get","args":{"run_id":"missing"}}`+"\n")
	}()
	if !scan.Scan() || !strings.Contains(scan.Text(), `"id":"control"`) {
		t.Fatal("control starved behind native write")
	}
	cancel()
	inW.Close()
	go io.Copy(io.Discard, outR)
	<-done
}

func TestHelperQueryFaultRetainsDiagnosticsOnlyCoverage(t *testing.T) {
	s, f := setup(t, Config{})
	r := refs(t, s)
	f.SetReadFault(dw.NewFault("provider_unavailable", "fixture provider unavailable", "reobserve"))
	reply := call(s, "query-fault", "observe", map[string]any{"scope": map[string]any{"refs": []dw.Ref{r["Desktop World Fixture"]}}, "projection": "outline", "fields": []string{"name"}, "budget": map[string]int{"max_output_bytes": 4096}})
	if reply.Error == nil || reply.Error.Code != "provider_unavailable" {
		t.Fatalf("fault lost: %+v", reply)
	}
	var ob dw.Observation
	if e := protocol.Decode(reply.Result.(json.RawMessage), &ob); e != nil {
		t.Fatal(e)
	}
	if len(ob.Objects) != 0 || ob.Coverage.Complete || !ob.Coverage.Dirty || ob.Coverage.SampleStart.IsZero() || ob.Coverage.SampleEnd.Before(ob.Coverage.SampleStart) || len(ob.Coverage.UnavailableSources) != 1 || ob.Coverage.UnavailableSources[0] != "provider_unavailable" {
		t.Fatalf("diagnostics lost or leaked objects: %+v", ob)
	}
	if len(ob.Coverage.Scope.Refs) != 1 || ob.Coverage.Scope.Refs[0] != r["Desktop World Fixture"] {
		t.Fatal("fault coverage changed scope")
	}
}
