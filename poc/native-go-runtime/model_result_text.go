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
	body := conciseToolText(view)
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

// conciseToolText is the default exec projection. Each action label is the JS
// step ID (or the object's printed ID), so a model can match the result to its
// script without reading a second receipt or an escaped JSON copy. Routine
// properties shared by consecutive steps appear once; exceptional delivery,
// verification and restoration states remain explicit.
func conciseToolText(out map[string]any) string {
	state, _ := out["state"].(string)
	var lines []string
	if prints, ok := out["print"].([]string); ok {
		lines = append(lines, prints...)
	}
	if state != "completed" && out["error"] == nil && out["native_error"] == nil {
		lines = append(lines, "state: "+state)
	}
	if observations, ok := out["observations"].([]map[string]any); ok {
		for _, observation := range observations {
			var reasons []string
			for _, key := range []string{"dirty", "truncated", "more"} {
				if observation[key] == true {
					reasons = append(reasons, key)
				}
			}
			if sources, ok := observation["unavailable_sources"].([]string); ok {
				reasons = append(reasons, sources...)
			}
			lines = append(lines, "observation incomplete ("+strings.Join(reasons, ", ")+")")
		}
	}
	if actions, ok := out["actions"].([]map[string]any); ok {
		for _, action := range actions {
			steps, ok := action["steps"].([]map[string]any)
			if !ok || len(steps) == 0 {
				lines = append(lines, "action: "+fmt.Sprint(action["outcome"]))
				continue
			}
			if outcome, _ := action["outcome"].(string); outcome != "completed" {
				lines = append(lines, "action: "+outcome)
			}
			for start := 0; start < len(steps); {
				end := start + 1
				for end < len(steps) && sameStepStatus(steps[start], steps[end]) {
					end++
				}
				ids := make([]string, 0, end-start)
				for _, step := range steps[start:end] {
					ids = append(ids, fmt.Sprint(step["id"]))
				}
				lines = append(lines, strings.Join(ids, ", ")+": "+stepStatus(action, steps[start]))
				start = end
			}
			if restoration, ok := action["restoration"].(string); ok && restoration != "not_borrowed" {
				lines = append(lines, "foreground restoration: "+restoration)
			}
			if seat, ok := action["seat_health"].(string); ok && seat != "ready" {
				lines = append(lines, "seat: "+seat)
			}
		}
	}
	if captures, ok := out["captures"].([]map[string]any); ok {
		for _, capture := range captures {
			if count, ok := capture["images"].(int); ok && count > 0 {
				lines = append(lines, fmt.Sprintf("capture: %d image(s) ready", count))
			} else {
				lines = append(lines, "capture: no image returned")
			}
		}
	}
	if err, ok := out["native_error"].(map[string]any); ok {
		line := "native " + fmt.Sprint(err["code"])
		if retry, ok := err["retry_class"]; ok {
			line += " (" + fmt.Sprint(retry) + ")"
		}
		lines = append(lines, line)
	} else if err, ok := out["error"].(map[string]any); ok {
		lines = append(lines, fmt.Sprintf("%v: %v", err["code"], err["message"]))
	}
	if len(lines) == 0 {
		if state == "completed" {
			return "ok"
		}
		return state
	}
	return strings.Join(lines, "\n")
}

func sameStepStatus(a, b map[string]any) bool {
	for _, key := range []string{"state", "channel", "verification", "delivery", "fault"} {
		if fmt.Sprint(a[key]) != fmt.Sprint(b[key]) {
			return false
		}
	}
	return true
}

func stepStatus(action, step map[string]any) string {
	outcome := fmt.Sprint(action["outcome"])
	state, _ := step["state"].(string)
	verification := fmt.Sprint(step["verification"])
	var status string
	switch {
	case state == "skipped":
		status = "skipped"
	case state == "unknown":
		status = "unknown; inspect original result before any new action"
	case state == "failed":
		status = "failed"
	case (state == "satisfied" || state == "") && verification == "verified":
		status = "verified"
	case (state == "dispatched" || state == "" && outcome == "completed") && verification == "not_requested":
		status = "dispatched; effect unverified"
	default:
		status = state
		if status == "" {
			status = outcome
		}
		status += "; verification=" + verification
	}
	if channel, ok := step["channel"].(string); ok && channel != "" {
		status += " via " + channel
	}
	if step["delivery"] == "unknown" {
		status += "; delivery unknown"
	} else if delivery, ok := step["delivery"].(string); ok && (delivery == "partial" || delivery == "none" && state != "skipped") {
		status += "; delivery=" + delivery
	}
	if fault, ok := step["fault"]; ok {
		status += "; fault=" + fmt.Sprint(fault)
	}
	return status
}
