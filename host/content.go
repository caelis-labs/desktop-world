package host

import (
	"encoding/json"

	"github.com/caelis-labs/desktop-world/protocol"
)

type TextContent struct {
	Type string `json:"type"`
	Text string `json:"text"`
}
type ToolResult struct {
	IsError           bool            `json:"isError,omitempty"`
	Content           []TextContent   `json:"content"`
	StructuredContent json.RawMessage `json:"structuredContent"`
}

// Content projects a reply to Caelis content-v1's 32 KiB text + structured
// budget, reserving 1 KiB for Runtime receipt fields. Full replies remain in
// Client for Reconcile. Oversize data is never silently clipped or reported as
// a failed/no-effect action: the model receives a bounded recovery notice.
func Content(reply Reply) ToolResult {
	body, err := json.Marshal(reply)
	if err == nil {
		body, err = protocol.CompactJSON(body)
	}
	var receipt struct {
		RunID      string `json:"run_id"`
		Outcome    string `json:"outcome"`
		SeatHealth string `json:"seat_health"`
		Input      *struct {
			Mode         string `json:"mode"`
			ForegroundMS int64  `json:"foreground_ms"`
			Restoration  string `json:"restoration"`
		} `json:"input"`
	}
	_ = json.Unmarshal(reply.Result, &receipt)
	isError := err != nil || reply.Error != nil || receipt.Outcome != "" && receipt.Outcome != "completed"
	if err != nil || 2*len(body)+1024 > 32*1024 {
		short := func(s string) string {
			if len(s) > 128 {
				return "unavailable"
			}
			return s
		}
		recovery := map[string]any{
			"error":               map[string]string{"code": "model_output_budget", "message": "Full reply retained by host. Use smaller read scope/fields/pages. For actions reconcile the original request; never repeat input to recover output."},
			"original_request_id": short(reply.ID), "run_id": short(receipt.RunID), "outcome": short(receipt.Outcome), "seat_health": short(receipt.SeatHealth),
		}
		if receipt.Input != nil {
			recovery["input"] = map[string]any{"mode": short(receipt.Input.Mode), "foreground_ms": receipt.Input.ForegroundMS, "restoration": short(receipt.Input.Restoration)}
		}
		body, _ = json.Marshal(recovery)
		isError = true
	}
	return ToolResult{IsError: isError, Content: []TextContent{{Type: "text", Text: string(body)}}, StructuredContent: body}
}
