// Isolated native fixture. Logs are an application-owned oracle, not UIA readback.
using System;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Forms;

class WindowsAcceptance : Form {
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    readonly string root;
    readonly TextBox input = new TextBox();
    readonly Label status = new Label();
    int submissions;

    void Log(string kind, string value) {
        File.AppendAllText(Path.Combine(root, "events.tsv"), kind + "\t" +
            Convert.ToBase64String(Encoding.UTF8.GetBytes(value)) + "\n", new UTF8Encoding(false));
    }

    void Status(string value) { status.Text = value; status.AccessibleName = value; }

    Button AddButton(string text, int x, int y, EventHandler click) {
        var button = new Button { Text = text, AccessibleName = text,
            Location = new Point(x, y), Size = new Size(155, 40) };
        button.Click += click;
        Controls.Add(button);
        return button;
    }

    WindowsAcceptance(string directory, string title) {
        root = directory;
        Text = title;
        AccessibleName = title;
        ClientSize = new Size(720, 440);
        StartPosition = FormStartPosition.Manual;
        Location = new Point(120, 120);
        KeyPreview = true;
        var menu = new MenuStrip();
        var file = new ToolStripMenuItem("File");
        file.DropDownItems.Add("Record menu", null, delegate {
            Status("MENU-OK"); Log("menu", "MENU-OK");
        });
        menu.Items.Add(file); MainMenuStrip = menu; Controls.Add(menu);
        Controls.Add(new Label { Text = "Test input", Location = new Point(25, 50), AutoSize = true });
        input.AccessibleName = "Test input";
        input.Location = new Point(25, 80); input.Size = new Size(650, 75);
        input.Multiline = true;
        input.TextChanged += delegate { Log("input", input.Text); };
        input.KeyDown += delegate(object sender, KeyEventArgs e) { Log("key", e.KeyData.ToString()); };
        Controls.Add(input);
        AddButton("Submit once", 25, 175, delegate {
            submissions++; Status("SUBMITTED-" + submissions);
            Log("submit", input.Text);
        });
        var pixelTarget = AddButton("Pixel target", 200, 175, delegate {
            Status("PIXEL-OK"); Log("pixel", "PIXEL-OK");
        });
        pixelTarget.FlatStyle = FlatStyle.Flat;
        pixelTarget.FlatAppearance.BorderSize = 0;
        pixelTarget.BackColor = Color.FromArgb(32, 160, 96);
        pixelTarget.FlatAppearance.MouseOverBackColor = pixelTarget.BackColor;
        pixelTarget.FlatAppearance.MouseDownBackColor = pixelTarget.BackColor;
        AddButton("Open test picker", 375, 175, delegate {
            Log("picker_opened", "DTW test picker");
            using (var picker = new OpenFileDialog { Title = "DTW test picker",
                InitialDirectory = root, FileName = "picker.txt", CheckFileExists = true }) {
                var selected = picker.ShowDialog(this);
                Log("picker", selected.ToString());
                Status("PICKER-" + selected);
            }
        });
        var check = new CheckBox { Text = "Test checkbox", AccessibleName = "Test checkbox",
            Location = new Point(25, 235), AutoSize = true };
        check.CheckedChanged += delegate { Log("check", check.Checked.ToString()); };
        Controls.Add(check);
        status.Location = new Point(25, 280); status.Size = new Size(650, 45);
        Status("READY-NATIVE"); Controls.Add(status);
        var canvas = new Panel { AccessibleName = "Mouse surface", Location = new Point(25, 340),
            Size = new Size(650, 75), BackColor = Color.LightSteelBlue };
        canvas.MouseDown += delegate(object sender, MouseEventArgs e) { Log("mouse_down", e.X + "," + e.Y); };
        canvas.MouseUp += delegate(object sender, MouseEventArgs e) { Log("mouse_up", e.X + "," + e.Y); };
        canvas.MouseWheel += delegate(object sender, MouseEventArgs e) { Log("wheel", e.Delta.ToString()); };
        Controls.Add(canvas);
        KeyDown += delegate(object sender, KeyEventArgs e) {
            if (e.Control && e.KeyCode == Keys.S) {
                File.WriteAllText(Path.Combine(root, "saved.txt"), input.Text, new UTF8Encoding(false));
                Log("save", input.Text); Status("SAVED-NATIVE");
                e.SuppressKeyPress = true;
            }
        };
        Shown += delegate { Log("ready", Text); };
    }

    [STAThread]
    static void Main(string[] args) {
        if (args.Length > 2 && args[2] == "dpi-aware") SetProcessDPIAware();
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new WindowsAcceptance(args[0], args[1]));
    }
}
