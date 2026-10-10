// Package wire is the versioned wire mapping, shared by budget accounting and
// protocol. Public Go structs deliberately carry no JSON serialization contract.
package wire

import (
	"bytes"
	"encoding/json"
	"fmt"
	dw "github.com/caelis-labs/desktop-world/internal/world"
	"io"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const Version = "desktop-world/0.1"

var durationType = reflect.TypeOf(time.Duration(0))
var timeType = reflect.TypeOf(time.Time{})
var revisionType = reflect.TypeOf(dw.Revision(0))
var versionType = reflect.TypeOf(dw.Version(0))
var acronym = regexp.MustCompile(`([A-Z]+)([A-Z][a-z])`)
var word = regexp.MustCompile(`([a-z0-9])([A-Z])`)

func name(f reflect.StructField) string {
	n := strings.ToLower(word.ReplaceAllString(acronym.ReplaceAllString(f.Name, "${1}_${2}"), "${1}_${2}"))
	if f.Type == durationType {
		n += "_ms"
	}
	return n
}
func Marshal(v any) ([]byte, error) {
	x, e := encode(reflect.ValueOf(v))
	if e != nil {
		return nil, e
	}
	return json.Marshal(x)
}
func encode(v reflect.Value) (any, error) {
	if !v.IsValid() {
		return nil, nil
	}
	if v.Kind() == reflect.Interface || v.Kind() == reflect.Pointer {
		if v.IsNil() {
			return nil, nil
		}
		return encode(v.Elem())
	}
	if v.Type() == reflect.TypeOf(json.RawMessage{}) {
		return v.Interface(), nil
	}
	if v.Type() == timeType {
		t := v.Interface().(time.Time)
		return t.UTC().Format(time.RFC3339Nano), nil
	}
	if v.Type() == durationType {
		return v.Int() / int64(time.Millisecond), nil
	}
	if v.Type() == revisionType || v.Type() == versionType {
		return strconv.FormatUint(v.Uint(), 10), nil
	}
	switch v.Kind() {
	case reflect.Struct:
		out := map[string]any{}
		typ := v.Type()
		for i := 0; i < v.NumField(); i++ {
			f := typ.Field(i)
			if !f.IsExported() {
				continue
			}
			fv := v.Field(i)
			if fv.IsZero() && !required(typ, f.Name) {
				continue
			}
			if fv.Kind() == reflect.Struct && strings.HasPrefix(fv.Type().Name(), "Fact[") && fv.FieldByName("Status").String() == "" {
				continue
			}
			x, e := encode(fv)
			if e != nil {
				return nil, e
			}
			out[name(f)] = x
		}
		return out, nil
	case reflect.Map:
		out := map[string]any{}
		it := v.MapRange()
		for it.Next() {
			x, e := encode(it.Value())
			if e != nil {
				return nil, e
			}
			out[it.Key().String()] = x
		}
		return out, nil
	case reflect.Slice, reflect.Array:
		out := make([]any, v.Len())
		for i := range out {
			x, e := encode(v.Index(i))
			if e != nil {
				return nil, e
			}
			out[i] = x
		}
		return out, nil
	default:
		return v.Interface(), nil
	}
}

// Unmarshal rejects duplicate keys, unknown fields, lossy numbers and trailing
// JSON. Size/depth limits apply before populating any Go values.
func Unmarshal(data []byte, out any) error {
	if len(data) > 1<<20 {
		return dw.Invalid("wire request exceeds 1 MiB")
	}
	if err := unique(data); err != nil {
		return err
	}
	v := reflect.ValueOf(out)
	if v.Kind() != reflect.Pointer || v.IsNil() {
		return fmt.Errorf("wire destination must be pointer")
	}
	return decode(data, v.Elem())
}
func unique(data []byte) error {
	d := json.NewDecoder(bytes.NewReader(data))
	d.UseNumber()
	var walk func(int) error
	walk = func(depth int) error {
		if depth > 64 {
			return dw.Invalid("JSON nesting exceeds 64")
		}
		t, e := d.Token()
		if e != nil {
			return dw.Invalid("invalid JSON")
		}
		delim, ok := t.(json.Delim)
		if !ok {
			return nil
		}
		switch delim {
		case '{':
			seen := map[string]bool{}
			for d.More() {
				k, e := d.Token()
				if e != nil {
					return dw.Invalid("invalid JSON key")
				}
				s, ok := k.(string)
				if !ok || seen[s] {
					return dw.Invalid("duplicate JSON key")
				}
				seen[s] = true
				if e = walk(depth + 1); e != nil {
					return e
				}
			}
		case '[':
			for d.More() {
				if e := walk(depth + 1); e != nil {
					return e
				}
			}
		default:
			return dw.Invalid("invalid JSON delimiter")
		}
		_, e = d.Token()
		return e
	}
	if e := walk(0); e != nil {
		return e
	}
	if _, e := d.Token(); e != io.EOF {
		return dw.Invalid("trailing JSON")
	}
	return nil
}
func decode(data []byte, v reflect.Value) error {
	if v.Type() == reflect.TypeOf(json.RawMessage{}) {
		v.SetBytes(append([]byte{}, data...))
		return nil
	}
	if v.Kind() == reflect.Pointer {
		if bytes.Equal(data, []byte("null")) {
			return dw.Invalid("explicit null is not allowed")
		}
		v.Set(reflect.New(v.Type().Elem()))
		return decode(data, v.Elem())
	}
	if v.Type() == timeType {
		var s string
		if json.Unmarshal(data, &s) != nil {
			return dw.Invalid("time requires RFC3339 string")
		}
		t, e := time.Parse(time.RFC3339Nano, s)
		if e != nil {
			return dw.Invalid("invalid timestamp")
		}
		v.Set(reflect.ValueOf(t))
		return nil
	}
	if v.Type() == revisionType || v.Type() == versionType {
		var s string
		if json.Unmarshal(data, &s) != nil {
			return dw.Invalid("revision/version requires decimal string")
		}
		n, e := strconv.ParseUint(s, 10, 64)
		if e != nil || strconv.FormatUint(n, 10) != s {
			return dw.Invalid("invalid decimal version")
		}
		v.SetUint(n)
		return nil
	}
	if v.Type() == durationType {
		var n int64
		if json.Unmarshal(data, &n) != nil || n < 0 || n > 3600000 {
			return dw.Invalid("duration requires bounded integer milliseconds")
		}
		v.SetInt(n * int64(time.Millisecond))
		return nil
	}
	switch v.Kind() {
	case reflect.Struct:
		var m map[string]json.RawMessage
		if json.Unmarshal(data, &m) != nil || m == nil {
			return dw.Invalid("expected object")
		}
		fields := map[string]int{}
		for i := 0; i < v.NumField(); i++ {
			f := v.Type().Field(i)
			if f.IsExported() {
				fields[name(f)] = i
			}
		}
		for k, b := range m {
			i, ok := fields[k]
			if !ok {
				return dw.Invalid("unknown field: " + k)
			}
			if e := decode(b, v.Field(i)); e != nil {
				return e
			}
		}
		return nil
	case reflect.Slice:
		var a []json.RawMessage
		if json.Unmarshal(data, &a) != nil || a == nil {
			return dw.Invalid("expected array")
		}
		v.Set(reflect.MakeSlice(v.Type(), len(a), len(a)))
		for i, b := range a {
			if e := decode(b, v.Index(i)); e != nil {
				return e
			}
		}
		return nil
	case reflect.Map:
		var m map[string]json.RawMessage
		if json.Unmarshal(data, &m) != nil || m == nil {
			return dw.Invalid("expected object")
		}
		v.Set(reflect.MakeMap(v.Type()))
		for k, b := range m {
			val := reflect.New(v.Type().Elem()).Elem()
			if e := decode(b, val); e != nil {
				return e
			}
			key := reflect.New(v.Type().Key()).Elem()
			key.SetString(k)
			v.SetMapIndex(key, val)
		}
		return nil
	default:
		if bytes.Equal(data, []byte("null")) {
			return dw.Invalid("null is not a value")
		}
		if e := json.Unmarshal(data, v.Addr().Interface()); e != nil {
			return dw.Invalid("incorrect wire value type")
		}
		return nil
	}
}

// Size includes the complete successful response envelope, including protocol.
func Size(epoch dw.Epoch, data any) int {
	b, e := Marshal(struct {
		Protocol string
		World    dw.Epoch
		Result   any
	}{Version, epoch, data})
	if e != nil {
		return int(^uint(0) >> 1)
	}
	return len(b)
}

func required(t reflect.Type, n string) bool {
	switch t.Name() {
	case "Coverage":
		return n == "Complete" || n == "Truncated" || n == "Dirty" || n == "VisitedNodes" || n == "MaxDepth"
	case "ChangeSet":
		return n == "From" || n == "To" || n == "ResetRequired"
	case "ResolvedAnchor":
		return n == "Valid"
	case "TextResult":
		return n == "Truncated"
	case "CaptureResult":
		return n == "Partial"
	}
	return false
}
