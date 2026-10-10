package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

type actionShape struct {
	Steps []struct {
		Op     string `json:"op"`
		Target struct {
			Ref    string `json:"ref"`
			Anchor struct {
				Target string `json:"target"`
			} `json:"anchor"`
		} `json:"target"`
	} `json:"steps"`
}

// Every connection has its own helper and JS process. File locks provide a
// narrow cross-process seat/app boundary without adding a daemon. The global
// shared lock lets known, independent apps proceed concurrently; an unknown
// target takes it exclusively rather than guessing that another app is safe.
func (s *supervisor) coordinateAction(ctx context.Context, id string, body json.RawMessage) (func(), string, error) {
	var act actionShape
	if err := json.Unmarshal(body, &act); err != nil || len(act.Steps) == 0 {
		return nil, "", errors.New("action shape unknown before dispatch")
	}
	foreground := false
	app := ""
	s.mu.Lock()
	for _, step := range act.Steps {
		if strings.HasPrefix(step.Op, "pointer.") || strings.HasPrefix(step.Op, "keyboard.") || step.Op == "focus" {
			foreground = true
		}
		ref := step.Target.Ref
		if ref == "" {
			ref = step.Target.Anchor.Target
		}
		known := s.resolveApp(ref)
		if known == "" || app != "" && app != known {
			app = ""
			break
		}
		app = known
	}
	s.mu.Unlock()
	root := os.Getenv("DTW_POC_COORD_DIR")
	if root == "" {
		root = filepath.Join(os.TempDir(), fmt.Sprintf("dtw-native-go-poc-%d", os.Getuid()))
	}
	if err := os.Mkdir(root, 0700); err != nil && !os.IsExist(err) {
		return nil, "", err
	}
	info, err := os.Lstat(root)
	if err != nil {
		return nil, "", err
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || !info.IsDir() || info.Mode().Perm() != 0700 || stat.Uid != uint32(os.Getuid()) {
		return nil, "", errors.New("coordination directory must be a private owned directory")
	}
	var held []*os.File
	coordTrace("waiting", id, app, foreground)
	release := func() {
		coordTrace("released", id, app, foreground)
		for i := len(held) - 1; i >= 0; i-- {
			_ = unix.Flock(int(held[i].Fd()), unix.LOCK_UN)
			_ = held[i].Close()
		}
	}
	acquire := func(name string, exclusive bool) error {
		path := filepath.Join(root, name)
		fd, err := unix.Open(path, unix.O_CREAT|unix.O_RDWR|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0600)
		if err != nil {
			return err
		}
		file := os.NewFile(uintptr(fd), path)
		mode := unix.LOCK_SH
		if exclusive {
			mode = unix.LOCK_EX
		}
		for {
			if err = unix.Flock(fd, mode|unix.LOCK_NB); err == nil {
				held = append(held, file)
				return nil
			}
			if err != unix.EWOULDBLOCK && err != unix.EAGAIN {
				_ = file.Close()
				return err
			}
			select {
			case <-ctx.Done():
				_ = file.Close()
				return ctx.Err()
			case <-time.After(10 * time.Millisecond):
			}
		}
	}
	if err := acquire("all.lock", app == ""); err != nil {
		release()
		return nil, "", err
	}
	if app != "" {
		digest := sha256.Sum256([]byte(app))
		if err := acquire("app-"+hex.EncodeToString(digest[:16])+".lock", true); err != nil {
			release()
			return nil, "", err
		}
	}
	if foreground {
		if err := acquire("foreground.lock", true); err != nil {
			release()
			return nil, "", err
		}
		if err := waitForUserInputQuiet(ctx, 600*time.Millisecond, 3*time.Second); err != nil {
			release()
			return nil, "", err
		}
		coordTrace("acquired", id, app, foreground)
		return release, "foreground_transaction", nil
	}
	coordTrace("acquired", id, app, foreground)
	return release, "semantic_background", nil
}

func coordTrace(event, id, app string, foreground bool) {
	path := os.Getenv("DTW_POC_COORD_TRACE")
	if path == "" {
		return
	}
	file, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer file.Close()
	row, _ := json.Marshal(map[string]any{"event": event, "native_id": id, "app": app, "foreground": foreground, "pid": os.Getpid(), "time": time.Now().UnixNano()})
	_, _ = file.Write(append(row, '\n'))
}

func (s *supervisor) resolveApp(ref string) string {
	for i := 0; i < 8 && ref != ""; i++ {
		if app := s.refApps[ref]; app != "" {
			return app
		}
		ref = s.refParents[ref]
	}
	return ""
}

func (s *supervisor) rememberObservation(result, request json.RawMessage) {
	var parsed struct {
		Objects []struct {
			Ref  string `json:"ref"`
			App  string `json:"app"`
			Kind string `json:"kind"`
			Name struct {
				Status string `json:"status"`
				Value  string `json:"value"`
			} `json:"name"`
		} `json:"objects"`
	}
	if json.Unmarshal(result, &parsed) != nil {
		return
	}
	var scope struct {
		Scope struct {
			Refs []string `json:"refs"`
		} `json:"scope"`
	}
	_ = json.Unmarshal(request, &scope)
	for _, object := range parsed.Objects {
		if (object.Kind == "app" || object.Kind == "application") && object.Name.Status == "known" && object.Name.Value != "" {
			s.refApps[object.Ref] = object.Name.Value
		}
	}
	for _, object := range parsed.Objects {
		if object.App != "" && object.App != object.Ref {
			s.refParents[object.Ref] = object.App
		}
		if s.refParents[object.Ref] == "" && len(scope.Scope.Refs) == 1 && object.Ref != scope.Scope.Refs[0] {
			s.refParents[object.Ref] = scope.Scope.Refs[0]
		}
	}
}
