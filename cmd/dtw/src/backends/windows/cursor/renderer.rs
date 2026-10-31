use super::{
    identity::{self, Identity},
    native, raster, Presentation, Target, CLASS, MAX_PACKET,
};
use crate::backends::windows::worker;
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    net::{Ipv4Addr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::GetModuleHandleW, Threading::*},
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

// Layout fingerprints are independent of app awareness. Effective scale comes
// from the owned PMv2 helper HWND after placement, and is monitored separately.
pub(super) fn displays() -> Option<Vec<[i32; 4]>> {
    unsafe extern "system" fn collect(_: HMONITOR, _: HDC, r: *mut RECT, data: LPARAM) -> BOOL {
        if r.is_null() {
            return BOOL(0);
        }
        let output = &mut *(data.0 as *mut Vec<[i32; 4]>);
        let r = *r;
        output.push([r.left, r.top, r.right, r.bottom]);
        BOOL(i32::from(output.len() <= 64))
    }
    let mut output = Vec::<[i32; 4]>::new();
    if !unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut output as *mut _ as isize),
        )
    }
    .as_bool()
        || output.is_empty()
    {
        return None;
    }
    output.sort_unstable();
    Some(output)
}
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, message, w, l),
    }
}
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}
impl Surface {
    fn new(image: &raster::Image) -> windows::core::Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: image.width,
                    biHeight: -image.height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            if bits.is_null() {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(dc);
                return Err(windows::core::Error::from_win32());
            }
            let old = SelectObject(dc, bitmap);
            if old.0.is_null() || old.0 as isize == -1 {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(dc);
                return Err(windows::core::Error::from_win32());
            }
            Ok(Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                width: image.width,
                height: image.height,
            })
        }
    }
    fn present(&self, h: HWND, image: &raster::Image, target: &Target, point: (i32, i32)) -> bool {
        unsafe {
            if !target.valid() {
                return false;
            }
            std::ptr::copy_nonoverlapping(image.pixels.as_ptr(), self.bits, image.pixels.len());
            let point = POINT {
                x: point.0 - image.hotspot,
                y: point.1 - image.hotspot,
            };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let source = POINT::default();
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
                BlendFlags: 0,
            };
            if UpdateLayeredWindow(
                h,
                None,
                Some(&point),
                Some(&size),
                self.dc,
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
            .is_err()
            {
                return false;
            }
            // Clip our panel to the actual target; a hotspot on a border must not
            // draw feedback over another application's adjacent content.
            let b = target.bounds;
            let region = CreateRectRgn(
                (b[0] - point.x).max(0),
                (b[1] - point.y).max(0),
                (b[2] - point.x).min(self.width),
                (b[3] - point.y).min(self.height),
            );
            if region.0.is_null() {
                return false;
            }
            if SetWindowRgn(h, region, false) == 0 {
                let _ = DeleteObject(region);
                return false;
            }
            pin(h, target)
        }
    }
}
fn pin(h: HWND, target: &Target) -> bool {
    unsafe {
        // Do not promote into the topmost band or raise the target. Insert
        // after its ordinary previous sibling, which means just ABOVE target.
        let mut previous = GetWindow(target.hwnd(), GW_HWNDPREV).unwrap_or_default();
        // Independent sessions keep a stable relative stack. Do not make two
        // timers repeatedly leapfrog each other's proven cursor windows.
        identity::begin_cycle();
        while !previous.0.is_null() && previous != h && identity::proven(previous) {
            previous = GetWindow(previous, GW_HWNDPREV).unwrap_or_default();
        }
        if previous == h {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            return target.valid();
        }
        while !previous.0.is_null()
            && GetWindowLongPtrW(previous, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0
        {
            previous = GetWindow(previous, GW_HWNDPREV).unwrap_or_default();
        }
        let after = if previous.0.is_null() {
            HWND_TOP
        } else {
            previous
        };
        if SetWindowPos(
            h,
            after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
        )
        .is_err()
        {
            return false;
        }
        target.valid() && GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}
struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
fn create_window() -> windows::core::Result<Window> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
        let module = GetModuleHandleW(None)?;
        let class: Vec<_> = CLASS.encode_utf16().chain(Some(0)).collect();
        let registered = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(module.0),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        });
        if registered == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            PCWSTR(class.as_ptr()),
            w!("DTW session cursor"),
            WS_POPUP | WS_DISABLED,
            0,
            0,
            1,
            1,
            None,
            None,
            HINSTANCE(module.0),
            None,
        )?;
        Ok(Window(hwnd))
    }
}
fn run(expected: u32, birth: u64) -> Result<(), Box<dyn std::error::Error>> {
    let parent = native::process_stamp(expected).map_err(|e| format!("{e:?}"))?;
    let own = native::process_stamp(std::process::id()).map_err(|e| format!("{e:?}"))?;
    if worker::parent_pid()? != expected
        || parent.generation != birth
        || parent.identity != own.identity
    {
        return Err("cursor parent identity mismatch".into());
    }
    let alive = worker::parent_alive_check()?;
    let closed = Arc::new(AtomicBool::new(false));
    let name = worker::event_name(std::process::id());
    let stop =
        worker::ProcessHandle(unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }?);
    let watchdog = closed.clone();
    std::thread::spawn(move || {
        while !watchdog.load(Ordering::Acquire) {
            if !alive() || !stop.alive() {
                watchdog.store(true, Ordering::Release);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Presentation owns no input. A wedged UI loop may not outlive the
        // existing cooperative Worker's termination grace.
        std::thread::sleep(Duration::from_millis(400));
        std::process::exit(0);
    });
    let (tx, rx) = mpsc::sync_channel::<Value>(1);
    let eof = closed.clone();
    std::thread::spawn(move || {
        let mut input = std::io::BufReader::new(std::io::stdin());
        loop {
            let mut line = Vec::new();
            let read = input
                .by_ref()
                .take((MAX_PACKET + 2) as u64)
                .read_until(b'\n', &mut line);
            if !matches!(read,Ok(n) if n>0)
                || line.len() > MAX_PACKET + 1
                || line.last() != Some(&b'\n')
            {
                break;
            }
            let Ok(value) = serde_json::from_slice(&line) else {
                break;
            };
            if tx.send(value).is_err() {
                break;
            }
        }
        eof.store(true, Ordering::Release);
    });
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
    socket.set_nonblocking(true)?;
    let mut window = None;
    let mut session = String::new();
    let mut token = String::new();
    let mut state = Presentation::default();
    let mut surface: Option<Surface> = None;
    let mut images: Option<(raster::Image, raster::Image)> = None;
    let mut image_dpi = 0;
    let mut dirty = false;
    let mut pulse = false;
    let mut geometry = Instant::now();
    while !closed.load(Ordering::Acquire) {
        if let Ok(command) = rx.try_recv() {
            let mut response = json!({"ok":false});
            match command["command"].as_str() {
                Some("initialize") if window.is_none() => {
                    let s = command["session"]
                        .as_str()
                        .ok_or("missing cursor session")?;
                    let t = command["token"].as_str().ok_or("missing cursor token")?;
                    if s.len() != 32
                        || !s.bytes().all(|b| b.is_ascii_hexdigit())
                        || t.len() != 64
                        || !t.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err("invalid cursor bootstrap".into());
                    }
                    let created = create_window()?;
                    let caption: Vec<u16> = format!("DTW session cursor {}", &s[..4])
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    unsafe {
                        let _ = SetWindowTextW(created.0, PCWSTR(caption.as_ptr()));
                    }
                    let id = Identity {
                        pid: own.pid,
                        birth: own.generation,
                        hwnd: created.0 .0 as usize as u64,
                    };
                    if !identity::serve(id.clone()) {
                        return Err("cursor identity endpoint unavailable".into());
                    }
                    response = json!({"ok":true,"session":s,"identity":id.json(),"port":socket.local_addr()?.port()});
                    session = s.into();
                    token = t.into();
                    window = Some(created);
                }
                Some("begin") if window.is_some() => {
                    if let (Some(operation), Some(target)) = (
                        command["operation"].as_u64(),
                        Target::parse(&command["target"]),
                    ) {
                        if state.begin(operation, target) {
                            unsafe {
                                let _ = ShowWindow(window.as_ref().unwrap().0, SW_HIDE);
                            }
                            dirty = false;
                            pulse = false;
                            if unsafe { GetDpiForWindow(window.as_ref().unwrap().0) } != image_dpi {
                                // A new independent action may follow a DPI
                                // retirement. Old cached pixels must neither
                                // retire this new operation nor be reused.
                                image_dpi = 0;
                                images = None;
                                surface = None;
                            }
                            response = json!({"ok":true});
                        }
                    }
                }
                Some("hide") => {
                    if let Some(operation) = command["operation"].as_u64() {
                        state.hide(operation);
                        dirty = true;
                        response = json!({"ok":true});
                    }
                }
                _ => (),
            }
            let mut out = std::io::stdout().lock();
            writeln!(out, "{response}")?;
            out.flush()?;
        }
        // Coalesce one paint after draining a bounded batch; sequence/press
        // state retains a down pulse even when the final packet is an up.
        for _ in 0..128 {
            let mut packet = [0u8; MAX_PACKET + 1];
            match socket.recv_from(&mut packet) {
                Ok((n, source)) if source.ip().is_loopback() && n <= MAX_PACKET => {
                    if let Ok(value) = serde_json::from_slice::<Value>(&packet[..n]) {
                        if value["token"] == token && !token.is_empty() {
                            dirty |= state.event(&value, Instant::now());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                _ => break,
            }
        }
        if geometry.elapsed() >= Duration::from_millis(50) {
            geometry = Instant::now();
            if state.target.as_ref().is_some_and(|t| !t.valid()) {
                state.hide(state.operation);
                dirty = true;
            }
            if image_dpi != 0
                && window
                    .as_ref()
                    .is_some_and(|w| unsafe { GetDpiForWindow(w.0) } != image_dpi)
            {
                state.hide(state.operation);
                dirty = true;
            }
            // Reassert relative order without re-rasterizing/compositing idle pixels.
            if let (Some(window), Some(target), Some(_)) =
                (window.as_ref(), state.target.as_ref(), state.point)
            {
                if !pin(window.0, target) {
                    state.hide(state.operation);
                    dirty = true;
                }
            }
        }
        let now_pulse = state
            .pulse_until
            .is_some_and(|until| Instant::now() < until);
        if pulse != now_pulse {
            dirty = true;
            pulse = now_pulse;
        }
        if dirty {
            dirty = false;
            if let Some(window) = window.as_ref() {
                match (state.target.as_ref(), state.point) {
                    (Some(target), Some(point)) => {
                        // Query the DPI of our own PMv2 window, never the
                        // unaware target's 96 DPI or a rounded scale index.
                        // Move hidden across monitors before rasterization.
                        let dpi = unsafe {
                            let monitor = MonitorFromPoint(
                                POINT {
                                    x: point.0,
                                    y: point.1,
                                },
                                MONITOR_DEFAULTTONULL,
                            );
                            if monitor.0.is_null() || !target.valid() {
                                return Err("cursor monitor/target is unproven".into());
                            }
                            if MonitorFromWindow(window.0, MONITOR_DEFAULTTONULL) != monitor {
                                let _ = ShowWindow(window.0, SW_HIDE);
                                SetWindowPos(
                                    window.0,
                                    None,
                                    point.0,
                                    point.1,
                                    0,
                                    0,
                                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                                )?;
                            }
                            GetDpiForWindow(window.0)
                        };
                        if !(48..=768).contains(&dpi) {
                            return Err("cursor window DPI is unproven".into());
                        }
                        if image_dpi != dpi {
                            surface = None;
                            images = None;
                        }
                        if images.is_none() {
                            image_dpi = dpi;
                            images = Some((
                                raster::image(&session, dpi, false),
                                raster::image(&session, dpi, true),
                            ));
                        }
                        let image = if pulse {
                            &images.as_ref().unwrap().1
                        } else {
                            &images.as_ref().unwrap().0
                        };
                        if surface.is_none() {
                            surface = Some(Surface::new(image)?);
                        }
                        if !surface
                            .as_ref()
                            .unwrap()
                            .present(window.0, image, target, point)
                        {
                            state.hide(state.operation);
                            unsafe {
                                let _ = ShowWindow(window.0, SW_HIDE);
                            }
                        }
                    }
                    _ => unsafe {
                        let _ = ShowWindow(window.0, SW_HIDE);
                    },
                }
            }
        }
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    closed.store(true, Ordering::Release);
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    // Drop the exact presentation HWND before acknowledging normal process exit.
    drop(surface);
    drop(window);
    Ok(())
}
pub(in crate::backends::windows) fn worker_entry() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--dtw-cursor-worker") {
        return false;
    }
    let result = (|| {
        let pid = args
            .get(2)
            .and_then(|v| v.parse().ok())
            .ok_or("missing cursor parent")?;
        let birth = args
            .get(3)
            .and_then(|v| v.parse().ok())
            .ok_or("missing cursor parent birth")?;
        run(pid, birth)
    })();
    if let Err(error) = result {
        super::diagnostic("worker", error);
        std::process::exit(1)
    }
    true
}
