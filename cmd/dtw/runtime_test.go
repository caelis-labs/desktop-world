package main

import (
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	quickjs "github.com/buke/quickjs-go"
)

// These are isolated binding feasibility tests. They do not claim native DTW
// feature or MCP acceptance.
func TestQuickJSAsyncPersistentStateAndInterrupt(t *testing.T) {
	type command struct {
		code   string
		stop   <-chan struct{}
		result chan string
	}
	commands := make(chan command)
	ready := make(chan struct{})
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		rt := quickjs.NewRuntime()
		defer rt.Close()
		ctx := rt.NewBareContext()
		defer ctx.Close()
		ctx.Globals().Set("state", ctx.NewObject())
		ctx.Globals().Set("nativeAsync", ctx.NewFunction(func(ctx *quickjs.Context, _ *quickjs.Value, _ []*quickjs.Value) *quickjs.Value {
			return ctx.NewPromise(func(resolve, _ func(*quickjs.Value)) {
				go func() {
					time.Sleep(10 * time.Millisecond)
					ctx.Schedule(func(inner *quickjs.Context) {
						value := inner.NewString("native-done")
						resolve(value)
						value.Free()
					})
				}()
			})
		}))
		var interrupted atomic.Bool
		rt.SetInterruptHandler(func() int {
			if interrupted.Load() {
				return 1
			}
			return 0
		})
		close(ready)
		for command := range commands {
			interrupted.Store(false)
			watchDone := make(chan struct{})
			go func() {
				select {
				case <-command.stop:
					interrupted.Store(true)
				case <-watchDone:
				}
			}()
			value := ctx.Eval(command.code)
			if value != nil && !value.IsException() {
				settled := ctx.Await(value)
				if settled != value {
					value.Free()
					value = settled
				}
			}
			close(watchDone)
			if value == nil {
				command.result <- "nil"
				continue
			}
			if value.IsException() {
				command.result <- "exception: " + ctx.Exception().Error()
			} else {
				command.result <- value.ToString()
			}
			value.Free()
		}
	}()
	select {
	case <-ready:
	case <-time.After(20 * time.Second):
		t.Fatal("QuickJS init hung")
	}
	defer close(commands)
	call := func(code string, stop <-chan struct{}) string {
		result := make(chan string, 1)
		commands <- command{code, stop, result}
		select {
		case out := <-result:
			return out
		case <-time.After(3 * time.Second):
			t.Fatal("QuickJS execution did not stop")
			return ""
		}
	}
	never := make(chan struct{})
	if got := call(`(async () => { state.items = [1,2,3,4].filter(x => x % 2 === 0); try { throw Error('caught') } catch (e) { state.error = e.message }; return await nativeAsync() })()`, never); got != "native-done" {
		t.Fatalf("async: %s", got)
	}
	if got := call(`state.items.join(',') + ':' + state.error`, never); got != "2,4:caught" {
		t.Fatalf("state: %s", got)
	}
	for _, code := range []string{`while (true) {}`, `(async()=>{ await nativeAsync(); while (true) {} })()`} {
		stop := make(chan struct{})
		go func() { time.Sleep(30 * time.Millisecond); close(stop) }()
		if got := call(code, stop); !strings.HasPrefix(got, "exception:") {
			t.Fatalf("interrupt %q: %s", code, got)
		}
	}
}
