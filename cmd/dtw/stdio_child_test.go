package main

import (
	"context"
	"encoding/json"
	"os"
	"testing"
)

// TestStdioChild is the private subprocess entry used by integration tests.
func TestStdioChild(t *testing.T) {
	if os.Getenv("DTW_TEST_SCRIPT_CHILD") == "1" {
		if err := runScriptChild(); err != nil {
			t.Fatal(err)
		}
		return
	}
	if os.Getenv("DTW_TEST_CHILD") == "1" {
		if err := runServer(context.Background()); err != nil {
			t.Fatal(err)
		}
	}
}

func mustJSON(v any) []byte {
	data, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return data
}
