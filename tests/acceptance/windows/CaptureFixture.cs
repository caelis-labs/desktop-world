using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows.Forms;

// A pixel-only fixture. Input evidence comes from this window's own client
// handler, independently of DTW's receipts and screenshots.
public sealed class CaptureFixture : Form {
    [StructLayout(LayoutKind.Sequential)]
    struct MinMaxInfo { public Point Reserved, MaxSize, MaxPosition, MinTrackSize, MaxTrackSize; }
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr window, IntPtr after,
        int x, int y, int width, int height, uint flags);
    readonly string root;
    readonly bool dense;
    readonly JavaScriptSerializer json = new JavaScriptSerializer();
    readonly List<object> hits = new List<object>();
    readonly string[] names = {"blue", "red", "purple", "green", "orange", "teal", "brown", "pink"};
    readonly Color[] colors = {Color.RoyalBlue, Color.Firebrick, Color.Purple, Color.ForestGreen,
                              Color.DarkOrange, Color.DarkCyan, Color.SaddleBrown, Color.DeepPink};
    readonly Rectangle[] controls = new Rectangle[8];
    int sequence;
    readonly Timer timer = new Timer { Interval = 50 };

    CaptureFixture(string root, Rectangle bounds, bool dense) {
        this.root = root;
        this.dense = dense;
        Text = "DTW Capture Fixture";
        AutoScaleMode = AutoScaleMode.None;
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer, true);
        Bounds = bounds;
        BackColor = Color.FromArgb(242, 242, 242);
        for (int i = 0; i < controls.Length; i++) {
            if (dense)
                controls[i] = new Rectangle(64 + i * 49, 70, 40, 40);
            else
                controls[i] = new Rectangle(48 + i % 4 * 250, 116 + i / 4 * 208, 200, 140);
        }
        Shown += delegate {
            // The fixture can deliberately exceed the monitor's visible area.
            // Make that capture budget test independent of the WinForms clamp.
            if (!SetWindowPos(Handle, IntPtr.Zero, bounds.X, bounds.Y, bounds.Width, bounds.Height, 0x14))
                throw new InvalidOperationException("Unable to place capture fixture");
            Publish();
        };
        timer.Tick += delegate {
            if (File.Exists(Path.Combine(root, "stop"))) { Close(); return; }
            string move = Path.Combine(root, "move.txt");
            if (sequence == 0 && File.Exists(move)) {
                string[] coords = File.ReadAllText(move).Split(',');
                int x, y;
                if (coords.Length == 2 && int.TryParse(coords[0], out x) && int.TryParse(coords[1], out y)) {
                    Location = new Point(x, y);
                    ++sequence;
                    Publish();
                }
            }
        };
        FormClosed += delegate { timer.Stop(); timer.Dispose(); };
        timer.Start();
    }

    protected override void WndProc(ref Message message) {
        base.WndProc(ref message);
        if (message.Msg == 0x24) {
            var limits = (MinMaxInfo)Marshal.PtrToStructure(message.LParam, typeof(MinMaxInfo));
            limits.MaxTrackSize = new Point(8192, 8192);
            Marshal.StructureToPtr(limits, message.LParam, false);
        }
    }

    protected override void OnPaint(PaintEventArgs e) {
        for (int i = 0; i < controls.Length; i++) {
            Rectangle button = controls[i];
            using (var brush = new SolidBrush(colors[i])) {
                e.Graphics.FillRectangle(brush, button);
                if (dense) {
                    // Repeated fine local edges in a small part of the canvas.
                    for (int j = 0; j < 3; j++)
                        e.Graphics.FillRectangle(Brushes.White, button.X + 6 + j * 10, button.Y + 8, 5, 24);
                }
            }
        }
    }

    protected override void OnMouseUp(MouseEventArgs e) {
        if (e.Button != MouseButtons.Left) return;
        string target = null;
        for (int i = 0; i < controls.Length; i++)
            if (controls[i].Contains(e.Location)) { target = names[i]; break; }
        var screen = PointToScreen(e.Location);
        hits.Add(new { target = target, point = new [] { screen.X, screen.Y } });
        Publish();
    }

    void Publish() {
        var targets = new Dictionary<string, object>();
        for (int i = 0; i < controls.Length; i++) {
            var rect = controls[i];
            var top = PointToScreen(rect.Location);
            targets[names[i]] = new [] { top.X, top.Y, rect.Width, rect.Height };
        }
        var displayBounds = new List<int[]>();
        foreach (var screen in Screen.AllScreens) {
            var r = screen.Bounds;
            displayBounds.Add(new [] { r.X, r.Y, r.Width, r.Height });
        }
        var origin = PointToScreen(Point.Empty);
        string data = json.Serialize(new {
            bounds = new [] { origin.X, origin.Y, ClientSize.Width, ClientSize.Height },
            dpi = GetDpiForWindow(Handle), displays = displayBounds, sequence = sequence,
            hits = hits, targets = targets
        });
        string path = Path.Combine(root, "state.json");
        string next = path + ".next";
        File.WriteAllText(next, data);
        if (File.Exists(path)) File.Replace(next, path, null); else File.Move(next, path);
    }

    [STAThread]
    public static void Main(string[] args) {
        if (args.Length != 6) return;
        SetProcessDpiAwarenessContext(new IntPtr(-4));
        Application.EnableVisualStyles();
        var bounds = new Rectangle(int.Parse(args[1]), int.Parse(args[2]), int.Parse(args[3]), int.Parse(args[4]));
        using (var fixture = new CaptureFixture(args[0], bounds, args[5] == "dense"))
            Application.Run(fixture);
    }
}
