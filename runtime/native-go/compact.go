package main

// compactExec projects one exec result for the model. The supervisor
// retains the full record and original native receipts for explicit queries.
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
					entry := map[string]any{"id": step["id"], "state": step["state"], "channel": step["channel"], "verification": step["verification"]}
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
