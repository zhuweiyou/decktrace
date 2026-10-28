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
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
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
        // 让 GetClientRect / 截图使用物理像素，避免缩放显示器下客户区被虚拟化、
        // 截图被裁剪。GUI（winit）通常已设置，重复调用失败无副作用，忽略即可。
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
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
    surface: Option<GdiSurface>,
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
            surface: None,
        })
    }

    /// 忽略帧率节流、立刻抓一帧。用于性能基准与冒烟测试。
    pub fn grab_now(&mut self) -> Result<RawFrame> {
        self.capture_frame()
    }

    fn capture_frame(&mut self) -> Result<RawFrame> {
        let hwnd = HWND(self.hwnd as usize as *mut c_void);
        if unsafe { IsIconic(hwnd).as_bool() } {
            return Err(CaptureError::Capture("游戏窗口已最小化".to_string()));
        }
        let (width, height) = client_size(hwnd)?;

        // 复用同一块 DIB / 内存 DC；窗口尺寸变化时重建。
        let stale = self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != width || s.height != height);
        if stale {
            self.surface = Some(GdiSurface::new(width, height)?);
        }
        let surface = self.surface.as_mut().expect("surface 已初始化");

        surface.grab(hwnd)?;
        let image = surface.to_rgb()?;
        self.seq += 1;
        Ok(RawFrame {
            seq: self.seq,
            timestamp: self.started.elapsed().as_secs_f64(),
            width: image.width(),
            height: image.height(),
            image,
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

        let frame = self.capture_frame()?;
        // 固定节拍推进：用上一个时间槽 + interval，而不是“采完 + interval”。
        // 否则每帧的采集耗时（~17ms）会累加到周期上，15fps 实际只能跑 ~12fps。
        self.next += self.interval;
        if self.next <= Instant::now() {
            // 采集本身超过一个间隔，已追不上节拍：重新对齐，避免连续补帧。
            self.next = Instant::now() + self.interval;
        }
        Ok(Some(frame))
    }

    fn stop(&mut self) {
        self.surface = None;
    }
}

/// 抓取指定窗口客户区的一帧彩色图像（一次性，不复用资源）。
///
/// 公开用于冒烟测试；实时采集请用 [`WindowsCaptureStream`]。
pub fn capture_window(hwnd: u64) -> Result<RgbImage> {
    let hwnd = HWND(hwnd as usize as *mut c_void);
    if unsafe { IsIconic(hwnd).as_bool() } {
        return Err(CaptureError::Capture("游戏窗口已最小化".to_string()));
    }
    let (width, height) = client_size(hwnd)?;
    let mut surface = GdiSurface::new(width, height)?;
    surface.grab(hwnd)?;
    surface.to_rgb()
}

fn client_size(hwnd: HWND) -> Result<(u32, u32)> {
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client) }
        .map_err(|err| CaptureError::Capture(format!("读取客户区失败：{err}")))?;
    let width = (client.right - client.left).max(0) as u32;
    let height = (client.bottom - client.top).max(0) as u32;
    if width == 0 || height == 0 {
        return Err(CaptureError::Capture("窗口客户区为空".to_string()));
    }
    Ok((width, height))
}

/// 一块可复用的 GDI 抓帧表面：内存 DC + 32 位自上而下 DIB。
///
/// DIB section 直接暴露像素指针，`PrintWindow`/`BitBlt` 画进来后无需
/// `GetDIBits` 再拷一次；配合跨帧复用，避免每帧重建位图和分配大缓冲。
struct GdiSurface {
    width: u32,
    height: u32,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
}

// GDI 句柄与 DIB 指针只随采集流在线程间移动，从不并发共享，故可 Send。
unsafe impl Send for GdiSurface {}

