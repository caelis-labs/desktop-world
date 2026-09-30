package acceptance_test

import (
	"context"
	"encoding/json"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
	"github.com/caelis-labs/desktop-world/protocol"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

func TestNativeFragmentedTextFixture(t *testing.T) {
	title := os.Getenv("DW_FRAGMENTED_FIXTURE_TITLE")
	if title == "" {
		t.Skip("explicit local browser fixture required")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	w, e := local.Open(ctx, local.Options{})
	if e != nil {
		t.Fatal(e)
	}
	defer w.Close(context.Background())
	a, e := w.NewActor(ctx, dw.ActorConfig{ID: "fragmented-discovery", ReadScopes: []dw.Scope{{Desktop: true}}, Operations: []string{"observe"}})
	if e != nil {
		t.Fatal(e)
	}
	inventory, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Budget: dw.Budget{ReadDeadline: 5 * time.Second, MaxResults: 1024, MaxVisitedNodes: 2048, MaxOutputBytes: 1 << 20}})
	if e != nil {
		t.Fatal(e)
	}
	var window dw.Ref
	for _, o := range inventory.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && strings.Contains(*o.Name.Value, title) {
			if window != "" {
				t.Fatal("ambiguous fixture window")
			}
			window = o.Ref
		}
	}
	if window == "" {
		t.Fatal("local browser fixture not open")
	}
	a.Close()
	a, e = w.NewActor(ctx, dw.ActorConfig{ID: "fragmented-local", ReadScopes: []dw.Scope{{Refs: []dw.Ref{window}}}, Operations: []string{"observe", "read"}})
	if e != nil {
		t.Fatal(e)
	}
	discovery, e := a.Observe(ctx, dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{window}}, Projection: dw.ProjectionOutline, Fields: []string{"role"}, Match: &dw.Locator{Within: window, Role: "document"}, Budget: dw.Budget{MaxDepth: 12, MaxVisitedNodes: 1500, MaxResults: 1024, MaxOutputBytes: 12000, ReadDeadline: 5 * time.Second}})
	if e != nil {
		t.Fatal(e)
	}
	if !discovery.Coverage.Complete || len(discovery.Objects) != 1 {
		t.Fatalf("document discovery ambiguous/incomplete: %+v", discovery.Coverage)
	}
	document := discovery.Objects[0].Ref
	request := dw.ObserveRequest{Scope: dw.Scope{Refs: []dw.Ref{document}}, Projection: dw.ProjectionOutline, Fields: []string{"role", "value_preview", "parent"}, Budget: dw.Budget{MaxDepth: 16, MaxVisitedNodes: 256, MaxResults: 256, MaxOutputBytes: 12000, MaxTextRunes: 384, ReadDeadline: 5 * time.Second}}
	var objects []dw.Object
	pages := 0
	modelBytes := 0
	var first dw.Coverage
	for {
		ob, e := a.Observe(ctx, request)
		if e != nil {
			t.Fatal(e)
		}
		body, _ := protocol.Marshal(ob)
		compact, _ := protocol.CompactJSON(body)
		modelBytes += len(compact)
		if len(body) > request.Budget.MaxOutputBytes {
			t.Fatalf("page exceeded output budget: %d", len(body))
		}
		pages++
		if pages == 1 {
			first = ob.Coverage
		}
		if !ob.Coverage.SampleStart.Equal(first.SampleStart) || !ob.Coverage.SampleEnd.Equal(first.SampleEnd) {
			t.Fatal("continuation resampled")
		}
		objects = append(objects, ob.Objects...)
		if ob.Coverage.Continuation == "" {
			if !ob.Coverage.Complete {
				t.Fatalf("incomplete traversal: %+v", ob.Coverage)
			}
			break
		}
		request.Continuation = ob.Coverage.Continuation
		if pages > 32 {
			t.Fatal("unbounded pagination")
		}
	}
	byRef := map[dw.Ref]dw.Object{}
	children := map[dw.Ref][]dw.Ref{}
	for _, o := range objects {
		if _, ok := byRef[o.Ref]; ok {
			t.Fatal("duplicate ref across pages")
		}
		byRef[o.Ref] = o
		children[o.Parent] = append(children[o.Parent], o.Ref)
	}
	type fragment struct {
		Ref, Parent dw.Ref
		Text        string
	}
	var fragments []fragment
	var paragraphs []string
	var current strings.Builder
	var walk func(dw.Ref)
	walk = func(ref dw.Ref) {
		o := byRef[ref]
		isParagraph := o.Role == "paragraph" || o.Role == "heading" || (o.Parent == document && o.Role == "container")
		if isParagraph && current.Len() > 0 {
			paragraphs = append(paragraphs, current.String())
			current.Reset()
		}
		if o.Role == "text" && len(children[ref]) == 0 && o.ValuePreview.Status == dw.FactKnown && o.ValuePreview.Value != nil {
			text := *o.ValuePreview.Value
			fragments = append(fragments, fragment{ref, o.Parent, text})
			current.WriteString(text)
		}
		for _, child := range children[ref] {
			walk(child)
		}
		if isParagraph && current.Len() > 0 {
			paragraphs = append(paragraphs, current.String())
			current.Reset()
		}
	}
	walk(document)
	if current.Len() > 0 {
		paragraphs = append(paragraphs, current.String())
	}
	b, _ := json.MarshalIndent(struct {
		Pages      int
		Coverage   dw.Coverage
		Fragments  []fragment
		Paragraphs []string
	}{pages, first, fragments, paragraphs}, "", "  ")
	if path := os.Getenv("DW_FRAGMENTED_EVIDENCE_PATH"); path != "" {
		if e = os.WriteFile(path, b, 0600); e != nil {
			t.Fatal(e)
		}
	}
	t.Logf("pages=%d visited=%d fragments=%d paragraphs=%q", pages, first.VisitedNodes, len(fragments), paragraphs)
	expected := []string{"Fragmented native AX fixture", "Every character has its own span.", "Nested 中文 🌍 café العربية.", "Repeat Repeat"}
	if !reflect.DeepEqual(paragraphs, expected) {
		t.Fatalf("native block order/boundaries: got=%q want=%q", paragraphs, expected)
	}
	joined := strings.Join(paragraphs, "\n")
	for _, want := range []string{"Every character has its own span.", "Nested 中文 🌍 café العربية.", "Repeat Repeat"} {
		if !strings.Contains(joined, want) {
			t.Fatalf("missing/order/Unicode: %q in %q", want, joined)
		}
	}
	if strings.Contains(joined, "HIDDEN") || strings.Contains(joined, "fixture-secret") {
		t.Fatal("hidden/protected content leaked")
	}
	baselineBytes := 0
	if len(fragments) > 80 {
		t.Fatal("baseline unexpectedly large")
	}
	for _, fragment := range fragments {
		read, e := a.ReadText(ctx, dw.TextRequest{Target: fragment.Ref})
		if e != nil || read.Source != "value" || read.Text.Value == nil || *read.Text.Value != fragment.Text {
			t.Fatalf("leaf baseline %+v %v", read, e)
		}
		body, _ := protocol.Marshal(read)
		compact, _ := protocol.CompactJSON(body)
		baselineBytes += len(compact)
	}
	t.Logf("model-visible compact result bytes: batched=%d over %d calls; per-leaf=%d over %d additional calls (discovery common)", modelBytes, pages, baselineBytes, len(fragments))
	if len(fragments) < 35 || len(paragraphs) < 3 {
		t.Fatal("fixture did not exercise fragmented leaves and paragraph boundaries")
	}
}
