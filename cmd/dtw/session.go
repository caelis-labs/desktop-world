package main

import (
	"bufio"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/host"
	"github.com/caelis-labs/desktop-world/internal/owneripc"
	"github.com/caelis-labs/desktop-world/internal/session"
	"github.com/caelis-labs/desktop-world/protocol"
)

type ownerDescriptor struct {
	Address string `json:"address"`
	Epoch   string `json:"epoch"`
	PID     int    `json:"pid"`
}

func randomID() string { var id [16]byte; _, _ = rand.Read(id[:]); return hex.EncodeToString(id[:]) }
func runSession(args []string) error {
	f := flag.NewFlagSet("session", flag.ContinueOnError)
	f.SetOutput(os.Stderr)
	var executable, path, mode, policy, assets, audit, auditMode string
	var apps, windows names
	f.StringVar(&executable, "helper", "", "trusted helper path; defaults to this executable")
	f.StringVar(&path, "session", "", "private owner descriptor for dtw auth; omit for stdio SDK")
	f.StringVar(&mode, "input-mode", "", "shared or cooperative")
	f.StringVar(&policy, "input-policy", "", "shared_input or no_shared_input")
	f.StringVar(&assets, "assets-dir", "", "capture directory")
	f.StringVar(&audit, "audit", "", "metadata audit destination")
	f.StringVar(&auditMode, "audit-mode", "rotate", "rotate, append or create")
	f.Var(&apps, "write-app", "preapprove exact name, including a not-yet-running app")
	f.Var(&windows, "write-app-window", "preapprove owning app of exact window title")
	if err := f.Parse(args); err != nil {
		return err
	}
	if f.NArg() != 0 {
		return fmt.Errorf("unexpected session arguments")
	}
	var err error
	if executable == "" {
		executable, err = os.Executable()
		if err != nil {
			return err
		}
	}
	executable, err = filepath.Abs(executable)
	if err != nil {
		return err
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	c, err := host.Start(ctx, host.Options{Executable: executable, InputMode: dw.InputMode(mode), InputPolicy: dw.InputPolicy(policy), AssetsDir: assets, AuditPath: audit, AuditMode: auditMode, Stderr: os.Stderr})
	if err != nil {
		return err
	}
	defer c.Close()
	s, err := session.New(ctx, c)
	if err != nil {
		return err
	}
	for _, name := range apps {
		if err = c.Declare(ctx, "session", name, ""); err != nil {
			return err
		}
	}
	for _, title := range windows {
		if err = c.Declare(ctx, "session", "", title); err != nil {
			return err
		}
	}
	if path != "" {
		path, err = filepath.Abs(path)
		if err != nil {
			return err
		}
		if err = os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			return err
		}
		file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			return fmt.Errorf("owner descriptor already exists or cannot be created; inspect its original session before explicit recovery: %w", err)
		}
		defer os.Remove(path)
		listener, address, cleanup, err := owneripc.Listen()
		if err != nil {
			file.Close()
			return err
		}
		defer cleanup()
		err = json.NewEncoder(file).Encode(ownerDescriptor{Address: address, Epoch: string(c.Hello.Environment.Epoch), PID: os.Getpid()})
		err2 := file.Close()
		if err != nil {
			return err
		}
		if err2 != nil {
			return err2
		}
		permits := make(chan struct{}, 4)
		go func() {
			for {
				connection, err := listener.Accept()
				if err != nil {
					return
				}
				select {
				case permits <- struct{}{}:
				default:
					connection.Close()
					continue
				}
				go func() {
					defer func() { <-permits }()
					defer connection.Close()
					connection.SetDeadline(time.Now().Add(15 * time.Second))
					var r session.Request
					scan := bufio.NewScanner(connection)
					scan.Buffer(make([]byte, 4096), 1<<20)
					if !scan.Scan() {
						return
					}
					if protocol.Decode(scan.Bytes(), &r) != nil || r.Channel != "host" {
						return
					}
					callCtx, cancel := context.WithTimeout(ctx, 12*time.Second)
					defer cancel()
					reply := s.Handle(callCtx, r)
					data, _ := protocol.Marshal(reply)
					connection.Write(data)
				}()
			}
		}()
	}
	hello := map[string]any{"type": "hello", "protocol": session.Version, "environment": c.Hello.Environment, "audit_path": c.Hello.AuditPath, "input_mode": c.Hello.InputMode, "input_policy": c.Hello.InputPolicy, "features": []string{"dynamic_app_grants", "deferred_apps", "bind_focus", "plan_builder", "receipt_reconciliation"}}
	b, err := protocol.Marshal(hello)
	if err != nil {
		return err
	}
	if _, err = fmt.Fprintln(os.Stdout, string(b)); err != nil {
		return err
	}
	go func() { <-ctx.Done(); os.Stdin.Close() }()
	return s.Serve(ctx, os.Stdin, os.Stdout)
}

