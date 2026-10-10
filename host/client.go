// Package host supervises the private Desktop World helper. Only trusted Bot
// host code gets this client; model tool arguments must never select a turn,
// authorize an app, choose an executable, or receive the control pipe.
package host

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"time"

	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/internal/helper"
	"github.com/caelis-labs/desktop-world/protocol"
)

type Options struct {
	InputPolicy dw.InputPolicy
	InputMode   dw.InputMode
	Executable  string
	AssetsDir   string
	AuditPath   string
	AuditMode   string
	Stderr      io.Writer
}

type Reply struct {
	ID       string          `json:"id"`
	Protocol string          `json:"protocol"`
	World    dw.Epoch        `json:"world"`
	Result   json.RawMessage `json:"result,omitempty"`
	Error    *dw.Fault       `json:"error,omitempty"`
}

type Hello struct {
	InputPolicy dw.InputPolicy `json:"input_policy"`
	InputMode   dw.InputMode   `json:"input_mode"`
	Type        string         `json:"type"`
	Protocol    string         `json:"protocol"`
	Environment dw.Environment `json:"environment"`
	AuditPath   string         `json:"audit_path"`
	Managed     bool           `json:"managed"`
	CoreNoAuth  bool           `json:"core_no_auth"`
}

type exchange struct {
	body     []byte
	done     chan struct{}
	reply    Reply
	err      error
	complete bool
}

type peer struct {
	mu      sync.Mutex
	writeMu sync.Mutex
	input   io.ReadCloser
	output  io.WriteCloser
	records map[string]*exchange
	broken  error
}

func newPeer(in io.ReadCloser, out io.WriteCloser) *peer {
	return &peer{input: in, output: out, records: map[string]*exchange{}}
}
func (p *peer) finish(id string, reply Reply, err error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if x := p.records[id]; x != nil && !x.complete {
		x.reply, x.err, x.complete = reply, err, true
		close(x.done)
	}
}
func (p *peer) fail(err error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.broken == nil {
		p.broken = err
	}
	for _, x := range p.records {
		if !x.complete {
			x.err, x.complete = p.broken, true
			close(x.done)
		}
	}
}
func (p *peer) read(scan *bufio.Scanner) {
	for scan.Scan() {
		var reply Reply
		if err := protocol.Decode(scan.Bytes(), &reply); err != nil || reply.ID == "" {
			p.fail(errors.New("invalid helper frame; effects may be unknown"))
			return
		}
		p.finish(reply.ID, reply, nil)
	}
	err := scan.Err()
	if err == nil {
		err = io.EOF
	}
	p.fail(fmt.Errorf("helper disconnected; effects may be unknown: %w", err))
}
func (p *peer) submit(id string, request any) (*exchange, error) {
	body, err := protocol.Marshal(request)
	if err != nil {
		return nil, err
	}
	p.mu.Lock()
	if x := p.records[id]; x != nil {
		p.mu.Unlock()
		if string(x.body) != string(body) {
			return nil, errors.New("request conflict: reuse ID only with identical arguments")
		}
		return x, nil
	}
	if p.broken != nil {
		err = p.broken
		p.mu.Unlock()
		return nil, err
	}
	if len(p.records) >= 4096 {
		p.mu.Unlock()
		return nil, errors.New("host session record limit reached")
	}
	x := &exchange{body: body, done: make(chan struct{})}
	p.records[id] = x
	p.mu.Unlock()
	// A separate writer keeps cancellation available even if the child stops
	// reading its data pipe. Closing the pipe releases this goroutine.
	go func() {
		p.writeMu.Lock()
		defer p.writeMu.Unlock()
		_, err := p.output.Write(append(body, '\n'))
		if err != nil {
			p.fail(err)
		}
	}()
	return x, nil
}
func wait(ctx context.Context, x *exchange) (Reply, error) {
	select {
	case <-x.done:
		return x.reply, x.err
	case <-ctx.Done():
		return Reply{}, ctx.Err()
	}
}

// Client keeps original replies for reconciliation until Close. It never
// restarts a helper or automatically replays a mutation.
type Client struct {
	Hello         Hello
	cmd           *exec.Cmd
	data, control *peer
	exited        chan struct{}
	closeOnce     sync.Once
	closeErr      error
	waitErr       error
	mu            sync.Mutex
	sequence      uint64
	requests      map[string]string
}

