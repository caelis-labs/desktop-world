package engine

import (
	"context"
	"crypto/sha256"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/backend"
	"time"
	"unicode/utf8"
)

type textState struct {
	digest  [32]byte
	version dw.Version
}
type textPage struct {
	target  dw.Ref
	version dw.Version
	offset  int
	source  string
	expires time.Time
}
type assetRecord struct {
	asset             dw.Asset
	intent            dw.Intent
	permissionVersion uint64
}

func (a *actor) readTextNative(ctx context.Context, ref dw.Ref) (backend.Text, error) {
	o, e := a.w.read(ctx, ref)
	if e != nil {
		return backend.Text{}, e
	}
	if f := o.States["protected"]; f.Status == dw.FactKnown && f.Value != nil && *f.Value {
		return backend.Text{Value: dw.Fact[string]{Status: dw.FactRedacted}, Source: "value"}, nil
	}
	a.w.mu.Lock()
	key := a.w.objects[ref].key
	a.w.mu.Unlock()
	v, e := a.w.call(ctx, func() (any, error) { return a.w.driver.ReadText(ctx, key) })
	if e != nil {
		return backend.Text{}, e
	}
	t := v.(backend.Text)
	if t.Value.Value != nil && (!utf8.ValidString(*t.Value.Value) || len(*t.Value.Value) > 1<<20) {
		return backend.Text{}, fault("resource_exhausted")
	}
	return t, t.Value.Validate()
}
func (a *actor) ReadText(ctx context.Context, r dw.TextRequest) (dw.TextResult, error) {
	if r.Target == "" || r.Offset < 0 || r.Offset > 1<<20 || r.LimitRunes < 0 || r.LimitRunes > 4096 {
		return dw.TextResult{}, dw.Invalid("invalid text request")
	}
	if r.Freshness.Mode != "" && r.Freshness.Mode != "refresh" && r.Freshness.Mode != "max_age" && r.Freshness.Mode != "cached" {
		return dw.TextResult{}, dw.Invalid("unknown freshness")
	}
	if r.LimitRunes == 0 {
		r.LimitRunes = 4096
	}
	ctx, cancel := context.WithTimeout(ctx, 500*time.Millisecond)
	defer cancel()
	in := dw.Intent{Operation: "read", Targets: []dw.Ref{r.Target}, Fields: []string{"text"}}
	if e := a.check(ctx, in, false); e != nil {
		return dw.TextResult{}, e
	}
	if _, e := a.w.Environment(ctx); e != nil {
		return dw.TextResult{}, e
	}
	start := time.Now().UTC()
	t, e := a.readTextNative(ctx, r.Target)
	if e != nil {
		return dw.TextResult{}, e
	}
	if e = a.check(ctx, in, false); e != nil {
		return dw.TextResult{}, e
	}
	out := dw.TextResult{Target: r.Target, Text: t.Value, Source: t.Source, Coverage: dw.Coverage{Scope: dw.Scope{Refs: []dw.Ref{r.Target}}, Fields: []string{"text"}, Complete: true, SampleStart: start, SampleEnd: time.Now().UTC()}}
	if t.Value.Status != dw.FactKnown {
		return out, nil
	}
	text := *t.Value.Value
	sum := sha256.Sum256([]byte(t.Source + "\x00" + text))
	a.w.mu.Lock()
	if a.textVersions == nil {
		a.textVersions = map[dw.Ref]textState{}
	}
	state, ok := a.textVersions[r.Target]
	if !ok || state.digest != sum {
		a.nextTextVersion++
		state = textState{sum, a.nextTextVersion}
		a.textVersions[r.Target] = state
	}
	out.TextVersion = state.version
	a.w.mu.Unlock()
	offset := r.Offset
	if r.Continuation != "" {
		a.w.mu.Lock()
		p, ok := a.texts[r.Continuation]
		a.w.mu.Unlock()
		if !ok || time.Now().After(p.expires) {
			return dw.TextResult{}, fault("continuation_expired")
		}
		if p.target != r.Target {
			return dw.TextResult{}, dw.Invalid("text continuation target mismatch")
		}
		if p.version != out.TextVersion || p.source != t.Source {
			return dw.TextResult{}, fault("text_changed")
		}
		if r.Offset != 0 && r.Offset != p.offset {
			return dw.TextResult{}, dw.Invalid("text continuation offset mismatch")
		}
		offset = p.offset
	}
	runes := []rune(text)
	if offset > len(runes) {
		return dw.TextResult{}, dw.Invalid("offset beyond text")
	}
	end := offset + r.LimitRunes
	if end > len(runes) {
		end = len(runes)
	}
	s := string(runes[offset:end])
	out.Text.Value = &s
	out.Truncated = end < len(runes)
	out.Coverage.Truncated = out.Truncated
	out.Coverage.Complete = !out.Truncated
	if out.Truncated {
		a.w.mu.Lock()
		defer a.w.mu.Unlock()
		a.pruneLocked()
		if len(a.texts) >= a.w.opts.ViewLimit {
			return dw.TextResult{}, fault("resource_exhausted")
		}
		out.Next = token("t-")
		a.texts[out.Next] = textPage{r.Target, out.TextVersion, end, t.Source, time.Now().Add(a.w.opts.HistoryTTL)}
		out.Coverage.Continuation = out.Next
	}
	return out, nil
}
func (a *actor) ResolveAnchor(ctx context.Context, an dw.Anchor) (dw.ResolvedAnchor, error) {
	if e := (dw.Target{Anchor: &an}).Validate(); e != nil {
		return dw.ResolvedAnchor{}, e
	}
	in := dw.Intent{Operation: "resolve_anchor", Targets: []dw.Ref{an.Target}, Fields: []string{"bounds"}}
	if e := a.check(ctx, in, false); e != nil {
		return dw.ResolvedAnchor{}, e
	}
	if _, e := a.w.Environment(ctx); e != nil {
		return dw.ResolvedAnchor{}, e
	}
	p, e := a.point(ctx, dw.Target{Anchor: &an})
	if e != nil {
		return dw.ResolvedAnchor{Anchor: an, Reason: asFault(e).Code}, e
	}
	if e = a.check(ctx, in, false); e != nil {
		return dw.ResolvedAnchor{}, e
	}
	a.w.mu.Lock()
	v := a.w.objects[an.Target].object.GeometryVersion
	a.w.mu.Unlock()
	return dw.ResolvedAnchor{Anchor: an, Point: p, GeometryVersion: v, Valid: true}, nil
}
func (a *actor) Capture(ctx context.Context, r dw.CaptureRequest) (dw.CaptureResult, error) {
	if r.Kind != "visible_region" && r.Kind != "window_content" {
		return dw.CaptureResult{}, dw.Invalid("unknown capture kind")
	}
	if r.MaxPixelWidth < 0 || r.MaxPixelHeight < 0 || r.MaxPixelWidth > 8192 || r.MaxPixelHeight > 8192 {
		return dw.CaptureResult{}, dw.Invalid("invalid capture size")
	}
	if r.MaxPixelWidth == 0 {
		r.MaxPixelWidth = 1920
	}
	if r.MaxPixelHeight == 0 {
		r.MaxPixelHeight = 1080
	}
	if r.Kind == "window_content" && (r.Region != nil || r.IncludeCursor) {
		return dw.CaptureResult{}, dw.Invalid("window_content uses the full target window, without region or cursor")
	}
	in := dw.Intent{Operation: "capture", CaptureKind: r.Kind}
	if r.Kind == "visible_region" {
		in.Scope = dw.Scope{Desktop: true}
	} else {
		if r.Target == "" {
			return dw.CaptureResult{}, dw.Invalid("window_content needs target")
		}
		in.Targets = []dw.Ref{r.Target}
	}
	if e := a.check(ctx, in, false); e != nil {
		return dw.CaptureResult{}, e
	}
	env, e := a.w.Environment(ctx)
	if e != nil {
		return dw.CaptureResult{}, e
	}
	native := backend.CaptureRequest{CaptureRequest: r}
	if r.Target != "" {
		if e = a.check(ctx, dw.Intent{Operation: "capture", Targets: []dw.Ref{r.Target}, CaptureKind: r.Kind}, false); e != nil {
			return dw.CaptureResult{}, e
		}
		o, e := a.w.read(ctx, r.Target)
		if e != nil {
			return dw.CaptureResult{}, e
		}
		if r.Kind == "window_content" && o.Kind != dw.KindWindow {
			return dw.CaptureResult{}, dw.Invalid("window_content target must be a window")
		}
		a.w.mu.Lock()
		native.Key = a.w.objects[r.Target].key
		a.w.mu.Unlock()
		if r.Kind == "visible_region" && r.Region == nil && o.Bounds.Value != nil {
			b := *o.Bounds.Value
			r.Region = &b
		}
	}
	if r.Region != nil {
		b := r.Region
		if b.Frame == "" || b.Topology != env.Topology || !dw.Finite(b.Rect.X) || !dw.Finite(b.Rect.Y) || !dw.Finite(b.Rect.Width) || !dw.Finite(b.Rect.Height) || b.Rect.Width <= 0 || b.Rect.Height <= 0 || b.Rect.Width > 100000 || b.Rect.Height > 100000 {
			return dw.CaptureResult{}, dw.Invalid("invalid capture region")
		}
	}
	native.CaptureRequest = r
	v, e := a.w.call(ctx, func() (any, error) { return a.w.driver.Capture(ctx, native) })
	if e != nil {
		return dw.CaptureResult{}, e
	}
	if e = a.check(ctx, in, false); e != nil {
		return dw.CaptureResult{}, e
	}
	images := v.([]backend.Image)
	if len(images) == 0 || (r.Kind == "window_content" && len(images) != 1) {
		return dw.CaptureResult{}, fault("invalid_capture")
	}
	if len(images) > 16 {
		return dw.CaptureResult{}, fault("resource_exhausted")
	}
	total := 0
	for _, img := range images {
		total += len(img.Bytes)
		if img.Width <= 0 || img.Height <= 0 || img.Width > r.MaxPixelWidth || img.Height > r.MaxPixelHeight || img.Bounds.Topology != env.Topology || len(img.Bytes) == 0 || !dw.Finite(img.Bounds.Rect.Width) || !dw.Finite(img.Bounds.Rect.Height) || img.Bounds.Rect.Width <= 0 || img.Bounds.Rect.Height <= 0 || total > 32<<20 {
			return dw.CaptureResult{}, fault("invalid_capture")
		}
	}
	a.w.mu.Lock()
	defer a.w.mu.Unlock()
	a.pruneLocked()
	if len(a.assets)+len(images) > 64 {
		return dw.CaptureResult{}, fault("resource_exhausted")
	}
	out := dw.CaptureResult{RelatedRevision: a.w.revision, ExpiresAt: time.Now().Add(a.w.opts.AssetTTL)}
	for _, img := range images {
		id := dw.AssetID(token("asset-"))
		asset := dw.Asset{ContentType: img.ContentType, Bytes: append([]byte{}, img.Bytes...), ExpiresAt: out.ExpiresAt}
		a.assets[id] = assetRecord{asset, in, a.w.permissionVersion}
		tile := dw.CaptureTile{Asset: id, ImageFrame: dw.FrameID(token("image-")), DesktopFrame: img.Bounds.Frame, PixelWidth: img.Width, PixelHeight: img.Height, ImageToDesktop: dw.Transform2D{A: img.Bounds.Rect.Width / float64(img.Width), D: img.Bounds.Rect.Height / float64(img.Height), TX: img.Bounds.Rect.X, TY: img.Bounds.Rect.Y}, Topology: img.Bounds.Topology, CapturedAt: time.Now().UTC(), Kind: r.Kind}
		if r.Kind == "window_content" {
			tile.Target = r.Target
			tile.DesktopFrame = ""
			tile.ImageToDesktop = dw.Transform2D{}
			tile.ImageToTarget = dw.Transform2D{A: img.Bounds.Rect.Width / float64(img.Width), D: img.Bounds.Rect.Height / float64(img.Height)}
		}
		out.Tiles = append(out.Tiles, tile)
	}
	return out, nil
}
func (a *actor) ReadAsset(ctx context.Context, id dw.AssetID) (dw.Asset, error) {
	if e := a.check(ctx, dw.Intent{Operation: "read_asset"}, false); e != nil {
		return dw.Asset{}, e
	}
	if _, e := a.w.Environment(ctx); e != nil {
		return dw.Asset{}, e
	}
	a.w.mu.Lock()
	record, ok := a.assets[id]
	perm := a.w.permissionVersion
	a.w.mu.Unlock()
	if !ok || time.Now().After(record.asset.ExpiresAt) || record.permissionVersion != perm {
		return dw.Asset{}, fault("asset_expired")
	}
	if e := a.check(ctx, record.intent, false); e != nil {
		return dw.Asset{}, e
	}
	return copyOf(record.asset), nil
}
