package auditlog

import (
	"os"
	"path/filepath"
	"testing"
)

func TestRotateAppendAndIncompleteRecord(t *testing.T) {
	path := filepath.Join(t.TempDir(), "audit.jsonl")
	first, e := Open(path, "rotate")
	if e != nil {
		t.Fatal(e)
	}
	first.WriteString("{}\n")
	second, e := Open(path, "rotate")
	if e != nil || second.Path == path {
		t.Fatal("collision did not rotate", e)
	}
	second.Close()
	first.Close()
	appended, e := Open(path, "append")
	if e != nil {
		t.Fatal(e)
	}
	if _, e = Open(path, "append"); e == nil {
		t.Fatal("two append writers")
	}
	appended.WriteString("{\"session\":2}\n")
	appended.Close()
	data, _ := os.ReadFile(path)
	if string(data) != "{}\n{\"session\":2}\n" {
		t.Fatal(string(data))
	}
	os.WriteFile(path, []byte("unfinished"), 0600)
	if _, e = Open(path, "append"); e == nil {
		t.Fatal("partial record appended")
	}
	data, _ = os.ReadFile(path)
	if string(data) != "unfinished" {
		t.Fatal("existing bytes changed")
	}
	if _, e = Open(path, "create"); e == nil {
		t.Fatal("exclusive create ignored collision")
	}
}
