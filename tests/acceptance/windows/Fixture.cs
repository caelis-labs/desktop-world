// Compiled locally by Windows PowerShell 5.1. No external packages.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DtwWindowsSmoke {
    // Opt-in read-only UIA barrier. Independent supervision releases this after
    // the native caller has been canceled/timed out; the 45 s ceiling prevents
    // a failed test from leaving the owned app permanently blocked.
    public sealed class BlockingReadLabel : Label {
        readonly string root;
        public BlockingReadLabel(string directory) { root=directory; Text="Blocking read probe"; }
        protected override AccessibleObject CreateAccessibilityInstance() { return new Barrier(this); }
        sealed class Barrier : Control.ControlAccessibleObject {
            readonly BlockingReadLabel label;
            public Barrier(BlockingReadLabel owner) : base(owner) { label=owner; }
            public override string Name {
                get {
                    if (File.Exists(Path.Combine(label.root,"block-uia"))) {
                        File.WriteAllText(Path.Combine(label.root,"uia-entered"),"read-only Name getter");
                        Stopwatch limit=Stopwatch.StartNew();
                        while (!File.Exists(Path.Combine(label.root,"release-uia")) && limit.ElapsedMilliseconds<45000)
                            System.Threading.Thread.Sleep(5);
                    }
                    return "Blocking read probe";
                }
                set { }
            }
        }
    }
    public sealed class Pad : Control {
        public Pad() {
            SetStyle(ControlStyles.Selectable | ControlStyles.StandardClick |
                     ControlStyles.StandardDoubleClick | ControlStyles.UserPaint, true);
            TabStop = true;
            AccessibleName = "Owned pointer pad";
            BackColor = Color.LightBlue;
        }
        protected override void OnPaint(PaintEventArgs e) {
            base.OnPaint(e);
            e.Graphics.DrawString("Owned pointer / wheel pad", Font, Brushes.Black, 12, 12);
        }
        protected override void OnMouseDown(MouseEventArgs e) { Focus(); base.OnMouseDown(e); }
        public event MouseEventHandler HorizontalWheel;
        protected override void WndProc(ref Message message) {
            if (message.Msg == 0x020E && HorizontalWheel != null)
                HorizontalWheel(this, new MouseEventArgs(MouseButtons.None, 0, 0, 0, (short)((long)message.WParam >> 16)));
            base.WndProc(ref message);
        }
    }

    public sealed class Fixture : Form, IMessageFilter {
        [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left, Top, Right, Bottom; }
        [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
        [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
        [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
        [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
        [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
        readonly string root;
        readonly string label;
        readonly JavaScriptSerializer json = new JavaScriptSerializer();
        readonly UTF8Encoding utf8 = new UTF8Encoding(false);
        readonly Stopwatch clock = Stopwatch.StartNew();
        readonly TextBox editor = new TextBox();
        readonly Button increment = new Button();
        readonly Point origin;
        readonly Pad pad = new Pad();
        readonly Timer timer = new Timer();
        readonly bool dpiSet;
        int sequence, increments, saves, leftUps, rightUps, doubles, wheelDelta, horizontalWheelDelta, moves, dragMoves, layoutSequence;
        string saved = "";
        bool modalOpen, menuOpen;
        bool ready, closed;

        protected override bool ShowWithoutActivation {
            get { return !String.IsNullOrEmpty(root) && File.Exists(Path.Combine(root,"show-without-activation")); }
        }

        Fixture(string directory, string instance, bool awarenessSet) {
            root = directory; label = instance; dpiSet = awarenessSet;
            Text = "DTW Windows Smoke " + label;
            AccessibleName = Text;
            AutoScaleMode = AutoScaleMode.None;
            StartPosition = FormStartPosition.Manual;
            FormBorderStyle = FormBorderStyle.FixedSingle;
            MaximizeBox = false;
            ClientSize = new Size(410, 480);
            if (File.Exists(Path.Combine(root, "layered-window"))) Opacity = .75;
            Rectangle area = Screen.PrimaryScreen.WorkingArea;
            Location = new Point(area.Left + 16 + (label == "A" ? 0 : 446), area.Top + 16);
            origin = Location;
            editor.Location = new Point(12, 12); editor.Size = new Size(386, 125);
            editor.Multiline = true; editor.AcceptsReturn = true;
            if (File.Exists(Path.Combine(root,"password-editor"))) editor.PasswordChar='*';
            editor.ScrollBars = ScrollBars.Vertical; editor.AccessibleName = "Message";
            string seed=Path.Combine(root,"seed-editor.txt");
            if(File.Exists(seed))editor.Text=File.ReadAllText(seed,utf8);
            editor.TextChanged += delegate {
                Record("text_changed", new { length = editor.Text.Length });
                if (editor.Text.Length>0 && char.IsHighSurrogate(editor.Text[editor.Text.Length-1]) &&
                    File.Exists(Path.Combine(root,"track-surrogate"))) {
                    Record("surrogate_half",new {length=editor.Text.Length});
                    if (File.Exists(Path.Combine(root,"delay-surrogate"))) System.Threading.Thread.Sleep(40);
                    if (File.Exists(Path.Combine(root,"block-surrogate"))) {
                        File.WriteAllText(Path.Combine(root,"surrogate-blocked"),"entered",utf8);
                        var barrier=Stopwatch.StartNew();
                        while (!File.Exists(Path.Combine(root,"release-surrogate")) && barrier.ElapsedMilliseconds<5000)
                            System.Threading.Thread.Sleep(5);
                    }
                }
            };
            Controls.Add(editor);
            increment.Text = "Increment"; increment.AccessibleName = "Increment";
            increment.Location = new Point(12, 145); increment.Size = new Size(120, 30);
            increment.Click += delegate { increments++; Record("increment", new { count = increments }); editor.Focus(); Publish(); };
            Button save = new Button { Text = "Save", AccessibleName = "Save", Location = new Point(144, 145), Size = new Size(100, 30) };
            save.Click += delegate { Save("button"); editor.Focus(); Publish(); };
            Controls.Add(increment); Controls.Add(save);
            Button openDialog = new Button { Text="Show owned dialog", AccessibleName="Show owned dialog", Location=new Point(250,145), Size=new Size(145,30) };
            openDialog.Click += delegate { BeginInvoke((Action)delegate {
                using (var dialog = new Form { Text="Owned native modal", AccessibleName="Owned native modal", ClientSize=new Size(280,100),
                    StartPosition=FormStartPosition.CenterParent, ShowInTaskbar=false }) {
                    var close=new Button { Text="Close dialog", AccessibleName="Close dialog", Location=new Point(20,20), Size=new Size(160,35), DialogResult=DialogResult.Cancel };
                    dialog.Controls.Add(close);dialog.CancelButton=close;
                    modalOpen=true; Publish();
                    try { dialog.ShowDialog(this); } finally { modalOpen=false; Publish(); }
                }
            }); };
            Controls.Add(openDialog);
            // Exercise the native HMENU/UIA provider, independent of managed
            // ToolStrip accessibility providers which may publish read gaps.
            var menu = new MainMenu();
            var file = new MenuItem("Audit File");
            file.Popup += delegate { menuOpen=true; Publish(); };
            file.MenuItems.Add(new MenuItem("Commit audit document", delegate { Save("menu"); }));
            menu.MenuItems.Add(file); Menu=menu;
            MenuComplete += delegate { menuOpen=false; Publish(); };
            pad.Location = new Point(12, 185); pad.Size = new Size(386, 210);
            if (label == "B") pad.BackColor=Color.LightPink;
            pad.MouseMove += delegate(object sender, MouseEventArgs e) {
                moves++; if ((e.Button & MouseButtons.Left) != 0) dragMoves++;
                Publish();
            };
            pad.HorizontalWheel += delegate(object sender, MouseEventArgs e) {
                horizontalWheelDelta += e.Delta; Record("pad_horizontal_wheel", new { delta=e.Delta }); Publish();
            };
            pad.MouseUp += delegate(object sender, MouseEventArgs e) {
                if (e.Button == MouseButtons.Left) leftUps++;
                if (e.Button == MouseButtons.Right) rightUps++;
                Record("pad_up", new { button = e.Button.ToString(), x = e.X, y = e.Y }); Publish();
            };
            pad.MouseDoubleClick += delegate { doubles++; Record("pad_double", new { count = doubles }); Publish(); };
            pad.MouseWheel += delegate(object sender, MouseEventArgs e) {
                wheelDelta += e.Delta; Record("pad_wheel", new { delta = e.Delta }); Publish();
            };
            Controls.Add(pad);
            Controls.Add(new BlockingReadLabel(root) { Location=new Point(12,405), Size=new Size(380,25) });
            Shown += delegate { ready = true; editor.Focus(); Record("shown", new { }); Publish(); };
            FormClosing += delegate(object sender, FormClosingEventArgs e) {
                if (File.Exists(Path.Combine(root,"require-save")) && editor.Text != saved) {
                    var answer=MessageBox.Show(this,"Save the owned audit document?","Unsaved audit document",MessageBoxButtons.YesNoCancel);
                    if(answer==DialogResult.Cancel) e.Cancel=true;
                    else if(answer==DialogResult.Yes) Save("quit_prompt");
                }
            };
            FormClosed += delegate { closed = true; timer.Stop(); Record("closed", new { }); Publish(); Application.RemoveMessageFilter(this); };
            timer.Interval = 50;
            timer.Tick += delegate {
                if (File.Exists(Path.Combine(root, "stop"))) { Close(); return; }
                ApplyLayoutCommand(); Publish();
            };
            Application.AddMessageFilter(this); timer.Start();
        }
        void ApplyLayoutCommand() {
            string path = Path.Combine(root, "layout.json");
            if (!File.Exists(path)) return;
            Dictionary<string, int> command;
            using (var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete))
            using (var reader = new StreamReader(stream, utf8))
                command = json.Deserialize<Dictionary<string, int>>(reader.ReadToEnd());
            int requested = command["sequence"];
            if (requested <= layoutSequence) return;
            int windowOffset = command["window_offset"], controlOffset = command["control_offset"];
            if ((windowOffset != 0 && windowOffset != 12 && windowOffset != -246) || (controlOffset != 0 && controlOffset != 200))
                throw new InvalidOperationException("Unsupported owned layout command");
            Location = new Point(origin.X + windowOffset, origin.Y);
            editor.Location = new Point(12, 12 + controlOffset);
            increment.Location = new Point(12, 145 + controlOffset);
            layoutSequence = requested;
            if (command.ContainsKey("activate") && command["activate"] == 1) { BringToFront(); Activate(); }
            Record("supervisor_layout", command);
        }
        object ControlBounds(Control control) {
            Rectangle bounds = control.RectangleToScreen(new Rectangle(Point.Empty, control.Size));
            return new { x = bounds.X, y = bounds.Y, width = bounds.Width, height = bounds.Height };
        }
        void Save(string source) {
            saved = editor.Text;
            File.WriteAllText(Path.Combine(root, "saved.txt"), saved, utf8);
            saves++; Record("save", new { source = source, count = saves }); Publish();
        }
        protected override bool ProcessCmdKey(ref Message msg, Keys keyData) {
            if (keyData == (Keys.Control | Keys.S)) { Save("shortcut"); return true; }
            if (keyData == (Keys.Control | Keys.A) && editor.Focused) { editor.SelectAll(); return true; }
            return base.ProcessCmdKey(ref msg, keyData);
        }
        public bool PreFilterMessage(ref Message message) {
            Control control = Control.FromHandle(message.HWnd);
            if (control == null || (control != this && !Contains(control))) return false;
            if (message.Msg == 0x100 || message.Msg == 0x104)
                Record("key_down", new { key = message.WParam.ToInt32() });
            if (message.Msg == 0x101 || message.Msg == 0x105)
                Record("key_up", new { key = message.WParam.ToInt32() });
            return false;
        }
        string Encode(object value) {
            // JavaScriptSerializer emits raw UTF-16 surrogates. UTF8Encoding
            // would replace a lone half observed during WM_CHAR processing.
            // Preserve every code unit in JSON, including intermediate states.
            string raw=json.Serialize(value);
            var encoded=new StringBuilder(raw.Length);
            foreach(char c in raw) {
                if(char.IsSurrogate(c)) encoded.Append("\\u").Append(((int)c).ToString("x4"));
                else encoded.Append(c);
            }
            return encoded.ToString();
        }
        void Record(string kind, object data) {
            File.AppendAllText(Path.Combine(root, "events.jsonl"),
                Encode(new { elapsed_ms = clock.Elapsed.TotalMilliseconds, kind = kind, data = data }) + "\n", utf8);
        }
        void Publish() {
            if (!IsHandleCreated) return;
            Rect bounds;
            bool boundsOk = GetWindowRect(Handle, out bounds);
            Point point = pad.PointToScreen(new Point(pad.Width / 2, pad.Height / 2));
            List<int> held = new List<int>();
            for (int key = 1; key <= 255; key++) if (GetAsyncKeyState(key) < 0) held.Add(key);
            string snapshot = Encode(new {
                pid = Process.GetCurrentProcess().Id, label = label, title = Text,
                sequence = ++sequence, elapsed_ms = clock.Elapsed.TotalMilliseconds,
                ready = ready, closed = closed, hwnd = Handle.ToInt64().ToString(),
                visible = IsWindowVisible(Handle), foreground = GetForegroundWindow() == Handle,
                dpi_awareness_set = dpiSet, dpi = GetDpiForWindow(Handle), bounds_ok = boundsOk,
                bounds = new { x = bounds.Left, y = bounds.Top, width = bounds.Right - bounds.Left, height = bounds.Bottom - bounds.Top },
                pad_point = new { x = point.X, y = point.Y },
                layout_sequence = layoutSequence, increment_bounds = ControlBounds(increment), editor_bounds = ControlBounds(editor),
                working_area = new { x = Screen.PrimaryScreen.WorkingArea.X, y = Screen.PrimaryScreen.WorkingArea.Y,
                    width = Screen.PrimaryScreen.WorkingArea.Width, height = Screen.PrimaryScreen.WorkingArea.Height },
                modal_open = modalOpen, editor_focused = editor.Focused, text = editor.Text, saved = saved, saves = saves, increments = increments,
                menu_open = menuOpen, moves = moves, drag_moves = dragMoves,
                left_up = leftUps, right_up = rightUps, double_clicks = doubles, wheel_delta = wheelDelta, horizontal_wheel_delta = horizontalWheelDelta,
                keys_down_global = held
            });
            string path = Path.Combine(root, "state.json"), temp = path + ".next";
            File.WriteAllText(temp, snapshot, utf8);
            if (File.Exists(path)) File.Replace(temp, path, null); else File.Move(temp, path);
        }
        [STAThread] public static void Main(string[] args) {
            if (args.Length == 0) args = new [] { Path.Combine(Path.GetDirectoryName(Application.ExecutablePath),"restart"), "A" };
            if ((args.Length != 2 && args.Length != 3) || (args[1] != "A" && args[1] != "B") ||
                (args.Length == 3 && args[2] != "unaware" && args[2] != "workbench")) return;
            try {
                Directory.CreateDirectory(args[0]);
                bool dpi = SetProcessDpiAwarenessContext(new IntPtr(args.Length == 3 ? -1 : -4));
                Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
                Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException);
                if(args.Length==3&&args[2]=="workbench") {
                    using(var form=new Workbench(args[0]))Application.Run(form);
                } else using (Fixture form = new Fixture(args[0], args[1], dpi)) Application.Run(form);
            } catch (Exception error) {
                File.WriteAllText(Path.Combine(args[0], "fixture-error.txt"), error.ToString());
                Environment.ExitCode = 1;
            }
        }
    }

    // The Windows native shell of computer-use-app. The legacy smoke fixture,
    // dense controls, native file panels and portable web/canvas pages share one
    // source/lifecycle instead of maintaining another set of miniature apps.
    public sealed class NativeScrollRegion : ScrollableControl {
        public int WheelEvents;
        public event Action Changed;
        public NativeScrollRegion(string name, Color color) {
            AccessibleName=name; BackColor=color; AutoScroll=true;
            AutoScrollMinSize=new Size(100,1800); DoubleBuffered=true;
        }
        protected override void OnPaint(PaintEventArgs e) {
            base.OnPaint(e);
            for(int i=0;i<60;i++)e.Graphics.DrawString("Row "+(i+1).ToString("D3"),Font,Brushes.Black,12,AutoScrollPosition.Y+i*28);
        }
        protected override void WndProc(ref Message message) {
            if(message.Msg==0x020A) {
                int delta=(short)((long)message.WParam>>16);
                AutoScrollPosition=new Point(0,Math.Max(0,-AutoScrollPosition.Y-delta/120*60));
                WheelEvents++; Invalidate(); if(Changed!=null)Changed(); return;
            }
            base.WndProc(ref message);
        }
    }

    sealed class ReviewEdit : TextBox {
        public Action Dropped;
        public bool DropNext;
        protected override void OnKeyPress(KeyPressEventArgs e) {
            if(DropNext && !char.IsControl(e.KeyChar)) {
                DropNext=false; e.Handled=true; if(Dropped!=null)Dropped();
            }
            base.OnKeyPress(e);
        }
    }

    sealed class ReviewOverlay : Form {
        protected override bool ShowWithoutActivation { get { return true; } }
        public ReviewOverlay(Point location) {
            StartPosition=FormStartPosition.Manual;Location=location;
            FormBorderStyle=FormBorderStyle.None;ClientSize=new Size(260,28);
            ShowInTaskbar=false;Controls.Add(new Label{Text="Review status overlay",Dock=DockStyle.Fill});
        }
    }

    public sealed class Workbench : Form {
        [StructLayout(LayoutKind.Sequential)] struct ScreenPoint { public int X, Y; }
        [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
        [DllImport("user32.dll")] static extern bool LogicalToPhysicalPointForPerMonitorDPI(IntPtr hwnd,ref ScreenPoint point);
        readonly string root;
        readonly UTF8Encoding utf8=new UTF8Encoding(false);
        readonly JavaScriptSerializer json=new JavaScriptSerializer();
        readonly TextBox editor=new TextBox();
        readonly TextBox reference=new TextBox();
        readonly CheckBox reviewed=new CheckBox();
        readonly Timer timer=new Timer();
        int commits,checkpoint,checks,opens,saves;
        TrackBar paritySlider;
        NativeScrollRegion parityLeft,parityRight;
        Form peer;
        ReviewEdit reviewCode;
        int formLoads,formReady,droppedKeys,formSubmissions;
        string submittedCode="";
        int sliderDown,sliderUp,sliderMoves,peerOpens,peerIncrements,peerCloses;
        bool ready,modal;
        string saved="";
        static Button Button(string name,int x,int y,int width) {
            return new Button{Text=name,AccessibleName=name,Location=new Point(x,y),Size=new Size(width,30)};
        }
        public Workbench(string directory) {
            root=directory;Text="DTW Computer Use Windows";AccessibleName=Text;
            StartPosition=FormStartPosition.Manual;Location=new Point(30,30);
            ClientSize=new Size(1040,850);AutoScaleMode=AutoScaleMode.None;
            editor.AccessibleName="Document editor";editor.Multiline=true;editor.AcceptsReturn=true;
            editor.Location=new Point(12,12);editor.Size=new Size(640,110);Controls.Add(editor);
            var commit=Button("Commit document",12,130,155);
            commit.Click+=delegate{saved=editor.Text;commits++;Event("commit");editor.Focus();Publish();};Controls.Add(commit);
            reviewed.Text="Reviewed";reviewed.AccessibleName="Reviewed";reviewed.Location=new Point(180,134);
            reviewed.CheckedChanged+=delegate{checks++;Event("reviewed");Publish();};Controls.Add(reviewed);
            var open=Button("Open document",12,170,155);Controls.Add(open);
            open.Click+=delegate{BeginInvoke((Action)delegate{
                modal=true;Publish();
                try{using(var panel=new OpenFileDialog{InitialDirectory=root,Title="Open owned document",Filter="Text documents|*.txt|All files|*.*"}) {
                    if(panel.ShowDialog(this)==DialogResult.OK){editor.Text=File.ReadAllText(panel.FileName,utf8);opens++;Event("open");}
                }}finally{modal=false;Publish();}
            });};
            var save=Button("Save document as",180,170,155);Controls.Add(save);
            save.Click+=delegate{BeginInvoke((Action)delegate{
                modal=true;Publish();
                try{using(var panel=new SaveFileDialog{InitialDirectory=root,Title="Save owned document",Filter="Text documents|*.txt",DefaultExt="txt",AddExtension=true}) {
                    if(panel.ShowDialog(this)==DialogResult.OK){File.WriteAllText(panel.FileName,editor.Text,utf8);saves++;Event("save");}
                }}finally{modal=false;Publish();}
            });};
            var show=Button("Show owned modal",348,170,155);Controls.Add(show);
            show.Click+=delegate{BeginInvoke((Action)delegate{
                modal=true;Publish();try{using(var dialog=new Form{Text="Workbench owned modal",AccessibleName="Workbench owned modal",ClientSize=new Size(300,100),StartPosition=FormStartPosition.CenterParent}) {
                    var close=Button("Close owned modal",20,20,220);close.DialogResult=DialogResult.Cancel;dialog.CancelButton=close;dialog.Controls.Add(close);dialog.ShowDialog(this);
                }}finally{modal=false;Publish();}
            });};
            reference.AccessibleName="Long native reference";reference.Multiline=true;reference.ReadOnly=true;
            reference.ScrollBars=ScrollBars.Both;reference.Location=new Point(12,220);reference.Size=new Size(640,180);
            reference.Text=File.ReadAllText(Path.Combine(root,"native-reference.txt"),utf8);Controls.Add(reference);
            var dense=new FlowLayoutPanel{Location=new Point(670,12),Size=new Size(355,820),AutoScroll=true};
            for(int i=1;i<=190;i++) {
                int index=i;var button=Button("Checkpoint "+i,0,0,150);
                button.Click+=delegate{checkpoint=index;Event("checkpoint");Publish();};dense.Controls.Add(button);
            }
            Controls.Add(dense);
            var menu=new MainMenu();var file=new MenuItem("Workbench File");
            file.MenuItems.Add(new MenuItem("Commit document",delegate{saved=editor.Text;commits++;Event("menu_commit");Publish();}));
            menu.MenuItems.Add(file);Menu=menu;
            var label=new Label{Text="Web / Canvas chat / Inventory use the same loopback workspace and independent journal.\nThe supervisor starts an isolated Chromium profile; no legacy WebBrowser/IE substitute.",
                Location=new Point(12,420),Size=new Size(640,80)};Controls.Add(label);
            if(File.Exists(Path.Combine(root,"extended-parity")))BuildParity();
            if(File.Exists(Path.Combine(root,"protocol-review")))BuildProtocolReview();
            Shown+=delegate{ready=true;editor.Focus();Event("ready");Publish();};
            FormClosed+=delegate{timer.Stop();Event("quit");};
            timer.Interval=30;timer.Tick+=delegate{
                if(File.Exists(Path.Combine(root,"stop"))||File.Exists(Path.Combine(root,"command"))){Close();return;}Publish();
            };timer.Start();
        }
        void BuildProtocolReview() {
            var load=Button("Load review form",12,505,220);
            var inject=Button("Load form with input fault",260,505,250);
            reviewCode=new ReviewEdit{AccessibleName="Review code",Location=new Point(12,563),Size=new Size(330,26),Visible=false};
            var submit=Button("Submit review",12,600,220);submit.Visible=false;
            var loading=new Timer{Interval=750};
            ReviewOverlay overlay=null;
            var overlayTimer=new Timer{Interval=5000};
            Action clearOverlay=delegate {
                overlayTimer.Stop();
                if(overlay!=null){overlay.Close();overlay=null;}
            };
            overlayTimer.Tick+=delegate{clearOverlay();};
            Action<bool> start=delegate(bool drop) {
                clearOverlay();
                loading.Stop();reviewCode.Hide();submit.Hide();
                reviewCode.Text="";reviewCode.DropNext=drop;
                Text="DTW Computer Use Windows - Loading review";
                formLoads++;Event("review_loading");Publish();loading.Start();
            };
            loading.Tick+=delegate {
                loading.Stop();reviewCode.Show();submit.Show();reviewCode.Focus();
                Text="DTW Computer Use Windows - Review form ready";
                overlay=new ReviewOverlay(PointToScreen(new Point(370,635)));
                overlay.Show(this);overlayTimer.Start();
                formReady++;Event("review_ready");Publish();
            };
            load.Click+=delegate{start(false);};inject.Click+=delegate{start(true);};
            reviewCode.Dropped=delegate{droppedKeys++;Event("review_key_dropped");Publish();};
            Action save=delegate {
                submittedCode=reviewCode.Text;formSubmissions++;Event("review_submit");Publish();
            };
            submit.Click+=delegate{save();};
            reviewCode.KeyDown+=delegate(object sender,KeyEventArgs e) {
                if(e.KeyCode==Keys.Enter){e.SuppressKeyPress=true;save();}
            };
            Controls.Add(load);Controls.Add(inject);Controls.Add(reviewCode);Controls.Add(submit);
            FormClosed+=delegate{loading.Dispose();overlayTimer.Dispose();};
        }

        void BuildParity() {
            var status=new Label{Text="Native slider: 0 / 200",Location=new Point(12,500),Size=new Size(310,22)};Controls.Add(status);
            paritySlider=new TrackBar{AccessibleName="Parity slider",Minimum=0,Maximum=200,TickFrequency=20,
                Location=new Point(12,526),Size=new Size(640,45)};Controls.Add(paritySlider);
            paritySlider.ValueChanged+=delegate{status.Text="Native slider: "+paritySlider.Value+" / 200";Event("slider_value");Publish();};
            paritySlider.MouseDown+=delegate(object sender,MouseEventArgs e){if(e.Button==MouseButtons.Left){sliderDown++;Event("slider_down");Publish();}};
            paritySlider.MouseUp+=delegate(object sender,MouseEventArgs e){if(e.Button==MouseButtons.Left){sliderUp++;Event("slider_up");Publish();}};
            paritySlider.MouseMove+=delegate(object sender,MouseEventArgs e){if((e.Button&MouseButtons.Left)!=0){sliderMoves++;Publish();}};
            var leftTitle=new Label{Text="Left scroll region (scroll this)",Location=new Point(12,578),Size=new Size(305,22)};Controls.Add(leftTitle);
            var rightTitle=new Label{Text="Right scroll region (keep at top)",Location=new Point(338,578),Size=new Size(310,22)};Controls.Add(rightTitle);
            parityLeft=new NativeScrollRegion("Left scroll region",Color.Honeydew){Location=new Point(12,605),Size=new Size(305,210)};
            parityRight=new NativeScrollRegion("Right scroll region",Color.Lavender){Location=new Point(338,605),Size=new Size(310,210)};
            parityLeft.Changed+=delegate{Event("left_scroll");Publish();};parityRight.Changed+=delegate{Event("right_scroll");Publish();};
            Controls.Add(parityLeft);Controls.Add(parityRight);
            var openPeer=Button("Open peer window",338,497,250);Controls.Add(openPeer);
            openPeer.Click+=delegate{
                if(peer!=null&&!peer.IsDisposed)return;
                peer=new Form{Text="Owned parity peer",AccessibleName="Owned parity peer",ClientSize=new Size(300,140),
                    StartPosition=FormStartPosition.Manual,Location=new Point(Left+200,Top+220),ShowInTaskbar=false};
                var incrementPeer=Button("Peer increment",20,20,250);peer.Controls.Add(incrementPeer);
                incrementPeer.Click+=delegate{peerIncrements++;incrementPeer.Text="Peer increment";Event("peer_increment");Publish();};
                var closePeer=Button("Close peer",20,70,250);peer.Controls.Add(closePeer);
                closePeer.Click+=delegate{peer.BeginInvoke((Action)delegate{peer.Close();});};
                peer.FormClosed+=delegate{peerCloses++;Event("peer_close");Publish();};
                peerOpens++;peer.Show(this);Event("peer_open");Publish();
            };
        }
        void Event(string kind) {File.AppendAllText(Path.Combine(root,"native-events.jsonl"),json.Serialize(new{kind=kind,pid=Process.GetCurrentProcess().Id,unix_ms=(DateTime.UtcNow.Ticks-621355968000000000L)/10000.0})+"\n",utf8);}
        void Publish() {
            if(IsDisposed||Disposing||!IsHandleCreated)return;
            string path=Path.Combine(root,"native-state.json"),temporary=path+".next";
            int[] pointer=null;
            if(reviewCode!=null) {
                var logical=reviewed.PointToScreen(new Point(8,reviewed.Height/2));
                var physical=new ScreenPoint{X=logical.X,Y=logical.Y};
                if(LogicalToPhysicalPointForPerMonitorDPI(reviewed.Handle,ref physical))
                    pointer=new[]{physical.X,physical.Y};
            }
            File.WriteAllText(temporary,json.Serialize(new{pid=Process.GetCurrentProcess().Id,ready=ready,modal=modal,hwnd=Handle.ToInt64(),
                text=editor.Text,saved=saved,commits=commits,checkpoint=checkpoint,reviewed=reviewed.Checked,checks=checks,opens=opens,saves=saves,
                review=reviewCode==null?null:new{loads=formLoads,ready=formReady,dropped_keys=droppedKeys,
                    submissions=formSubmissions,submitted_code=submittedCode,visible_value=reviewCode.Text,pointer=pointer},
                parity=paritySlider==null?null:new{slider=paritySlider.Value,slider_down=sliderDown,slider_up=sliderUp,slider_drag_moves=sliderMoves,
                    left_scroll=-parityLeft.AutoScrollPosition.Y,right_scroll=-parityRight.AutoScrollPosition.Y,
                    left_wheels=parityLeft.WheelEvents,right_wheels=parityRight.WheelEvents,
                    peer_opens=peerOpens,peer_increments=peerIncrements,peer_closes=peerCloses,peer_open=peer!=null&&!peer.IsDisposed,
                    global_left_held=GetAsyncKeyState(1)<0}}),utf8);
            if(File.Exists(path))File.Replace(temporary,path,null);else File.Move(temporary,path);
        }
    }
}
