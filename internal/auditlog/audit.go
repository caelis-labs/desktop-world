// Package auditlog manages private, single-writer metadata logs.
package auditlog

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"
)

type File struct {
	*os.File
	Path string
	lock string
}

func (f *File) Close() error {
	err := f.File.Close()
	if f.lock != "" {
		err = errors.Join(err, os.Remove(f.lock))
	}
	return err
}

// rotate chooses a new name on collision. append requires exclusive ownership
// and a complete last line; it never repairs or truncates an existing log.
func Open(path, mode string) (*File, error) {
	if mode == "" {
		mode = "rotate"
	}
	if mode != "rotate" && mode != "create" && mode != "append" {
		return nil, fmt.Errorf("audit mode must be rotate, create or append")
	}
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return nil, err
	}
	flags := os.O_WRONLY | os.O_CREATE | os.O_EXCL
	var lock string
	if mode == "append" {
		lock = path + ".lock"
		owner, e := os.OpenFile(lock, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if e != nil {
			return nil, fmt.Errorf("audit already owned or lock needs explicit recovery: %w", e)
		}
		_, e = fmt.Fprintf(owner, "pid=%d\n", os.Getpid())
		e = errors.Join(e, owner.Close())
		if e != nil {
			os.Remove(lock)
			return nil, e
		}
		flags = os.O_WRONLY | os.O_CREATE | os.O_APPEND
	}
	fail := func(e error) (*File, error) {
		if lock != "" {
			os.Remove(lock)
		}
		return nil, e
	}
	if info, e := os.Lstat(path); e == nil {
		if !info.Mode().IsRegular() {
			return fail(fmt.Errorf("audit must be a regular file"))
		}
		if mode == "append" && info.Size() > 0 {
			r, e := os.Open(path)
			if e != nil {
				return fail(e)
			}
			var last [1]byte
			_, e = r.ReadAt(last[:], info.Size()-1)
			e = errors.Join(e, r.Close())
			if e != nil && e != io.EOF {
				return fail(e)
			}
			if last[0] != '\n' {
				return fail(fmt.Errorf("audit has an incomplete last record; choose a new file"))
			}
		}
	}
	f, e := os.OpenFile(path, flags, 0600)
	if mode == "rotate" && errors.Is(e, os.ErrExist) {
		path = fmt.Sprintf("%s.%s-%d", path, time.Now().UTC().Format("20060102T150405.000000000Z"), os.Getpid())
		f, e = os.OpenFile(path, flags, 0600)
	}
	if e != nil {
		return fail(e)
	}
	if lock == "" {
		lock = path + ".lock"
		owner, err := os.OpenFile(lock, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			f.Close()
			return nil, fmt.Errorf("audit already owned or lock needs explicit recovery: %w", err)
		}
		_, err = fmt.Fprintf(owner, "pid=%d\n", os.Getpid())
		err = errors.Join(err, owner.Close())
		if err != nil {
			f.Close()
			return fail(err)
		}
	}
	info, e := f.Stat()
	if e != nil || !info.Mode().IsRegular() {
		f.Close()
		return fail(fmt.Errorf("audit must be a regular file"))
	}
	if mode == "append" {
		current, e := os.Lstat(path)
		if e != nil || !os.SameFile(info, current) || !current.Mode().IsRegular() {
			f.Close()
			return fail(fmt.Errorf("audit identity changed while opening"))
		}
	}
	return &File{File: f, Path: path, lock: lock}, nil
}
