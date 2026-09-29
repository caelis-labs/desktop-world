// dw-analyze measures recorded SDK traffic without operating the desktop.
package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/caelis-labs/desktop-world/protocol"
)

type row struct {
	Op, Started, Finished, Error string
	Request                      json.RawMessage
	Response                     json.RawMessage
	RequestBytes                 int `json:"request_bytes"`
	ResponseBytes                int `json:"response_bytes"`
}

func main() {
	input := flag.String("input", "", "SDK calls.jsonl to analyze")
	flag.Parse()
	if *input == "" {
		fmt.Fprintln(os.Stderr, "use -input calls.jsonl")
		os.Exit(2)
	}
	f, err := os.Open(*input)
	if err != nil {
		panic(err)
	}
	defer f.Close()
	counts, bytesByOp, fieldBytes := map[string]int{}, map[string]int{}, map[string]int{}
	errorCounts := map[string]int{}
	seen := map[string]bool{}
	reqBytes, fullBytes, compactBytes, repeated, objectCount, calls := 0, 0, 0, 0, 0, 0
	elapsed := 0.0
	top := []map[string]any{}
	firstVisible := ""
	scan := bufio.NewScanner(f)
	scan.Buffer(make([]byte, 4096), 8<<20)
	for scan.Scan() {
		calls++
		var r row
		if err := json.Unmarshal(scan.Bytes(), &r); err != nil {
			panic(err)
		}
		counts[r.Op]++
		bytesByOp[r.Op] += r.ResponseBytes
		a, _ := time.Parse(time.RFC3339Nano, r.Started)
		b, _ := time.Parse(time.RFC3339Nano, r.Finished)
		elapsed += b.Sub(a).Seconds()
		if !strings.HasPrefix(r.Op, "world.") {
			continue
		}
		reqBytes += r.RequestBytes
		fullBytes += r.ResponseBytes
		compact, err := protocol.CompactJSON(r.Response)
		if err != nil {
			panic(err)
		}
		compactBytes += len(compact)
		var result map[string]json.RawMessage
		_ = json.Unmarshal(r.Response, &result)
		if nested := result["result"]; nested != nil {
			if e := result["error"]; e != nil {
				var fault struct{ Code string }
				_ = json.Unmarshal(e, &fault)
				if fault.Code != "" {
					errorCounts[fault.Code]++
				}
			}
			_ = json.Unmarshal(nested, &result)
		}
		if r.Error != "" {
			errorCounts[strings.SplitN(r.Error, ":", 2)[0]]++
		}
		var objects []map[string]json.RawMessage
		_ = json.Unmarshal(result["objects"], &objects)
		if len(objects) > 0 && firstVisible == "" {
			firstVisible = r.Finished
		}
		objectCount += len(objects)
		for _, o := range objects {
			for k, v := range o {
				key, _ := json.Marshal(k)
				fieldBytes[k] += len(key) + 1 + len(v)
			}
			delete(o, "sample_start")
			delete(o, "sample_end")
			data, _ := json.Marshal(o)
			data, _ = protocol.CompactJSON(data)
			if seen[string(data)] {
				repeated++
			}
			seen[string(data)] = true
		}
		top = append(top, map[string]any{"line": calls, "op": r.Op, "response_bytes": r.ResponseBytes, "compact_bytes": len(compact)})
	}
	if err := scan.Err(); err != nil {
		panic(err)
	}
	sort.Slice(top, func(i, j int) bool { return top[i]["response_bytes"].(int) > top[j]["response_bytes"].(int) })
	if len(top) > 10 {
		top = top[:10]
	}
	out := map[string]any{"record_count": calls, "operations": counts, "response_bytes_by_operation": bytesByOp, "world_request_bytes": reqBytes, "world_response_bytes": fullBytes, "compact_replay_response_bytes": compactBytes, "compact_replay_reduction": 1 - float64(compactBytes)/float64(fullBytes), "object_count": objectCount, "identical_objects_ignoring_sampling_time": repeated, "object_field_bytes_excluding_braces_commas": fieldBytes, "error_counts": errorCounts, "api_elapsed_seconds_sum": elapsed, "first_visible_observe_finished": firstVisible, "largest_responses": top, "model_tokens": "unavailable: bytes and offline presentation replay are not model token usage or new task timings"}
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	if err := enc.Encode(out); err != nil {
		panic(err)
	}
}
