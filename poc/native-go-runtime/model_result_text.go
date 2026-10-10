package main

import (
	"encoding/json"
	"fmt"
	"strings"
)

// modelResultText keeps a single model-visible representation. A full result
// remains available by original ID only when explicitly requested.
func modelResultText(in input, full map[string]any) string {
	id := in.ExecutionID
	if id == "" {
		id = "?"
	}
	if in.Operation == "result" && in.Detail == "full" {
		body, err := json.MarshalIndent(full, "", "  ")
		if err != nil {
			return id + " · original result unavailable: " + err.Error()
		}
		return id + " · original result\n" + string(body)
	}
	view := compactExec(full)
	body := compactToolText(view)
	body = strings.ReplaceAll(body, "; execution_id: "+id, "")
	body = strings.ReplaceAll(body, "execution_id: "+id, "")
	body = strings.TrimSpace(body)
	if nativeErr, ok := view["native_error"].(map[string]any); ok {
		message, _ := nativeErr["message"].(string)
		code, _ := nativeErr["code"].(string)
		if message != "" && message != code {
			body += ": " + message
		}
		if code == "native_unknown" {
			body += "; outcome uncertain, query result(" + id + "); do not repeat"
		}
	}
	if body == "" || body == "completed" {
		body = "ok"
	}
	if in.Operation == "status" && view["error"] == nil {
		body = fmt.Sprint(view["state"])
	}
	return id + " · " + body
}
