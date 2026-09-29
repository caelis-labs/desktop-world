// bot-host demonstrates the trusted application's side of managed helper IPC.
// Default: observe only. --fixture-title/--text is an explicit development-only
// write scope to a uniquely titled test fixture, never an agent-selected grant.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/protocol"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	binary := flag.String("helper", "", "absolute helper executable path")
	title := flag.String("fixture-title", "", "unique development fixture title; explicitly permits a write")
	text := flag.String("text", "Desktop World Bot alpha smoke 🌍", "text for the fixture named 内容")
	flag.Parse()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	c, err := host.Start(ctx, host.Options{Executable: *binary, Stderr: os.Stderr})
	if err != nil {
		return err
	}
	defer c.Close()
	turn := "bot-smoke-" + time.Now().UTC().Format("20060102T150405")
	if err = c.BeginTurn(ctx, turn); err != nil {
		return err
	}
	reply, err := c.Call(ctx, turn, "inventory", "observe", dw.ObserveRequest{Scope: dw.Scope{Desktop: true}, Projection: dw.ProjectionSummary, Fields: []string{"name", "role", "app"}, Budget: dw.Budget{MaxResults: 256, MaxOutputBytes: 131072}})
	if err != nil {
		return err
	}
	if reply.Error != nil {
		return reply.Error
	}
	if *title == "" {
		if err = c.EndTurn(ctx, turn); err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(host.Content(reply))
	}
	var ob dw.Observation
	if err = protocol.Decode(reply.Result, &ob); err != nil {
		return err
	}
	var window dw.Object
	count := 0
	for _, o := range ob.Objects {
		if o.Kind == dw.KindWindow && o.Name.Value != nil && *o.Name.Value == *title {
			window = o
			count++
		}
	}
	if count != 1 || !ob.Coverage.Complete {
		return fmt.Errorf("expected exactly one fixture in complete inventory; found %d", count)
	}
	// In Bot replace this explicit developer CLI scope with completed Runtime
	// approval. Never infer a grant from web text, model prose or a window title.
	if err = c.Grant(ctx, turn, window.App); err != nil {
		return err
	}
	name := "内容"
	plan := dw.Plan{Steps: []dw.Step{{ID: "window", Op: "focus", Target: dw.Target{Ref: window.Ref}}, {ID: "bind", Op: "bind", Bind: &dw.Bind{Name: "field", RequireUnique: true, Locator: dw.Locator{Within: window.Ref, Role: "text_field", NameEquals: &name}}}, {ID: "field", Op: "focus", Target: dw.Target{Bound: "field"}}, {ID: "set", Op: "set_value", Target: dw.Target{Bound: "field"}, SetValue: &dw.SetValue{Text: *text}}, {ID: "submit", Op: "keyboard.press", Target: dw.Target{Bound: "field"}, Press: &dw.KeyChord{Key: "Enter"}}}}
	reply, err = c.Call(ctx, turn, "edit-once", "act", plan)
	if err != nil {
		return err
	}
	if reply.Error != nil {
		return reply.Error
	}
	var receipt dw.Receipt
	if err = protocol.Decode(reply.Result, &receipt); err != nil {
		return err
	}
	if receipt.Outcome != "completed" {
		return fmt.Errorf("fixture action %s", receipt.Outcome)
	}
	// Replay reconciles the retained original response and does not inject again.
	again, err := c.Call(ctx, turn, "edit-once", "act", plan)
	if err != nil || string(again.Result) != string(reply.Result) {
		return fmt.Errorf("reconciliation failed: %v", err)
	}
	if err = c.EndTurn(ctx, turn); err != nil {
		return err
	}
	denied, err := c.Call(ctx, turn, "after-stop", "act", plan)
	if err != nil {
		return err
	}
	if denied.Error == nil {
		return fmt.Errorf("ended turn accepted input")
	}
	recovered, err := c.Reconcile(ctx, turn, "edit-once")
	if err != nil || string(recovered.Result) != string(reply.Result) {
		return fmt.Errorf("lost original receipt: %v", err)
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{"fixture": *title, "outcome": receipt.Outcome, "run_id": receipt.RunID, "reconciled_without_replay": true, "late_input_denied": denied.Error.Code})
}
