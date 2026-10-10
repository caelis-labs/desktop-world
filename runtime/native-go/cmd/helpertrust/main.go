// helpertrust opens the actual owned POC helper and reads its hello environment.
// No grant is declared, permission requested, desktop content read, or input sent.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
)

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: helpertrust ABSOLUTE_OWNED_HELPER_PATH")
		os.Exit(2)
	}
	path := os.Args[1]
	bytes, err := os.ReadFile(path)
	if err != nil {
		fail(err)
	}
	hash := sha256.Sum256(bytes)
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	client, err := host.Start(ctx, host.Options{Executable: path, InputMode: dw.InputModeCooperative})
	if err != nil {
		fail(err)
	}
	defer client.Close()
	var accessibility, screenCapture string
	for _, permission := range client.Hello.Environment.Permissions {
		if permission.Name == "accessibility" {
			accessibility = string(permission.State)
		} else if permission.Name == "screen_capture" {
			screenCapture = string(permission.State)
		}
	}
	row := map[string]any{
		"helper_path":    path,
		"helper_sha256":  hex.EncodeToString(hash[:]),
		"hello_protocol": client.Hello.Protocol,
		"hello_managed":  client.Hello.Managed,
		"helper_epoch":   client.Hello.Environment.Epoch,
		"accessibility":  accessibility,
		"screen_capture": screenCapture,
	}
	encoded, err := json.Marshal(row)
	if err != nil {
		fail(err)
	}
	fmt.Println(string(encoded))
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
