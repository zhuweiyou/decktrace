//! 采集子系统：trait、窗口选择、macOS/Windows 实现。

mod traits;

pub use traits::{CaptureError, CaptureStream, Platform, Result, WindowService, select_window};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub use macos::MacPlatform as PlatformImpl;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform as PlatformImpl;

/// 创建当前平台的采集实现。
pub fn create_platform() -> Result<Box<dyn Platform>> {
    Ok(Box::new(PlatformImpl::new()?))
}
