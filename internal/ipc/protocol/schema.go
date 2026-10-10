package protocol

import (
	"reflect"
	"regexp"
	"strings"
	"time"

	dw "github.com/caelis-labs/desktop-world/internal/world"
)

type schema = map[string]any

// ArgumentsSchema describes the actual wire names and units. Runtime validation
// remains authoritative for cross-field, scope and lifecycle constraints.
func ArgumentsSchema(operation string) map[string]any {
	var value any
	switch operation {
	case "world.observe":
		value = dw.ObserveRequest{}
	case "world.read":
		value = dw.TextRequest{}
	case "world.sync":
		value = dw.ChangeRequest{}
	case "world.act":
		value = dw.Plan{}
	case "world.capture":
		value = dw.CaptureRequest{}
	case "world.run.get", "world.run.cancel":
		value = struct{ RunID dw.RunID }{}
	default:
		return nil
	}
	return typeSchema(reflect.TypeOf(value))
}

var schemaAcronym = regexp.MustCompile(`([A-Z]+)([A-Z][a-z])`)
var schemaWord = regexp.MustCompile(`([a-z0-9])([A-Z])`)

func typeSchema(t reflect.Type) schema {
	if t.Kind() == reflect.Pointer {
		return typeSchema(t.Elem())
	}
	if t == reflect.TypeOf(time.Time{}) {
		return schema{"type": "string", "format": "date-time"}
	}
	if t == reflect.TypeOf(time.Duration(0)) {
		return schema{"type": "integer", "minimum": 0, "maximum": 3600000, "description": "Integer milliseconds."}
	}
	if t == reflect.TypeOf(dw.Version(0)) || t == reflect.TypeOf(dw.Revision(0)) {
		return schema{"type": "string", "pattern": "^(0|[1-9][0-9]*)$", "description": "Unsigned decimal string, never a JSON number."}
	}
	var s schema
	switch t.Kind() {
	case reflect.String:
		s = schema{"type": "string"}
	case reflect.Bool:
		s = schema{"type": "boolean"}
	case reflect.Int, reflect.Int64, reflect.Uint64:
		s = schema{"type": "integer"}
	case reflect.Float64:
		s = schema{"type": "number"}
	case reflect.Slice:
		s = schema{"type": "array", "items": typeSchema(t.Elem())}
	case reflect.Map:
		s = schema{"type": "object", "additionalProperties": typeSchema(t.Elem())}
	case reflect.Struct:
		properties := schema{}
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if !f.IsExported() {
				continue
			}
			name := strings.ToLower(schemaWord.ReplaceAllString(schemaAcronym.ReplaceAllString(f.Name, "${1}_${2}"), "${1}_${2}"))
			if f.Type == reflect.TypeOf(time.Duration(0)) {
				name += "_ms"
			}
			properties[name] = typeSchema(f.Type)
		}
		s = schema{"type": "object", "properties": properties, "additionalProperties": false}
	default:
		panic("unsupported schema type: " + t.String())
	}
	if t.Kind() != reflect.Struct {
		return s
	}
	p := s["properties"].(schema)
	require := func(names ...string) { s["required"] = names }
	field := func(n string) schema { return p[n].(schema) }
	enum := func(n string, values ...string) { field(n)["enum"] = values }
	bound := func(n string, min, max any) { field(n)["minimum"] = min; field(n)["maximum"] = max }
	switch t {
	case reflect.TypeOf(dw.Scope{}):
		s["oneOf"] = []any{schema{"required": []string{"desktop"}, "properties": schema{"desktop": schema{"const": true}}, "not": schema{"required": []string{"refs"}}}, schema{"required": []string{"refs"}, "properties": schema{"refs": schema{"minItems": 1}, "desktop": schema{"const": false}}}}
	case reflect.TypeOf(dw.Target{}):
		arms := []any{}
		for _, n := range []string{"ref", "bound", "point", "anchor"} {
			arms = append(arms, schema{"required": []string{n}, "maxProperties": 1})
		}
		s["oneOf"] = arms
	case reflect.TypeOf(dw.Point{}):
		require("frame", "topology", "x", "y")
		bound("x", -1e7, 1e7)
		bound("y", -1e7, 1e7)
	case reflect.TypeOf(dw.Anchor{}):
		require("target", "u", "v")
		bound("u", 0, 1)
		bound("v", 0, 1)
	case reflect.TypeOf(dw.Budget{}):
		bound("max_results", 0, 1024)
		bound("max_depth", 0, 32)
		bound("max_visited_nodes", 0, 10000)
		bound("max_output_bytes", 0, 1<<20)
		bound("max_text_runes", 0, 4096)
		bound("read_deadline_ms", 0, 10000)
	case reflect.TypeOf(dw.Freshness{}):
		enum("mode", "cached", "max_age", "refresh")
	case reflect.TypeOf(dw.Locator{}):
		require("within")
		enum("kind", "application", "window", "ui")
		bound("max_depth", 0, 32)
		field("name_equals")["description"] = "Exact name, not a label inferred from nearby text."
		field("name_contains")["description"] = "Explicit substring discovery, not identity."
	case reflect.TypeOf(dw.ObserveRequest{}):
		require("scope")
		enum("projection", "summary", "outline", "detail", "capture_windows")
		field("projection")["description"] = "Start summary with fields [name,role]; inspect one returned window using outline. Increase depth only as needed. For pixels, opt into capture_windows within an application Ref; capture returned window Refs explicitly."
		field("fields")["items"] = schema{"type": "string", "enum": []string{"kind", "role", "name", "value_preview", "uri", "states", "bounds", "capabilities", "app", "window", "parent", "relations", "lifecycle"}}
		field("fields")["uniqueItems"] = true
	case reflect.TypeOf(dw.TextRequest{}):
		require("target")
		bound("offset", 0, 1e9)
		bound("limit_runes", 0, 4096)
	case reflect.TypeOf(dw.ChangeRequest{}):
		require("cursor")
		bound("max_output_bytes", 0, 1<<20)
		bound("wait_ms", 0, 10000)
	case reflect.TypeOf(dw.CaptureRequest{}):
		enum("kind", "visible_region", "window_content")
		bound("max_pixel_width", 0, 8192)
		bound("max_pixel_height", 0, 8192)
		s["description"] = "Capture is separately authorized; visible_region may include occluding apps. window_content requires an observed capture window, full target only, no cursor/region; coordinates are target-local, never desktop input authority."
	case reflect.TypeOf(dw.Plan{}):
		require("epoch", "request_id", "steps")
		field("request_id")["description"] = "Current epoch + ':' + caller-generated stable ID. Preserve the same ID and body for retry; never replay unknown effects with a new ID."
		field("steps")["minItems"] = 1
		field("steps")["maxItems"] = 16
		bound("timeout_ms", 0, 10000)
	case reflect.TypeOf(dw.Step{}):
		require("id", "op")
		enum("op", "bind", "bind_focus", "wait", "focus", "invoke", "set_value", "set_expanded", "set_checked", "set_selected", "scroll_into_view", "pointer.move", "pointer.click", "pointer.drag", "pointer.scroll", "keyboard.press", "keyboard.type_text")
		enum("completion", "dispatch", "verify")
		bound("timeout_ms", 0, 10000)
		s["description"] = "Ordered step. bind uses bind only; wait uses after predicates without target; other ops require exactly one target. set_value uses set_value, set_expanded/set_checked/set_selected use explicit desired booleans; scroll_into_view has no argument arm. keyboard.type_text uses type_text, keyboard.press uses press, pointer.* uses click/drag/scroll as applicable. focus/invoke/pointer.move have no argument arm. verify needs after except focus/set_value/set_expanded/set_checked/set_selected/scroll_into_view (always verified). Do not batch an unknown future dialog: observe its new Ref first."
		variants := []any{}
		for _, op := range []string{"bind", "bind_focus", "wait", "focus", "invoke", "set_value", "set_expanded", "set_checked", "set_selected", "scroll_into_view", "pointer.move", "pointer.click", "pointer.drag", "pointer.scroll", "keyboard.press", "keyboard.type_text"} {
			arm := map[string]string{"bind": "bind", "bind_focus": "bind_focus", "set_value": "set_value", "set_expanded": "set_expanded", "set_checked": "set_checked", "set_selected": "set_selected", "pointer.click": "click", "pointer.drag": "drag", "pointer.scroll": "scroll", "keyboard.press": "press", "keyboard.type_text": "type_text"}[op]
			props := schema{"op": schema{"const": op}}
			required := []string{"op"}
			if op != "bind" && op != "bind_focus" && op != "wait" {
				required = append(required, "target")
			}
			if arm != "" {
				required = append(required, arm)
			}
			if op == "wait" {
				required = append(required, "after")
				props["after"] = schema{"minItems": 1}
			}
			forbid := []any{}
			for _, candidate := range []string{"bind", "bind_focus", "set_value", "set_expanded", "set_checked", "set_selected", "type_text", "press", "click", "drag", "scroll"} {
				if candidate != arm {
					forbid = append(forbid, schema{"required": []string{candidate}})
				}
			}
			if op == "bind" || op == "bind_focus" || op == "wait" {
				forbid = append(forbid, schema{"required": []string{"target"}})
			}
			variants = append(variants, schema{"properties": props, "required": required, "not": schema{"anyOf": forbid}})
		}
		s["oneOf"] = variants
	case reflect.TypeOf(dw.BindFocus{}):
		require("name", "within")
		field("name")["minLength"] = 1
		field("name")["maxLength"] = 128
	case reflect.TypeOf(dw.Bind{}):
		require("name", "locator", "require_unique")
		field("require_unique")["const"] = true
	case reflect.TypeOf(dw.Predicate{}):
		require("target", "property")
		enum("property", "name", "value", "role", "lifecycle", "version", "geometry_version", "exists", "foreground", "enabled", "focused", "selected", "checked", "expanded", "offscreen", "read_only", "protected")
		s["oneOf"] = []any{schema{"required": []string{"equals_string"}}, schema{"required": []string{"equals_bool"}}, schema{"required": []string{"equals_version"}}}
	case reflect.TypeOf(dw.SetExpanded{}):
		require("expanded")
		s["description"] = "Reach the explicit expanded state using semantic provider support; always verified, no toggle or physical-input fallback."
	case reflect.TypeOf(dw.SetChecked{}):
		require("checked")
		s["description"] = "Reach explicit checked state using semantic provider support; always verified. No physical-input fallback or replay of an uncertain toggle."
	case reflect.TypeOf(dw.SetSelected{}):
		require("selected")
		s["description"] = "Set this item's selected state; never explicitly clear other items. Provider selection rules may reject or adjust selections. Always verified; no physical-input fallback."
	case reflect.TypeOf(dw.SetValue{}), reflect.TypeOf(dw.TypeText{}):
		require("text")
		field("text")["maxLength"] = 65536
		field("text")["description"] = "UTF-8 without NUL, at most 65536 bytes. type_text requires verified focus; cooperative host mode limits a burst to 256 UTF-16 units (newline/Tab are real keys)."
	case reflect.TypeOf(dw.KeyChord{}):
		require("key")
		field("key")["pattern"] = "^([A-Z0-9]|Enter|Tab|Escape|Backspace|Delete|Space|Left|Right|Up|Down|Home|End|PageUp|PageDown)$"
		field("modifiers")["items"] = schema{"type": "string", "enum": []string{"primary", "meta", "control", "alt", "shift"}}
		field("modifiers")["uniqueItems"] = true
		s["description"] = "primary=Command on macOS, Control on Windows. Do not combine primary with meta/control."
	case reflect.TypeOf(dw.Click{}):
		require("button", "count")
		enum("button", "left", "right", "middle")
		bound("count", 1, 2)
	case reflect.TypeOf(dw.Drag{}):
		require("to")
		bound("duration_ms", 0, 2000)
		s["description"] = "Cooperative host mode limits a drag to 500 ms and both endpoints to the same observed window; absolute Points are unavailable in that mode."
	case reflect.TypeOf(dw.Scroll{}):
		require("unit")
		field("unit")["const"] = "wheel_step"
		bound("dx", -100, 100)
		bound("dy", -100, 100)
	default:
		if _, ok := p["run_id"]; ok {
			require("run_id")
		}
	}
	return s
}
