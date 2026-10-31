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
        int sequence, increments, saves, leftUps, rightUps, doubles, wheelDelta, layoutSequence;
        string saved = "";
        bool ready, closed;

        Fixture(string directory, string instance, bool awarenessSet) {
            root = directory; label = instance; dpiSet = awarenessSet;
            Text = "DTW Windows Smoke " + label;
            AccessibleName = Text;
            AutoScaleMode = AutoScaleMode.None;
            StartPosition = FormStartPosition.Manual;
            FormBorderStyle = FormBorderStyle.FixedSingle;
            MaximizeBox = false;
            ClientSize = new Size(410, 440);
            Rectangle area = Screen.PrimaryScreen.WorkingArea;
            Location = new Point(area.Left + 16 + (label == "A" ? 0 : 446), area.Top + 16);
            origin = Location;
            editor.Location = new Point(12, 12); editor.Size = new Size(386, 125);
            editor.Multiline = true; editor.AcceptsReturn = true;
            editor.ScrollBars = ScrollBars.Vertical; editor.AccessibleName = "Message";
            editor.TextChanged += delegate {
                Record("text_changed", new { length = editor.Text.Length });
                if (editor.Text.Length>0 && char.IsHighSurrogate(editor.Text[editor.Text.Length-1]) &&
                    File.Exists(Path.Combine(root,"track-surrogate"))) {
                    Record("surrogate_half",new {length=editor.Text.Length});
                    if (File.Exists(Path.Combine(root,"delay-surrogate"))) System.Threading.Thread.Sleep(40);
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
                    dialog.Controls.Add(close);dialog.CancelButton=close;dialog.ShowDialog(this);
                }
            }); };
            Controls.Add(openDialog);
            pad.Location = new Point(12, 185); pad.Size = new Size(386, 210);
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
            if ((windowOffset != 0 && windowOffset != 12) || (controlOffset != 0 && controlOffset != 200))
                throw new InvalidOperationException("Unsupported owned layout command");
            Location = new Point(origin.X + windowOffset, origin.Y);
            editor.Location = new Point(12, 12 + controlOffset);
            increment.Location = new Point(12, 145 + controlOffset);
            layoutSequence = requested;
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
        void Record(string kind, object data) {
            File.AppendAllText(Path.Combine(root, "events.jsonl"),
                json.Serialize(new { elapsed_ms = clock.Elapsed.TotalMilliseconds, kind = kind, data = data }) + "\n", utf8);
        }
        void Publish() {
            if (!IsHandleCreated) return;
            Rect bounds;
            bool boundsOk = GetWindowRect(Handle, out bounds);
            Point point = pad.PointToScreen(new Point(pad.Width / 2, pad.Height / 2));
            List<int> held = new List<int>();
            for (int key = 1; key <= 255; key++) if (GetAsyncKeyState(key) < 0) held.Add(key);
            string snapshot = json.Serialize(new {
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
                editor_focused = editor.Focused, text = editor.Text, saved = saved, saves = saves, increments = increments,
                left_up = leftUps, right_up = rightUps, double_clicks = doubles, wheel_delta = wheelDelta,
                keys_down_global = held
            });
            string path = Path.Combine(root, "state.json"), temp = path + ".next";
            File.WriteAllText(temp, snapshot, utf8);
            if (File.Exists(path)) File.Replace(temp, path, null); else File.Move(temp, path);
        }
        [STAThread] public static void Main(string[] args) {
            if ((args.Length != 2 && args.Length != 3) || (args[1] != "A" && args[1] != "B") ||
                (args.Length == 3 && args[2] != "unaware")) return;
            try {
                Directory.CreateDirectory(args[0]);
                bool dpi = SetProcessDpiAwarenessContext(new IntPtr(args.Length == 3 ? -1 : -4));
                Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
                Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException);
                using (Fixture form = new Fixture(args[0], args[1], dpi)) Application.Run(form);
            } catch (Exception error) {
                File.WriteAllText(Path.Combine(args[0], "fixture-error.txt"), error.ToString());
                Environment.ExitCode = 1;
            }
        }
    }
}
