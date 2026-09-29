// Browser fixture: native accessibility tests drive this page while its DOM
// listeners record the received events independently of Desktop World.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"html/template"
	"log"
	"net"
	"net/http"
	"os"
	"sync"
	"time"
)

func main() {
	addr := flag.String("addr", "127.0.0.1:18771", "IPv4 loopback address")
	title := flag.String("title", "Desktop World Browser Fixture", "unique window title")
	path := flag.String("log", "browser-fixture.jsonl", "new independent event log")
	flag.Parse()
	host, _, err := net.SplitHostPort(*addr)
	if err != nil || host != "127.0.0.1" {
		log.Fatal("fixture only binds 127.0.0.1")
	}
	f, err := os.OpenFile(*path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		log.Fatal(err)
	}
	defer f.Close()
	listener, err := net.Listen("tcp4", *addr)
	if err != nil {
		log.Fatal(err)
	}
	origin := "http://" + listener.Addr().String()
	var mu sync.Mutex
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.Host != listener.Addr().String() {
			http.Error(w, "invalid host", 403)
			return
		}
		if r.URL.Path != "/" || r.Method != "GET" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		if err := template.Must(template.New("fixture").Parse(page)).Execute(w, *title); err != nil {
			log.Print(err)
		}
	})
	mux.HandleFunc("/events", func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" || r.Host != listener.Addr().String() || r.Header.Get("Origin") != origin {
			http.Error(w, "forbidden", 403)
			return
		}
		var e struct {
			Event   string `json:"event"`
			Value   string `json:"value"`
			Trusted bool   `json:"trusted"`
		}
		d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096))
		d.DisallowUnknownFields()
		if d.Decode(&e) != nil || (e.Event != "ready" && e.Event != "input" && e.Event != "keydown" && e.Event != "submit" && e.Event != "canvas_click") {
			http.Error(w, "invalid event", 400)
			return
		}
		mu.Lock()
		err := json.NewEncoder(f).Encode(map[string]any{"event": e.Event, "value": e.Value, "trusted": e.Trusted, "at": time.Now().UTC()})
		mu.Unlock()
		if err != nil {
			http.Error(w, "log unavailable", 500)
			return
		}
		w.WriteHeader(204)
	})
	fmt.Println(origin)
	log.Fatal((&http.Server{Handler: mux, ReadHeaderTimeout: 3 * time.Second, ReadTimeout: 5 * time.Second, WriteTimeout: 5 * time.Second}).Serve(listener))
}

const page = `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>{{.}}</title>
<style>body{font:18px system-ui;max-width:740px;margin:48px auto;padding:24px;background:#f4f5f7;color:#202635}h1{font-size:28px}input,button{font:inherit;padding:12px}input{width:420px}button{cursor:pointer}output{display:block;margin:24px 0}canvas{background:white;border:1px solid #999}</style>
<h1>{{.}}</h1><form id="form"><label for="field">DW 网页内容</label><p><input id="field" autocomplete="off"><button>DW 网页提交</button></p></form><output id="status" aria-live="polite">submitted:0</output>
<p>下方文字只绘制为 Canvas 像素，不提供语义按钮。</p><canvas id="canvas" width="450" height="100" aria-label="DW Canvas 负面用例"></canvas>
<script>
const field=document.getElementById('field'), form=document.getElementById('form'), canvas=document.getElementById('canvas');let count=0, queue=Promise.resolve();
function record(event,value,trusted){queue=queue.then(()=>fetch('/events',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({event,value,trusted})}));}
field.addEventListener('input',e=>record('input',field.value,e.isTrusted));field.addEventListener('keydown',e=>record('keydown',e.key,e.isTrusted));
form.addEventListener('submit',e=>{e.preventDefault();document.getElementById('status').textContent='submitted:'+(++count)+' — '+field.value;record('submit',field.value,e.isTrusted)});
const ctx=canvas.getContext('2d');ctx.font='22px system-ui';ctx.fillText('DW Canvas Secret Action',20,55);canvas.addEventListener('click',e=>record('canvas_click','',e.isTrusted));record('ready',navigator.userAgent,false);
</script></html>`
