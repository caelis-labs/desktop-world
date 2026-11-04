// Independent read-only Win32 observer. --spoof is an explicitly owned
// negative-control window, never an alternate route for driving application UI.
using System;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DtwWindowsCursorAcceptance {
    public static class Observer {
        const string ClassName="DesktopWorld.SessionCursor.v1";
        static readonly UTF8Encoding Utf8=new UTF8Encoding(false);
        static readonly JavaScriptSerializer Json=new JavaScriptSerializer();
        [StructLayout(LayoutKind.Sequential)] struct Rect {public int Left,Top,Right,Bottom;}
        [StructLayout(LayoutKind.Sequential)] struct Point {public int X,Y;}
        [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct ProcessEntry {
            public uint Size,Usage,Pid;public IntPtr Heap;public uint Module,Threads,Parent;public int Priority;public uint Flags;
            [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)]public string Name;
        }
        delegate bool EnumProc(IntPtr hwnd,IntPtr data);
        [DllImport("user32.dll")]static extern bool EnumWindows(EnumProc callback,IntPtr data);
        [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetClassName(IntPtr h,StringBuilder text,int size);
        [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetWindowText(IntPtr h,StringBuilder text,int size);
        [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
        [DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr h,out Rect rect);
        [DllImport("user32.dll")]static extern bool IsWindowVisible(IntPtr h);
        [DllImport("user32.dll")]static extern bool IsWindowEnabled(IntPtr h);
        [DllImport("user32.dll")]static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")]static extern bool GetCursorPos(out Point p);
        [DllImport("user32.dll")]static extern short GetAsyncKeyState(int key);
        [DllImport("winmm.dll")]static extern uint timeBeginPeriod(uint milliseconds);
        [DllImport("winmm.dll")]static extern uint timeEndPeriod(uint milliseconds);
        [DllImport("user32.dll")]static extern IntPtr GetDC(IntPtr h);
        [DllImport("user32.dll")]static extern int ReleaseDC(IntPtr h,IntPtr dc);
        [DllImport("gdi32.dll",SetLastError=true)]static extern bool BitBlt(IntPtr destination,int x,int y,int width,int height,IntPtr source,int sx,int sy,uint operation);
        [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW")]static extern IntPtr GetWindowLongPtr(IntPtr h,int index);
        [DllImport("user32.dll")]static extern uint GetDpiForWindow(IntPtr h);
        [DllImport("user32.dll")]static extern bool SetProcessDpiAwarenessContext(IntPtr context);
        [DllImport("kernel32.dll")]static extern IntPtr CreateToolhelp32Snapshot(uint flags,uint pid);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode)]static extern bool Process32First(IntPtr snap,ref ProcessEntry p);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode)]static extern bool Process32Next(IntPtr snap,ref ProcessEntry p);
        [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr h);
        static Dictionary<uint,uint> Parents() {
            var result=new Dictionary<uint,uint>();var snap=CreateToolhelp32Snapshot(2,0);
            try {var p=new ProcessEntry();p.Size=(uint)Marshal.SizeOf(p);
                if(Process32First(snap,ref p))do {result[p.Pid]=p.Parent;}while(Process32Next(snap,ref p));
            }finally {CloseHandle(snap);}return result;
        }
        static object Bounds(Rect r) {return new {x=r.Left,y=r.Top,width=r.Right-r.Left,height=r.Bottom-r.Top};}
        static readonly int[] Palette={0xB284FF,0xF784AA,0x60DAAE,0xF4B242,0x4CCCE0,0xDD71EC,0xE85262,0xB8DC36,0x507EEC};
        static void Snapshot(string output,IntPtr target) {
            var parents=Parents();var rows=new List<object>();Rect scene;GetWindowRect(target,out scene);
            uint targetPid;GetWindowThreadProcessId(target,out targetPid);Point cursor;GetCursorPos(out cursor);
            bool foreground=GetForegroundWindow()==target;string imagePath=Path.ChangeExtension(output,".png");Bitmap desktop=null;
            if(foreground&&IsWindowVisible(target)&&scene.Right>scene.Left&&scene.Bottom>scene.Top&&scene.Right-scene.Left<=8192&&scene.Bottom-scene.Top<=8192) {
                desktop=new Bitmap(scene.Right-scene.Left,scene.Bottom-scene.Top,PixelFormat.Format32bppArgb);
                IntPtr screen=GetDC(IntPtr.Zero);
                try {using(var g=Graphics.FromImage(desktop)) {IntPtr dc=g.GetHdc();try {
                    if(!BitBlt(dc,0,0,desktop.Width,desktop.Height,screen,scene.Left,scene.Top,0x40CC0020))throw new System.ComponentModel.Win32Exception();
                }finally{g.ReleaseHdc(dc);}}}finally{ReleaseDC(IntPtr.Zero,screen);}
            }
            try {
                EnumWindows(delegate(IntPtr h,IntPtr unused) {
                    var text=new StringBuilder(128);GetClassName(h,text,text.Capacity);if(text.ToString()!=ClassName)return true;
                    uint pid;GetWindowThreadProcessId(h,out pid);Rect r;GetWindowRect(h,out r);uint parent=parents.ContainsKey(pid)?parents[pid]:0;
                    string path=null;long birth=0;try{var p=Process.GetProcessById((int)pid);path=p.MainModule.FileName;birth=p.StartTime.ToUniversalTime().ToFileTimeUtc();}catch{}
                    var counts=new int[9];
                    if(desktop!=null&&IsWindowVisible(h))for(int y=Math.Max(r.Top,scene.Top);y<Math.Min(r.Bottom,scene.Bottom);y++)
                        for(int x=Math.Max(r.Left,scene.Left);x<Math.Min(r.Right,scene.Right);x++) {
                            var color=desktop.GetPixel(x-scene.Left,y-scene.Top);int rgb=color.ToArgb()&0xFFFFFF;
                            for(int i=0;i<Palette.Length;i++)if(rgb==Palette[i])counts[i]++;
                        }
                    var title=new StringBuilder(128);GetWindowText(h,title,title.Capacity);
                    rows.Add(new {hwnd=h.ToInt64(),pid=pid,parent=parent,birth=birth,path=path,title=title.ToString(),bounds=Bounds(r),visible=IsWindowVisible(h),enabled=IsWindowEnabled(h),
                        ex_style=GetWindowLongPtr(h,-20).ToInt64(),style=GetWindowLongPtr(h,-16).ToInt64(),dpi=GetDpiForWindow(h),palette_pixels=counts});
                    return true;
                },IntPtr.Zero);
                bool after=GetForegroundWindow()==target;Rect afterRect;GetWindowRect(target,out afterRect);
                if(desktop!=null&&after&&scene.Left==afterRect.Left&&scene.Top==afterRect.Top&&scene.Right==afterRect.Right&&scene.Bottom==afterRect.Bottom)desktop.Save(imagePath,ImageFormat.Png);
                else imagePath=null;
                var tree=new List<object>();foreach(var pair in parents)tree.Add(new {pid=pair.Key,parent=pair.Value});
                File.WriteAllText(output,Json.Serialize(new {target=target.ToInt64(),target_pid=targetPid,target_bounds=Bounds(scene),foreground_before=foreground,foreground_after=after,
                    system_cursor=new{x=cursor.X,y=cursor.Y},windows=rows,processes=tree,desktop_image=imagePath}),Utf8);
            }finally{if(desktop!=null)desktop.Dispose();}
        }
        delegate IntPtr WindowProc(IntPtr h,uint message,IntPtr w,IntPtr l);
        [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]struct WindowClass {
            public uint Style;public WindowProc Proc;public int ExtraClass,ExtraWindow;public IntPtr Instance,Icon,Cursor,Brush;
            public string Menu,Name;
        }
        [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern ushort RegisterClass(ref WindowClass value);
        [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr CreateWindowEx(uint extended,string cls,string title,uint style,int x,int y,int w,int h,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr param);
        [DllImport("user32.dll")]static extern IntPtr DefWindowProc(IntPtr h,uint message,IntPtr w,IntPtr l);
        [DllImport("user32.dll")]static extern bool DestroyWindow(IntPtr h);
        [DllImport("user32.dll")]static extern bool ShowWindow(IntPtr h,int show);
        [DllImport("user32.dll")]static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
        [DllImport("user32.dll")]static extern bool SetLayeredWindowAttributes(IntPtr h,uint key,byte alpha,uint flags);
        [DllImport("gdi32.dll")]static extern IntPtr CreateSolidBrush(uint color);
        [DllImport("gdi32.dll")]static extern IntPtr CreateCompatibleDC(IntPtr source);
        [DllImport("gdi32.dll")]static extern bool DeleteDC(IntPtr dc);
        [DllImport("gdi32.dll")]static extern bool DeleteObject(IntPtr value);
        [DllImport("gdi32.dll")]static extern IntPtr SelectObject(IntPtr dc,IntPtr value);
        [DllImport("gdi32.dll")]static extern IntPtr CreateCompatibleBitmap(IntPtr dc,int width,int height);
        [StructLayout(LayoutKind.Sequential)]struct BitmapHeader {public uint Size;public int Width,Height;public ushort Planes,Bits;public uint Compression,ImageSize;public int X,Y;public uint Used,Important;}
        [StructLayout(LayoutKind.Sequential)]struct BitmapInfo {public BitmapHeader Header;public uint Colors;}
        [DllImport("gdi32.dll")]static extern IntPtr CreateDIBSection(IntPtr dc,ref BitmapInfo info,uint usage,out IntPtr bits,IntPtr section,uint offset);
        [DllImport("gdi32.dll")]static extern int GetDIBits(IntPtr dc,IntPtr bitmap,uint first,uint count,byte[] pixels,ref BitmapInfo info,uint usage);
        [DllImport("user32.dll",SetLastError=true)]static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
        [DllImport("user32.dll")]static extern int FillRect(IntPtr dc,ref Rect rect,IntPtr brush);
        [DllImport("kernel32.dll")]static extern void SetLastError(uint error);
        static object SurfacePixels(IntPtr window,string output,bool dib,uint flags,uint background) {
            Rect r;if(!GetWindowRect(window,out r))throw new Exception("Native surface disappeared");
            int width=r.Right-r.Left,height=r.Bottom-r.Top;
            if(width<=0||height<=0||width>4096||height>4096)throw new Exception("Native surface size exceeds diagnostic budget");
            IntPtr screen=GetDC(IntPtr.Zero),memory=IntPtr.Zero,bitmap=IntPtr.Zero,old=IntPtr.Zero,brush=IntPtr.Zero,bits;
            try {
                memory=CreateCompatibleDC(screen);
                var info=new BitmapInfo{Header=new BitmapHeader{Size=40,Width=width,Height=-height,Planes=1,Bits=32}};
                bitmap=dib?CreateDIBSection(screen,ref info,0,out bits,IntPtr.Zero,0):CreateCompatibleBitmap(screen,width,height);
                if(memory==IntPtr.Zero||bitmap==IntPtr.Zero)throw new Exception("Native surface allocation failed");
                old=SelectObject(memory,bitmap);brush=CreateSolidBrush(background);
                var local=new Rect{Right=width,Bottom=height};FillRect(memory,ref local,brush);
                SetLastError(0);bool accepted=PrintWindow(window,memory,flags);int error=Marshal.GetLastWin32Error();
                SelectObject(memory,old);old=IntPtr.Zero;
                var pixels=new byte[width*height*4];
                if(GetDIBits(memory,bitmap,0,(uint)height,pixels,ref info,0)!=height)throw new Exception("Native surface pixel read failed");
                var alpha=new int[256];int colored=0;
                for(int i=0;i<pixels.Length;i+=4){alpha[pixels[i+3]]++;if(pixels[i]!=0||pixels[i+1]!=0||pixels[i+2]!=0)colored++;}
                // RGB diagnostic only: do not pretend GDI's alpha is valid.
                for(int i=3;i<pixels.Length;i+=4)pixels[i]=255;
                using(var image=new Bitmap(width,height,PixelFormat.Format32bppArgb)) {
                    var data=image.LockBits(new Rectangle(0,0,width,height),ImageLockMode.WriteOnly,PixelFormat.Format32bppArgb);
                    try{for(int y=0;y<height;y++)Marshal.Copy(pixels,y*width*4,new IntPtr(data.Scan0.ToInt64()+y*data.Stride),width*4);}
                    finally{image.UnlockBits(data);}image.Save(output,ImageFormat.Png);
                }
                return new{dib=dib,flags=flags,background=background,accepted=accepted,last_error=error,alpha=alpha,colored=colored,pixels=width*height,image=output};
            }finally{
                if(old!=IntPtr.Zero)SelectObject(memory,old);
                if(brush!=IntPtr.Zero)DeleteObject(brush);if(bitmap!=IntPtr.Zero)DeleteObject(bitmap);
                if(memory!=IntPtr.Zero)DeleteDC(memory);if(screen!=IntPtr.Zero)ReleaseDC(IntPtr.Zero,screen);
            }
        }
        static void Surfaces(string output,IntPtr target) {
            uint targetPid;GetWindowThreadProcessId(target,out targetPid);Rect targetBounds;GetWindowRect(target,out targetBounds);
            var rows=new List<object>();
            EnumWindows(delegate(IntPtr window,IntPtr unused){
                uint pid;GetWindowThreadProcessId(window,out pid);var name=new StringBuilder(256);GetClassName(window,name,256);
                if(pid!=targetPid||!IsWindowVisible(window)||(name.ToString()!="tooltips_class32"&&name.ToString()!="SysShadow"))return true;
                Rect r;GetWindowRect(window,out r);
                if(r.Left<targetBounds.Left||r.Top<targetBounds.Top||r.Right>targetBounds.Right||r.Bottom>targetBounds.Bottom)return true;
                var variants=new List<object>();
                foreach(bool dib in new[]{true,false})foreach(uint flags in new uint[]{2,0})foreach(uint background in new uint[]{0,0xFFFFFF}) {
                    string path=output+"-"+window.ToInt64()+"-"+dib+"-"+flags+"-"+background+".png";
                    variants.Add(SurfacePixels(window,path,dib,flags,background));
                }
                rows.Add(new{hwnd=window.ToInt64(),pid=pid,class_name=name.ToString(),bounds=Bounds(r),variants=variants});return true;
            },IntPtr.Zero);
            File.WriteAllText(output,Json.Serialize(new{target=target.ToInt64(),pid=targetPid,windows=rows}),Utf8);
        }
        static void Spoof(string output,IntPtr target) {
            Rect r;GetWindowRect(target,out r);WindowProc proc=delegate(IntPtr h,uint m,IntPtr w,IntPtr l){return DefWindowProc(h,m,w,l);};
            var cls=new WindowClass{Proc=proc,Name=ClassName,Brush=CreateSolidBrush(0x00FF00FF)};
            if(RegisterClass(ref cls)==0)throw new Exception("Owned spoof class registration failed");
            IntPtr hwnd=CreateWindowEx(0x080800A8,ClassName,"Owned cursor-class negative control",0x88000000,r.Left+180,r.Top+260,80,80,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
            if(hwnd==IntPtr.Zero)throw new Exception("Owned spoof creation failed");
            try {
                // A persistent owned cover: ordinary target focus must not make
                // this negative control disappear below the capture window.
                // It remains foreign to DTW regardless of class/basename.
                SetLayeredWindowAttributes(hwnd,0,255,2);ShowWindow(hwnd,4);
                if(!SetWindowPos(hwnd,new IntPtr(-1),0,0,0,0,0x53))throw new Exception("Owned topmost cover placement failed");
                File.WriteAllText(output,Json.Serialize(new {pid=Process.GetCurrentProcess().Id,hwnd=hwnd.ToInt64(),class_name=ClassName,
                    topmost=(GetWindowLongPtr(hwnd,-20).ToInt64()&8)!=0,visible=IsWindowVisible(hwnd)}),Utf8);
                while(!File.Exists(output+".stop")){Application.DoEvents();Thread.Sleep(10);}
            }finally{DestroyWindow(hwnd);GC.KeepAlive(proc);}
        }
        // A separate native process, independent of Python, MCP and the UIA
        // worker. Retain every sample and the actual gaps; never infer a quiet
        // interval or input release from sparse before/after snapshots.
        static string Sample(long now,double gap) {
            IntPtr foreground=GetForegroundWindow();uint pid;
            GetWindowThreadProcessId(foreground,out pid);
            Point point;bool readable=GetCursorPos(out point);
            var held=new List<int>();
            for(int key=1;key<256;key++)if(GetAsyncKeyState(key)<0)held.Add(key);
            return Json.Serialize(new {timestamp=now,frequency=Stopwatch.Frequency,
                unix_ms=(DateTime.UtcNow.Ticks-621355968000000000L)/10000.0,
                gap_ms=gap,foreground=foreground.ToInt64(),foreground_pid=pid,
                cursor_readable=readable,cursor=new{x=point.X,y=point.Y},held=held});
        }
        static void Stream(string output) {
            bool timer=timeBeginPeriod(1)==0;
            int count=0;long previous=0;double largest=0;
            Process.GetCurrentProcess().PriorityClass=ProcessPriorityClass.High;
            Thread.CurrentThread.Priority=ThreadPriority.Highest;
            // Prime CLR reflection/JSON and Win32 imports before publishing
            // readiness. The probe starts only after the first flushed sample;
            // every sample from that point remains in the strict gap check.
            long warmupStart=Stopwatch.GetTimestamp();Sample(warmupStart,0);
            double warmupMs=(Stopwatch.GetTimestamp()-warmupStart)*1000.0/Stopwatch.Frequency;
            var rows=new BlockingCollection<string>(8192);
            Exception writeError=null;
            var writerThread=new Thread(delegate() {
                try{using(var writer=new StreamWriter(output,false,Utf8)) {
                    int written=0;
                    foreach(string row in rows.GetConsumingEnumerable()) {
                        writer.WriteLine(row);written++;
                        if(written==1||written%32==0)writer.Flush();
                    }
                }}catch(Exception error){writeError=error;}
            });
            writerThread.Start();
            try {
                    while(!File.Exists(output+".stop")) {
                        if(writeError!=null)throw new IOException("Independent observer writer failed",writeError);
                        long now=Stopwatch.GetTimestamp();
                        double gap=previous==0?0:(now-previous)*1000.0/Stopwatch.Frequency;
                        previous=now;largest=Math.Max(largest,gap);
                        string row=Sample(now,gap);
                        if(!rows.TryAdd(row))throw new IOException("Independent observer queue overflow; samples are incomplete");
                        count++;Thread.Sleep(2);
                    }
            }finally {
                bool released=!timer||timeEndPeriod(1)==0;
                rows.CompleteAdding();bool writerStopped=writerThread.Join(5000);
                File.WriteAllText(output+".summary.json",Json.Serialize(new {samples=count,max_gap_ms=largest,
                    warmup_ms=warmupMs,timer_resolution_acquired=timer,period_released=released,writer_stopped=writerStopped,write_error=writeError==null?null:writeError.ToString()}),Utf8);
                if(!writerStopped||writeError!=null||!released)throw new IOException("Independent observer cleanup failed");
            }
        }
        [STAThread]public static void Main(string[] args) {
            SetProcessDpiAwarenessContext(new IntPtr(-4));
            try {if(args.Length!=3)throw new ArgumentException("--snapshot|--spoof output.json owned-target-hwnd");
                if(args[0]=="--snapshot")Snapshot(args[1],new IntPtr(long.Parse(args[2])));
                else if(args[0]=="--stream")Stream(args[1]);
                else if(args[0]=="--surfaces")Surfaces(args[1],new IntPtr(long.Parse(args[2])));
                else if(args[0]=="--spoof")Spoof(args[1],new IntPtr(long.Parse(args[2])));
                else throw new ArgumentException("Unknown observer mode");
            }catch(Exception e){if(args.Length>1)File.WriteAllText(args[1]+".error.txt",e.ToString(),Utf8);Console.Error.WriteLine(e);Environment.ExitCode=1;}
        }
    }
}
