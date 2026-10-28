//! 采集子系统：trait、窗口选择、macOS/Windows 实现。

mod traits;
mod types;

pub use traits::{CaptureError, CaptureStream, Platform, Result, WindowService, select_window};
pub use types::{RawFrame, WindowInfo};

#[cfg(not(target_os = "android"))]
pub mod adb;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub use macos::MacPlatform as PlatformImpl;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform as PlatformImpl;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct PlatformImpl;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl PlatformImpl {
    pub fn new() -> Result<Self> {
        Err(CaptureError::Unsupported(
            "当前平台没有采集实现".to_string(),
        ))
    }
}

/// 创建当前平台的采集实现。
pub fn create_platform() -> Result<Box<dyn Platform>> {
    Ok(Box::new(PlatformImpl::new()?))
}