func runAuth(args []string) error {
	if len(args) == 0 {
		return fmt.Errorf("use auth list|add|revoke --session OWNER_FILE")
	}
	op := args[0]
	f := flag.NewFlagSet("auth", flag.ContinueOnError)
	f.SetOutput(os.Stderr)
	var path, name, window, app, grant, id string
	f.StringVar(&path, "session", "", "trusted owner descriptor")
	f.StringVar(&name, "app", "", "exact application name")
	f.StringVar(&window, "app-window", "", "exact window title")
	f.StringVar(&app, "app-ref", "", "observed application Ref")
	f.StringVar(&grant, "grant", "", "grant ID")
	f.StringVar(&id, "id", "", "stable owner request ID for exact request reconciliation")
	if err := f.Parse(args[1:]); err != nil {
		return err
	}
	if path == "" || f.NArg() != 0 {
		return fmt.Errorf("auth requires --session OWNER_FILE")
	}
	var a session.OwnerArgs
	switch op {
	case "list":
		op = "grants"
		if name+window+app+grant != "" {
			return fmt.Errorf("list takes no selector")
		}
	case "add":
		n := 0
		for _, v := range []string{name, window, app} {
			if v != "" {
				n++
			}
		}
		if n != 1 || grant != "" {
			return fmt.Errorf("add requires exactly one --app, --app-window or --app-ref")
		}
		if app != "" {
			op = "grant"
			a.Application = dw.Ref(app)
		} else {
			op = "declare"
			a.Name, a.WindowTitle = name, window
		}
	case "revoke":
		if (app == "") == (grant == "") || name+window != "" {
			return fmt.Errorf("revoke requires --app-ref or --grant")
		}
		a.Application, a.GrantID = dw.Ref(app), grant
	default:
		return fmt.Errorf("auth supports list, add and revoke")
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var descriptor ownerDescriptor
	if err = json.Unmarshal(raw, &descriptor); err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	connection, err := owneripc.Dial(ctx, descriptor.Address)
	if err != nil {
		return err
	}
	defer connection.Close()
	connection.SetDeadline(time.Now().Add(15 * time.Second))
	if id == "" {
		id = "auth-" + randomID()
	}
	body, _ := protocol.Marshal(a)
	request, _ := protocol.Marshal(session.Request{ID: id, Channel: "host", Op: op, Args: body})
	// Half-close framing is not available on Windows named pipes. A newline-delimited frame is used on both platforms.
	if _, err = connection.Write(append(request, '\n')); err != nil {
		return err
	}
	response, err := io.ReadAll(io.LimitReader(connection, 1<<20))
	if err != nil {
		return err
	}
	var reply host.Reply
	if err = protocol.Decode(response, &reply); err != nil {
		return err
	}
	if string(reply.World) != descriptor.Epoch {
		return fmt.Errorf("owner epoch changed; do not reuse this descriptor")
	}
	fmt.Println(string(response))
	if reply.Error != nil {
		return reply.Error
	}
	return nil
}
