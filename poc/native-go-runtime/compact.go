package main

import (
	"fmt"
	"strings"
)

// compactExec is an opt-in POC projection of one exec result. The supervisor
// retains the full record and the original native receipts for status/result.
// Only model-facing, routine terminal output changes here.
func compactExec(full map[string]any) map[string]any {
	out := map[string]any{}
	for _, key := range []string{"state", "print", "error", "native_error"} {
		if value, ok := full[key]; ok {
			out[key] = value
		}
	}
	state, _ := full["state"].(string)
	if state != "completed" || full["native_error"] != nil {
		out["execution_id"] = full["execution_id"]
		for _, key := range []string{"native_request_ids", "native_pending_ids"} {
			if value, ok := full[key]; ok {
				out[key] = value
			}
		}
	}
	if observations, ok := full["observations"].([]map[string]any); ok {
		var exceptional []map[string]any
		for _, observation := range observations {
			if observation["complete"] == true && observation["dirty"] != true &&
				observation["truncated"] != true && observation["more"] != true &&
				observation["unavailable_sources"] == nil {
				continue
			}
			item := map[string]any{"complete": observation["complete"]}
			for _, key := range []string{"native_request_id", "dirty", "truncated", "more", "unavailable_sources"} {
				if value, exists := observation[key]; exists && value != false {
					item[key] = value
				}
			}
			exceptional = append(exceptional, item)
		}
		if len(exceptional) > 0 {
			out["observations"] = exceptional
			out["execution_id"] = full["execution_id"]
		}
	}
	if actions, ok := full["actions"].([]map[string]any); ok {
		projected := make([]map[string]any, 0, len(actions))
		for _, action := range actions {
			item := map[string]any{"outcome": action["outcome"]}
			outcome, _ := action["outcome"].(string)
			abnormal := outcome != "completed" || action["seat_health"] != "ready"
			if steps, ok := action["steps"].([]map[string]any); ok {
				if len(steps) == 0 {
					abnormal = true
				}
				briefSteps := make([]map[string]any, 0, len(steps))
				for _, step := range steps {
					entry := map[string]any{"id": step["id"], "channel": step["channel"], "verification": step["verification"]}
					if delivery := step["delivery"]; delivery != "not_applicable" {
						entry["delivery"] = delivery
					}
					if fault, exists := step["fault"]; exists {
						entry["fault"] = fault
						abnormal = true
					}
					if step["delivery"] == "unknown" || step["verification"] != "verified" {
						abnormal = true
					}
					briefSteps = append(briefSteps, entry)
				}
				item["steps"] = briefSteps
			}
			if restoration := action["restoration"]; restoration != nil && restoration != "not_borrowed" {
				item["restoration"] = restoration
				if restoration != "restored" {
					abnormal = true
				}
			}
			if abnormal {
				for _, key := range []string{"native_request_id", "run_id", "seat_health", "restoration_reason"} {
					if value, exists := action[key]; exists {
						item[key] = value
					}
				}
				out["execution_id"] = full["execution_id"]
			}
			projected = append(projected, item)
		}
		out["actions"] = projected
	}
	if captures, ok := full["captures"].([]map[string]any); ok {
		projected := make([]map[string]any, 0, len(captures))
		for _, capture := range captures {
			projected = append(projected, map[string]any{"images": capture["images"]})
		}
		out["captures"] = projected
	}
	return out
}

func compactToolText(out map[string]any) string {
	state, _ := out["state"].(string)
	var lines []string
	if values, ok := out["print"].([]string); ok {
		lines = append(lines, values...)
	}
	if state != "completed" && out["error"] == nil && out["native_error"] == nil {
		lines = append(lines, fmt.Sprintf("state: %v; execution_id: %v", state, out["execution_id"]))
	}
	if observations, ok := out["observations"].([]map[string]any); ok {
		for _, observation := range observations {
			var reasons []string
			if observation["dirty"] == true {
				reasons = append(reasons, "dirty")
			}
			if observation["truncated"] == true {
				reasons = append(reasons, "truncated")
			}
			if observation["more"] == true {
				reasons = append(reasons, "more")
			}
			if sources, ok := observation["unavailable_sources"].([]string); ok {
				reasons = append(reasons, sources...)
			}
			lines = append(lines, fmt.Sprintf("observation incomplete (%s); execution_id: %v", strings.Join(reasons, ", "), out["execution_id"]))
		}
	}
	if actions, ok := out["actions"].([]map[string]any); ok {
		for _, action := range actions {
			if steps, ok := action["steps"].([]map[string]any); ok {
				for _, step := range steps {
					line := fmt.Sprintf("%v: %v, %v via %v", step["id"], action["outcome"], step["verification"], step["channel"])
					if step["delivery"] == "unknown" {
						line += "; delivery unknown"
					}
					if fault, ok := step["fault"]; ok {
						line += fmt.Sprintf("; fault=%v", fault)
					}
					if restoration, ok := action["restoration"]; ok {
						line += fmt.Sprintf("; restoration=%v", restoration)
					}
					if seat, ok := action["seat_health"]; ok {
						line += fmt.Sprintf("; seat_health=%v", seat)
					}
					if action["outcome"] != "completed" || action["run_id"] != nil {
						line += fmt.Sprintf("; execution_id: %v", out["execution_id"])
					}
					lines = append(lines, line)
				}
			} else {
				lines = append(lines, fmt.Sprintf("action %v; execution_id: %v", action["outcome"], out["execution_id"]))
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
		line := fmt.Sprintf("native %v", err["code"])
		if retry, ok := err["retry_class"]; ok {
			line += fmt.Sprintf(" (%v)", retry)
		}
		lines = append(lines, fmt.Sprintf("%s; execution_id: %v", line, out["execution_id"]))
	}
	if err, ok := out["error"].(map[string]any); ok && out["native_error"] == nil {
		lines = append(lines, fmt.Sprintf("%v: %v; execution_id: %v", err["code"], err["message"], out["execution_id"]))
	}
	if len(lines) > 0 {
		return strings.Join(lines, "\n")
	}
	if state == "" {
		return "unknown state"
	}
	return state
}

// Keep the machine-readable exceptional state in structuredContent while the
// ordinary facts have one model-visible home in TextContent.
func compactStructured(out map[string]any) map[string]any {
	result := make(map[string]any, len(out))
	for key, value := range out {
		result[key] = value
	}
	delete(result, "print")
	if actions, ok := result["actions"].([]map[string]any); ok {
		allVerified := true
		for _, action := range actions {
			if action["outcome"] != "completed" || action["run_id"] != nil || action["seat_health"] != nil {
				allVerified = false
			}
			if steps, ok := action["steps"].([]map[string]any); ok {
				for _, step := range steps {
					if step["verification"] != "verified" || step["delivery"] == "unknown" {
						allVerified = false
					}
				}
			}
		}
		if allVerified {
			delete(result, "actions")
		}
	}
	if captures, ok := result["captures"].([]map[string]any); ok {
		allReady := true
		for _, capture := range captures {
			if count, ok := capture["images"].(int); !ok || count < 1 {
				allReady = false
			}
		}
		if allReady {
			delete(result, "captures")
		}
	}
	return result
}