func Start(ctx context.Context, o Options) (*Client, error) {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	if err := o.InputMode.Validate(); err != nil {
		return nil, err
	}
	if err := o.InputPolicy.Validate(); err != nil {
		return nil, err
	}
	if !filepath.IsAbs(o.Executable) {
		return nil, errors.New("helper executable must be an absolute trusted path")
	}
	cr, cw, err := os.Pipe()
	if err != nil {
		return nil, err
	}
	rr, rw, err := os.Pipe()
	if err != nil {
		cr.Close()
		cw.Close()
		return nil, err
	}
	args := []string{"serve", "--host-control", "--full-output"}
	if o.InputMode != "" {
		args = append(args, "--input-mode", string(o.InputMode))
	}
	if o.InputPolicy != "" {
		args = append(args, "--input-policy", string(o.InputPolicy))
	}
	if o.AssetsDir != "" {
		args = append(args, "--assets-dir", o.AssetsDir)
	}
	if o.AuditPath != "" {
		args = append(args, "--audit", o.AuditPath)
		if o.AuditMode != "" {
			args = append(args, "--audit-mode", o.AuditMode)
		}
	}
	cmd := exec.Command(o.Executable, args...)
	cmd.Stderr = o.Stderr
	for _, k := range []string{"HOME", "PATH", "TMPDIR", "LANG", "LC_CTYPE", "SystemRoot", "WINDIR", "USERPROFILE", "TEMP", "TMP"} {
		if v, ok := os.LookupEnv(k); ok {
			cmd.Env = append(cmd.Env, k+"="+v)
		}
	}
	cleanupControl, err := prepareControl(cmd, cr, rw)
	if err != nil {
		cr.Close()
		cw.Close()
		rr.Close()
		rw.Close()
		return nil, err
	}
	defer cleanupControl()
	stdin, err := cmd.StdinPipe()
	if err != nil {
		cr.Close()
		cw.Close()
		rr.Close()
		rw.Close()
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		stdin.Close()
		cr.Close()
		cw.Close()
		rr.Close()
		rw.Close()
		return nil, err
	}
	if err = cmd.Start(); err != nil {
		stdin.Close()
		stdout.Close()
		cr.Close()
		cw.Close()
		rr.Close()
		rw.Close()
		return nil, err
	}
	cr.Close()
	rw.Close()
	c := &Client{cmd: cmd, data: newPeer(stdout, stdin), control: newPeer(rr, cw), exited: make(chan struct{}), requests: map[string]string{}}
	go func() { err := cmd.Wait(); c.mu.Lock(); c.waitErr = err; c.mu.Unlock(); close(c.exited) }()
	controlScan := bufio.NewScanner(rr)
	controlScan.Buffer(make([]byte, 4096), 2<<20)
	go c.control.read(controlScan)
	ready := make(chan error, 1)
	go func() {
		scan := bufio.NewScanner(stdout)
		scan.Buffer(make([]byte, 4096), 2<<20)
		if !scan.Scan() {
			ready <- errors.New("missing helper hello")
			return
		}
		// Hello has extension fields; its stable consumed subset is deliberately decoded here.
		var raw struct {
			Type, Protocol string
			InputPolicy    dw.InputPolicy `json:"input_policy"`
			InputMode      dw.InputMode   `json:"input_mode"`
			Environment    json.RawMessage
			Managed        bool
			CoreNoAuth     bool   `json:"core_no_auth"`
			AuditPath      string `json:"audit_path"`
		}
		if err := json.Unmarshal(scan.Bytes(), &raw); err != nil {
			ready <- err
			return
		}
		c.Hello.Type, c.Hello.Protocol, c.Hello.Managed, c.Hello.CoreNoAuth = raw.Type, raw.Protocol, raw.Managed, raw.CoreNoAuth
		c.Hello.AuditPath = raw.AuditPath
		c.Hello.InputPolicy = raw.InputPolicy
		c.Hello.InputMode = raw.InputMode.Effective()
		if err := protocol.Decode(raw.Environment, &c.Hello.Environment); err != nil {
			ready <- err
			return
		}
		if raw.Type != "hello" || raw.Protocol != helper.Version || !raw.Managed {
			ready <- errors.New("incompatible or unmanaged helper")
			return
		}
		policy := o.InputPolicy
		if policy == "" {
			policy = dw.InputShared
		}
		if raw.InputPolicy != policy {
			ready <- errors.New("helper input policy does not match trusted host policy")
			return
		}
		if raw.InputMode.Effective() != o.InputMode.Effective() || c.Hello.Environment.InputMode.Effective() != o.InputMode.Effective() {
			ready <- errors.New("helper input mode does not match trusted host mode")
			return
		}
		ready <- nil
		c.data.read(scan)
	}()
	select {
	case err = <-ready:
	case <-ctx.Done():
		err = ctx.Err()
	}
	if err != nil {
		c.Close()
		return nil, err
	}
	return c, nil
}

func (c *Client) controlRequest(ctx context.Context, request helper.ControlRequest) (Reply, error) {
	if err := ctx.Err(); err != nil {
		return Reply{}, err
	}
	c.mu.Lock()
	c.sequence++
	id := fmt.Sprintf("host-%d", c.sequence)
	c.mu.Unlock()
	request.ID = id
	x, err := c.control.submit(id, request)
	if err != nil {
		return Reply{}, err
	}
	reply, err := wait(ctx, x)
	if err != nil {
		c.Close() // An uncertain control mutation must never leave live grants behind.
		return Reply{}, err
	}
	if reply.Error != nil {
		return reply, reply.Error
	}
	return reply, nil
}
func (c *Client) controlCall(ctx context.Context, op, turn string, app dw.Ref) error {
	_, err := c.controlRequest(ctx, helper.ControlRequest{Op: op, Turn: turn, Application: app})
	return err
}

