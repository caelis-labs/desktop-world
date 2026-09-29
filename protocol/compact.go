package protocol

import (
	"bytes"
	"encoding/json"
)

// CompactJSON is a model-facing presentation, not the typed wire protocol.
// Known facts use {"known": value}; other fact statuses remain explicit. All
// values remain untrusted UI data. Per-object sample times are omitted; the
// observation's coverage interval, versions, lifecycle and coverage are kept.
// No object, field value, capability, receipt or uncertainty is filtered out.
// Keep the original wire response when exact per-object timestamps are needed.
func CompactJSON(data []byte) ([]byte, error) {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	return json.Marshal(compact(value))
}

func compact(value any) any {
	switch v := value.(type) {
	case []any:
		for i := range v {
			v[i] = compact(v[i])
		}
		return v
	case map[string]any:
		if status, ok := v["status"].(string); ok {
			if status == "known" {
				if data, exists := v["value"]; exists {
					out := map[string]any{"known": data}
					// Unusual provenance must not be silently relabeled ui_content.
					if source, ok := v["source"]; ok && source != "ui_content" {
						out["source"] = source
					}
					return out
				}
			}
			if status == "unknown" || status == "redacted" || status == "unsupported" || status == "unrequested" {
				return v
			}
		}
		if _, ok := v["ref"]; ok && v["kind"] != nil {
			delete(v, "sample_start")
			delete(v, "sample_end")
		}
		for key, child := range v {
			v[key] = compact(child)
		}
		return v
	default:
		return value
	}
}
