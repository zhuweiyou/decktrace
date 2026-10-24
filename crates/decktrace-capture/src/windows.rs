//! Windows 采集后端占位实现。
//!
//! 先保留 `Platform` / `WindowService` 结构，后续接 Windows Graphics Capture
//! 或 PrintWindow。当前所有操作都会返回明确的“尚未实现”。

use decktrace_core::{CaptureError, CaptureStream, Platform, WindowInfo, WindowService};

pub struct WindowsPlatform {
    windows: WindowsWindowService,
}

impl WindowsPlatform {
    pub fn new() -> Result<Self, CaptureError> {
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
        _window: &WindowInfo,
        _fps: f32,
    ) -> Result<Box<dyn CaptureStream>, CaptureError> {
        Err(CaptureError::Unsupported(
            "Windows 采集后端尚未实现".to_string(),
        ))
    }
}

pub struct WindowsWindowService;

impl WindowService for WindowsWindowService {
    fn list_windows(&self, _include_off_screen: bool) -> Result<Vec<WindowInfo>, CaptureError> {
        Err(CaptureError::Unsupported(
            "Windows 窗口枚举尚未实现".to_string(),
        ))
    }

    fn find_window(
        &self,
        _selector: &decktrace_core::WindowSelector,
    ) -> Result<WindowInfo, CaptureError> {
        Err(CaptureError::Unsupported(
            "Windows 窗口枚举尚未实现".to_string(),
        ))
    }
}