// Declare approves a selector for one turn; binding waits for a complete unique observation.
func (c *Client) Declare(ctx context.Context, turn, name, windowTitle string) error {
	_, err := c.controlRequest(ctx, helper.ControlRequest{Op: "declare", Turn: turn, Name: name, WindowTitle: windowTitle})
	return err
}
func (c *Client) Revoke(ctx context.Context, turn string, app dw.Ref) error {
	_, err := c.controlRequest(ctx, helper.ControlRequest{Op: "revoke", Turn: turn, Application: app})
	return err
}
func (c *Client) RevokeGrant(ctx context.Context, turn, grantID string) error {
	_, err := c.controlRequest(ctx, helper.ControlRequest{Op: "revoke", Turn: turn, GrantID: grantID})
	return err
}

type ApplicationGrant = helper.ApplicationGrant
type GrantStatus = helper.GrantStatus

func (c *Client) Grants(ctx context.Context, turn string) (GrantStatus, error) {
	reply, err := c.controlRequest(ctx, helper.ControlRequest{Op: "grants", Turn: turn})
	if err != nil {
		return GrantStatus{}, err
	}
	var out GrantStatus
	err = protocol.Decode(reply.Result, &out)
	return out, err
}

func (c *Client) BeginTurn(ctx context.Context, turn string) error {
	return c.controlCall(ctx, "begin_turn", turn, "")
}

// Grant must follow Runtime approval of this exact observed application instance.
func (c *Client) Grant(ctx context.Context, turn string, app dw.Ref) error {
	return c.controlCall(ctx, "grant", turn, app)
}

// EndTurn revokes authority and cancels work; it does not prove no effect occurred.
func (c *Client) EndTurn(ctx context.Context, turn string) error {
	err := c.controlCall(ctx, "end_turn", turn, "")
	if err != nil {
		c.Close() // Also fail closed if the caller passes an expired context.
	}
	return err
}

func (c *Client) Call(ctx context.Context, turn, id, op string, args any) (Reply, error) {
	if err := ctx.Err(); err != nil {
		return Reply{}, err
	}
	if id == "" || len(id) > 128 || turn == "" {
		return Reply{}, errors.New("host supplies turn and stable request ID")
	}
	key := turn + "\x00" + id
	c.mu.Lock()
	wireID := c.requests[key]
	if wireID == "" {
		if len(c.requests) >= 4096 {
			c.mu.Unlock()
			return Reply{}, errors.New("host session record limit reached")
		}
		c.sequence++
		wireID = fmt.Sprintf("data-%d", c.sequence)
		c.requests[key] = wireID
	}
	c.mu.Unlock()
	body, err := protocol.Marshal(args)
	if err != nil {
		return Reply{}, err
	}
	x, err := c.data.submit(wireID, helper.Request{ID: wireID, Op: op, Turn: turn, Args: body})
	if err != nil {
		return Reply{}, err
	}
	reply, err := wait(ctx, x)
	if err != nil {
		stopCtx, cancel := context.WithTimeout(context.Background(), time.Second)
		stopErr := c.EndTurn(stopCtx, turn)
		cancel()
		if stopErr != nil {
			c.Close()
		}
		return Reply{}, fmt.Errorf("call cancelled; turn stopped, effects may be unknown; reconcile original request: %w", err)
	}
	return reply, err
}

// Reconcile waits for the original reply without sending input, including after
// EndTurn. Helper errors retain their result/receipt in Reply and are not Go errors.
func (c *Client) Reconcile(ctx context.Context, turn, id string) (Reply, error) {
	c.mu.Lock()
	wireID := c.requests[turn+"\x00"+id]
	c.mu.Unlock()
	c.data.mu.Lock()
	x := c.data.records[wireID]
	c.data.mu.Unlock()
	if x == nil {
		return Reply{}, errors.New("unknown original request")
	}
	return wait(ctx, x)
}
func (c *Client) Close() {
	c.closeOnce.Do(func() {
		// Closing the owner pipe revokes grants, then EOF requests bounded native cleanup.
		c.control.output.Close()
		c.data.output.Close()
		select {
		case <-c.exited:
		case <-time.After(2 * time.Second):
			c.mu.Lock()
			c.closeErr = errors.New("helper_close_incomplete: forced termination does not prove native cleanup")
			c.mu.Unlock()
			_ = c.cmd.Process.Kill()
			<-c.exited
		}
		c.control.input.Close()
		c.data.input.Close()
		c.data.fail(errors.New("host closed; no automatic restart or replay"))
		c.control.fail(errors.New("host closed"))
	})
}

// CloseWithError reports a forced or failed child exit; Close retains its legacy signature.
func (c *Client) CloseWithError() error {
	c.Close()
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closeErr != nil {
		return c.closeErr
	}
	return c.waitErr
}
