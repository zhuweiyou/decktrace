//! Windows 采集后端：Win32 窗口枚举 + GDI `PrintWindow`/`BitBlt`。
//!
//! 这是实验实现。Wine 只能验证窗口枚举和部分 GDI 路径，不能替代真实 Windows
//! 游戏窗口验证；全屏/硬件加速窗口可能需要后续改用 Windows Graphics Capture。

use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use image::RgbImage;
use tracking::WindowSelector;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClientRect, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use crate::{
    CaptureError, CaptureStream, Platform, RawFrame, Result, WindowInfo, WindowService,
    select_window,
};

pub struct WindowsPlatform {
    windows: WindowsWindowService,
}

impl WindowsPlatform {
    pub fn new() -> Result<Self> {
        Ok(Self {
            windows: WindowsWindowService,
        })
    }
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &str {
        "windows"
    }

    fn window_service(&self) -> &dyn WindowService {
        &self.windows
    }

    fn create_capture_stream(
        &self,
        window: &WindowInfo,
        fps: f32,
    ) -> std::result::Result<Box<dyn CaptureStream>, CaptureError> {
        Ok(Box::new(WindowsCaptureStream::new(window.id, fps)?))
    }
}

pub struct WindowsWindowService;

impl WindowService for WindowsWindowService {
    fn list_windows(
        &self,
        include_off_screen: bool,
    ) -> std::result::Result<Vec<WindowInfo>, CaptureError> {
        let mut context = EnumContext {
            include_off_screen,
            windows: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(enum_window),
                LPARAM(&mut context as *mut EnumContext as isize),
            )
        }
        .map_err(|err| CaptureError::Window(format!("枚举窗口失败：{err}")))?;
        Ok(context.windows)
    }

    fn find_window(
        &self,
        selector: &WindowSelector,
    ) -> std::result::Result<WindowInfo, CaptureError> {
        select_window(&self.list_windows(selector.include_off_screen)?, selector)
    }
}

struct EnumContext {
    include_off_screen: bool,
    windows: Vec<WindowInfo>,
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let context = unsafe { &mut *(lparam.0 as *mut EnumContext) };
    if let Some(window) = unsafe { window_info(hwnd, context.include_off_screen) } {
        context.windows.push(window);
    }
    BOOL(1)
}

unsafe fn window_info(hwnd: HWND, include_off_screen: bool) -> Option<WindowInfo> {
    if unsafe { IsIconic(hwnd).as_bool() } {
        return None;
    }
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    if !include_off_screen && !visible {
        return None;
    }
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return None;
    }

    let title_len = unsafe { GetWindowTextLengthW(hwnd) };
    if title_len <= 0 {
        return None;
    }
    let mut title_buf = vec![0u16; title_len as usize + 1];
    let title_len = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
    if title_len <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let app = process_name(pid).unwrap_or_default();

    let mut rect = RECT::default();
    let mut got_rect = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok();
    if !got_rect || rect.right <= rect.left || rect.bottom <= rect.top {
        got_rect = unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok();
    }
    if !got_rect || rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }

    Some(WindowInfo {
        id: hwnd.0 as usize as u64,
        title,
        app,
        pid: pid as i32,
        x: rect.left as f64,
        y: rect.top as f64,
        width: (rect.right - rect.left) as f64,
        height: (rect.bottom - rect.top) as f64,
        onscreen: visible,
        layer: 0,
    })
}

fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 1024];
        let mut len = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    }
}

pub struct WindowsCaptureStream {
    hwnd: u64,
    interval: Duration,
    started: Instant,
    next: Instant,
    seq: u64,
}

impl WindowsCaptureStream {
    pub fn new(hwnd: u64, fps: f32) -> Result<Self> {
        if hwnd == 0 {
            return Err(CaptureError::Window("无效的窗口句柄".to_string()));
        }
        let fps = fps.clamp(1.0, 60.0);
        let now = Instant::now();
        Ok(Self {
            hwnd,
            interval: Duration::from_secs_f64(1.0 / f64::from(fps)),
            started: now,
            next: now,
            seq: 0,
        })
    }
}

impl CaptureStream for WindowsCaptureStream {
    fn start(&mut self) -> Result<()> {
        let now = Instant::now();
        self.started = now;
        self.next = now;
        Ok(())
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        let now = Instant::now();
        if self.next > now {
            let wait = (self.next - now).min(timeout);
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            if Instant::now() < self.next {
                return Ok(None);
            }
        }

        let image = capture_window(self.hwnd)?;
        self.seq += 1;
        self.next = Instant::now() + self.interval;
        Ok(Some(RawFrame {
            seq: self.seq,
            timestamp: self.started.elapsed().as_secs_f64(),
            width: image.width(),
            height: image.height(),
            image,
        }))
    }

    fn stop(&mut self) {}
}

fn capture_window(hwnd: u64) -> Result<RgbImage> {
    let hwnd = HWND(hwnd as usize as *mut c_void);
    if unsafe { IsIconic(hwnd).as_bool() } {
        return Err(CaptureError::Capture("游戏窗口已最小化".to_string()));
    }

    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client) }
        .map_err(|err| CaptureError::Capture(format!("读取客户区失败：{err}")))?;
    let width = (client.right - client.left).max(0) as u32;
    let height = (client.bottom - client.top).max(0) as u32;
    if width == 0 || height == 0 {
        return Err(CaptureError::Capture("窗口客户区为空".to_string()));
    }

    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            return Err(CaptureError::Capture("获取屏幕 DC 失败".to_string()));
        }
        let mem_dc = CreateCompatibleDC(None);
        if mem_dc.0.is_null() {
            ReleaseDC(None, screen_dc);
            return Err(CaptureError::Capture("创建内存 DC 失败".to_string()));
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return Err(CaptureError::Capture("创建位图失败".to_string()));
        }
        let old = SelectObject(mem_dc, bitmap.into());

        // PrintWindow 能拿到被遮挡窗口；部分硬件加速窗口会失败，退回屏幕 BitBlt。
        let printed = PrintWindow(hwnd, mem_dc, PRINT_WINDOW_FLAGS(3)).as_bool();
        if !printed {
            let mut origin = POINT::default();
            let has_origin = ClientToScreen(hwnd, &mut origin).as_bool();
            if !has_origin
                || BitBlt(
                    mem_dc,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    Some(screen_dc),
                    origin.x,
                    origin.y,
                    SRCCOPY,
                )
                .is_err()
            {
                SelectObject(mem_dc, old);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
                return Err(CaptureError::Capture(
                    "PrintWindow/BitBlt 均失败".to_string(),
                ));
            }
        }

        let mut buffer = vec![0u8; width as usize * height as usize * 4];
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let lines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height,
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        if lines <= 0 {
            return Err(CaptureError::Capture("读取位图失败".to_string()));
        }
        bgra_to_rgb(&buffer, width, height)
            .ok_or_else(|| CaptureError::Capture("BGRA 转 RGB 失败".to_string()))
    }
}

fn bgra_to_rgb(buffer: &[u8], width: u32, height: u32) -> Option<RgbImage> {
    let mut data = vec![0u8; width as usize * height as usize * 3];
    let (dst_pixels, _) = data.as_chunks_mut::<3>();
    let (src_pixels, _) = buffer.as_chunks::<4>();
    for (dst, src) in dst_pixels.iter_mut().zip(src_pixels) {
        *dst = [src[2], src[1], src[0]];
    }
    RgbImage::from_raw(width, height, data)
}