impl GdiSurface {
    fn new(width: u32, height: u32) -> Result<Self> {
        unsafe {
            let mem_dc = CreateCompatibleDC(None);
            if mem_dc.0.is_null() {
                return Err(CaptureError::Capture("创建内存 DC 失败".to_string()));
            }
            let mut info = BITMAPINFO::default();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // 负高度 = 自上而下，行序与图像一致
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(
                Some(mem_dc),
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            ) {
                Ok(bitmap) => bitmap,
                Err(err) => {
                    let _ = DeleteDC(mem_dc);
                    return Err(CaptureError::Capture(format!("创建 DIB 位图失败：{err}")));
                }
            };
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(mem_dc);
                return Err(CaptureError::Capture("DIB 像素指针为空".to_string()));
            }
            let old = SelectObject(mem_dc, bitmap.into());
            Ok(Self {
                width,
                height,
                mem_dc,
                bitmap,
                old,
                bits: bits.cast(),
            })
        }
    }

    /// 把窗口内容画进 DIB：优先 `PrintWindow`（可抓被遮挡窗口），失败退回屏幕 `BitBlt`。
    fn grab(&mut self, hwnd: HWND) -> Result<()> {
        unsafe {
            let t_print = Instant::now();
            let printed = PrintWindow(hwnd, self.mem_dc, PRINT_WINDOW_FLAGS(3)).as_bool();
            let print_elapsed = t_print.elapsed();
            if !printed {
                let screen_dc = GetDC(None);
                if screen_dc.0.is_null() {
                    return Err(CaptureError::Capture("获取屏幕 DC 失败".to_string()));
                }
                let mut origin = POINT::default();
                let has_origin = ClientToScreen(hwnd, &mut origin).as_bool();
                let copied = has_origin
                    && BitBlt(
                        self.mem_dc,
                        0,
                        0,
                        self.width as i32,
                        self.height as i32,
                        Some(screen_dc),
                        origin.x,
                        origin.y,
                        SRCCOPY,
                    )
                    .is_ok();
                ReleaseDC(None, screen_dc);
                if !copied {
                    return Err(CaptureError::Capture(
                        "PrintWindow/BitBlt 均失败".to_string(),
                    ));
                }
            }
            if std::env::var_os("DECKTRACE_CAPTURE_PROFILE").is_some() {
                eprintln!(
                    "[capture] {}x{} print={:.2}ms{}",
                    self.width,
                    self.height,
                    print_elapsed.as_secs_f64() * 1000.0,
                    if printed { "" } else { " (bitblt 回退)" }
                );
            }
            Ok(())
        }
    }

    fn to_rgb(&self) -> Result<RgbImage> {
        let len = self.width as usize * self.height as usize * 4;
        // SAFETY: bits 指向 CreateDIBSection 分配的 width*height*4 字节，生命周期同 self。
        let src = unsafe { std::slice::from_raw_parts(self.bits, len) };
        bgra_to_rgb(src, self.width, self.height)
            .ok_or_else(|| CaptureError::Capture("BGRA 转 RGB 失败".to_string()))
    }
}

impl Drop for GdiSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem_dc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.mem_dc);
        }
    }
}

/// BGRA → RGB，按 3 字节像素并行重排。缓冲区由调用方保证 `>= w*h*4`。
fn bgra_to_rgb(buffer: &[u8], width: u32, height: u32) -> Option<RgbImage> {
    use rayon::prelude::*;

    let pixels = width as usize * height as usize;
    if buffer.len() < pixels * 4 {
        return None;
    }
    let mut data = Vec::with_capacity(pixels * 3);
    // SAFETY: 下面的 par_chunks_mut 会完整写入全部 pixels*3 字节后再返回。
    unsafe {
        data.set_len(pixels * 3);
    }
    data.par_chunks_mut(3)
        .zip(buffer[..pixels * 4].par_chunks(4))
        .for_each(|(dst, src)| {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
        });
    RgbImage::from_raw(width, height, data)
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgb;

    #[test]
    fn bgra_to_rgb_swaps_channel_order() {
        // 两个像素：BGRA=(10,20,30,255) 和 (1,2,3,255)，期望 RGB=(30,20,10)/(3,2,1)。
        let bgra = [10u8, 20, 30, 255, 1, 2, 3, 255];
        let image = bgra_to_rgb(&bgra, 2, 1).expect("转换应成功");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(&image.get_pixel(0, 0).0, &[30, 20, 10]);
        assert_eq!(&image.get_pixel(1, 0).0, &[3, 2, 1]);
    }

    #[test]
    fn bgra_to_rgb_rejects_short_buffer() {
        // 需要 4*4=16 字节，只给 8 字节应返回 None 而不是越界。
        let short = [0u8; 8];
        assert!(bgra_to_rgb(&short, 2, 2).is_none());
    }
}
